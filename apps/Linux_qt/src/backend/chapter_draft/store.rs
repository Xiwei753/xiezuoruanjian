// =============================================================================
// chapter_draft/store.rs — 草稿磁盘布局、编解码与状态加载
// =============================================================================
//
// 目录（app-meta 是 LocalOnly，不同步）：
//   <projects_root>/<project_id>/app-meta/chapter-drafts/<chapter_id>/
//     meta.json   text.txt   facts/<seq:08>-<id>.json
//
// 所有写入走 `writer_core::storage::atomic_write_string`；本模块只做磁盘 I/O
// 与编解码，不在 registry 锁内运行。meta 索引允许滞后（每 2s 才写），读取
// 时由 `merge_index` 扫描 facts/ 目录补齐，绝不把“索引为空”当空草稿。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::{DraftFactIndex, DraftState, DRAFT_SCHEMA_VERSION, MAX_MEMORY_FACT_INDEX};

pub(super) const META_FILE: &str = "meta.json";
pub(super) const TEXT_FILE: &str = "text.txt";
const FACTS_DIR: &str = "facts";

pub(super) fn is_safe_id(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value
            .chars()
            .any(|ch| matches!(ch, '/' | '\\' | '\0' | '\u{1f}'))
}

pub(super) fn workspace_key_of(projects_root: &Path) -> String {
    let absolute = if projects_root.is_absolute() {
        projects_root.to_path_buf()
    } else {
        std::env::current_dir().map_or_else(
            |_| projects_root.to_path_buf(),
            |cwd| cwd.join(projects_root),
        )
    };
    std::fs::canonicalize(&absolute)
        .unwrap_or(absolute)
        .to_string_lossy()
        .into_owned()
}

pub(super) fn drafts_root(projects_root: &Path, project_id: &str) -> PathBuf {
    projects_root
        .join(project_id)
        .join("app-meta")
        .join("chapter-drafts")
}

/// 一章草稿的身份与目录；`key` 含 workspace，跨 workspace 的 registry 条目
/// 不会互相覆盖。
pub(super) struct DraftLocation {
    pub(super) project_id: String,
    pub(super) chapter_id: String,
    pub(super) workspace_key: String,
    pub(super) projects_root: PathBuf,
    pub(super) dir: PathBuf,
    pub(super) key: String,
    legacy_path: PathBuf,
}

impl DraftLocation {
    pub(super) fn new(projects_root: &Path, project_id: &str, chapter_id: &str) -> Option<Self> {
        if !is_safe_id(project_id) || !is_safe_id(chapter_id) {
            return None;
        }
        let workspace_key = workspace_key_of(projects_root);
        let drafts_root = drafts_root(projects_root, project_id);
        Some(Self {
            project_id: project_id.to_string(),
            chapter_id: chapter_id.to_string(),
            projects_root: projects_root.to_path_buf(),
            dir: drafts_root.join(chapter_id),
            legacy_path: drafts_root.join(format!("{chapter_id}.json")),
            key: format!("{workspace_key}\u{1f}{project_id}\u{1f}{chapter_id}"),
            workspace_key,
        })
    }

    pub(super) fn legacy_path(&self) -> PathBuf {
        self.legacy_path.clone()
    }
}

// ── meta.json ─────────────────────────────────────────────────────────────

#[derive(Default, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct MetaData {
    pub(super) schema_version: u32,
    pub(super) project_id: String,
    pub(super) volume_id: String,
    pub(super) chapter_id: String,
    pub(super) workspace_key: String,
    pub(super) generation: String,
    pub(super) session_id: u64,
    pub(super) base_generation: String,
    pub(super) base_revision: u64,
    pub(super) base_sha256: String,
    pub(super) text_revision: u64,
    pub(super) text_sha256: String,
    pub(super) updated_at_ms: i64,
    pub(super) facts: Vec<DraftFactIndex>,
    /// Issue #1062 复审：已随 durable receipt 确认、随后才删除事实文件的
    /// event ID。meta 先写索引再删文件；两步之间崩溃时，缺失文件命中这里
    /// 就按“已确认”跳过，真正未确认的缺失事实仍然报错。
    pub(super) acked_event_ids: Vec<String>,
    /// 已确认到的正文 revision（诊断与防止 ack 记录无限增长）。
    pub(super) acked_through_revision: u64,
    /// 已应用过的 Core journal transaction ID：重放同一事务幂等，不回退 base。
    pub(super) applied_transactions: Vec<String>,
    /// 本次文档加载时的编辑器 revision / 基线摘要（编辑器坐标，见 #1062 复审）。
    pub(super) editor_base_revision: u64,
    pub(super) editor_base_sha256: String,
    pub(super) editor_base_session: u64,
    pub(super) committed_text_revision: u64,
    pub(super) committed_text_sha256: String,
}

impl MetaData {
    /// 索引里 seq 缺失/eventId 为空的条目不可用（等 facts/ 目录补齐）。
    pub(super) fn index(&self) -> Vec<DraftFactIndex> {
        self.facts
            .iter()
            .filter(|fact| fact.seq > 0 && !fact.event_id.is_empty())
            .cloned()
            .collect()
    }
}

pub(super) fn read_meta(dir: &Path, project_id: &str, chapter_id: &str) -> Option<MetaData> {
    read_meta_checked(dir, project_id, chapter_id).ok()
}

/// 区分“没有 meta”（Ok(None)）与“meta 存在但不可用/身份不符”（Err）。
///
/// Issue #1062 复审：恢复链必须把后者当成未决状态并保留磁盘文件，不能与
/// “从未有过草稿”混为一谈。
pub(super) fn read_meta_checked(
    dir: &Path,
    project_id: &str,
    chapter_id: &str,
) -> Result<MetaData, String> {
    let path = dir.join(META_FILE);
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Err(String::new()),
        Err(error) => {
            return Err(format!(
                "read chapter draft meta {}: {error}",
                path.display()
            ))
        }
    };
    let meta: MetaData = serde_json::from_str(&content).map_err(|error| {
        format!(
            "chapter draft meta invalid (kept): {}: {error}",
            path.display()
        )
    })?;
    if meta.schema_version != DRAFT_SCHEMA_VERSION
        || meta.project_id != project_id
        || meta.chapter_id != chapter_id
    {
        return Err(format!(
            "chapter draft meta identity/schema mismatch (kept): project={project_id} chapter={chapter_id} schema={}",
            meta.schema_version
        ));
    }
    Ok(meta)
}

pub(super) fn write_meta(dir: &Path, meta: &MetaData) -> Result<(), String> {
    let json = serde_json::to_string(meta)
        .map_err(|error| format!("encode chapter draft meta {}: {error}", dir.display()))?;
    writer_core::storage::atomic_write_string(&dir.join(META_FILE), &json)
        .map_err(|error| format!("write chapter draft meta {}: {error}", dir.display()))
}

/// 用内存状态构造 meta.json（正文摘要取已持久化的 text.txt 那一版，避免
/// meta 比正文新）。
pub(super) fn meta_from_state(
    location: &DraftLocation,
    state: &DraftState,
    text_revision: u64,
    text_sha256: &str,
) -> MetaData {
    MetaData {
        schema_version: DRAFT_SCHEMA_VERSION,
        project_id: state.project_id.clone(),
        volume_id: state.volume_id.clone(),
        chapter_id: state.chapter_id.clone(),
        workspace_key: location.workspace_key.clone(),
        generation: state.generation.clone(),
        session_id: state.session_id,
        base_generation: state.base_generation.clone(),
        base_revision: state.base_revision,
        base_sha256: state.base_sha256.clone(),
        text_revision,
        text_sha256: text_sha256.to_string(),
        updated_at_ms: state.updated_at_ms,
        facts: state.facts.clone(),
        acked_event_ids: state.acked_event_ids.clone(),
        acked_through_revision: state.acked_through_revision,
        applied_transactions: state.applied_transactions.clone(),
        editor_base_revision: state.editor_base_revision,
        editor_base_sha256: state.editor_base_sha256.clone(),
        editor_base_session: state.editor_base_session,
        committed_text_revision: state.committed_text_revision,
        committed_text_sha256: state.committed_text_sha256.clone(),
    }
}

// ── facts/<seq>-<id>.json ─────────────────────────────────────────────────

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct FactFile {
    event_id: String,
    revision: u64,
    #[serde(default)]
    session_id: u64,
    projection: serde_json::Value,
}

pub(super) struct ParsedFact {
    pub(super) event_id: String,
    pub(super) revision: u64,
    pub(super) session_id: u64,
    pub(super) projection_json: String,
}

pub(super) fn write_fact_file(
    dir: &Path,
    seq: u64,
    event_id: &str,
    revision: u64,
    session_id: u64,
    projection_json: &str,
) -> Result<(), String> {
    let projection: serde_json::Value = serde_json::from_str(projection_json)
        .map_err(|error| format!("draft fact projection invalid event={event_id}: {error}"))?;
    let json = serde_json::to_string(&FactFile {
        event_id: event_id.to_string(),
        revision,
        session_id,
        projection,
    })
    .map_err(|error| format!("encode draft fact event={event_id}: {error}"))?;
    let path = fact_file_path(dir, seq, event_id);
    writer_core::storage::atomic_write_string(&path, &json)
        .map_err(|error| format!("write draft fact {}: {error}", path.display()))
}

pub(super) fn read_fact_file(path: &Path) -> Result<ParsedFact, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|error| format!("read draft fact {}: {error}", path.display()))?;
    let file: FactFile = serde_json::from_str(&content)
        .map_err(|error| format!("draft fact corrupted {}: {error}", path.display()))?;
    if file.event_id.is_empty() || file.projection.is_null() {
        return Err(format!("draft fact incomplete: {}", path.display()));
    }
    Ok(ParsedFact {
        event_id: file.event_id,
        revision: file.revision,
        session_id: file.session_id,
        projection_json: file.projection.to_string(),
    })
}

fn facts_dir(dir: &Path) -> PathBuf {
    dir.join(FACTS_DIR)
}

fn event_id_file_part(event_id: &str) -> String {
    event_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

pub(super) fn fact_file_path(dir: &Path, seq: u64, event_id: &str) -> PathBuf {
    facts_dir(dir).join(format!("{seq:08}-{}.json", event_id_file_part(event_id)))
}

/// 列出 facts/ 下的分段文件（按 seq 升序）；目录不存在视为空。
pub(super) fn list_fact_files(dir: &Path) -> Result<Vec<(u64, PathBuf)>, String> {
    let facts = facts_dir(dir);
    let entries = match std::fs::read_dir(&facts) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("read draft facts dir {}: {error}", facts.display())),
    };
    let mut files = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("read draft facts dir {}: {error}", facts.display()))?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.ends_with(".json") {
            continue;
        }
        let Some(seq) = name
            .split_once('-')
            .and_then(|(head, _)| head.parse::<u64>().ok())
        else {
            return Err(format!(
                "draft fact file name has no seq: {}",
                path.display()
            ));
        };
        files.push((seq, path));
    }
    files.sort_by_key(|(seq, _)| *seq);
    Ok(files)
}

/// meta 索引与 facts/ 目录合并：索引缺失（meta 滞后）时读文件补齐。
///
/// Issue #1062 复审：`acked` 是已经随 durable receipt 确认的 event ID。ack
/// 决议先写进 meta，再删除事实文件；若删除与 meta 写入之间崩溃，索引里已经
/// 没有这些条目、目录里也可能已经没有文件——它们必须按已确认跳过。反过来，
/// 索引里有、目录里没有、且不在 acked 里的条目必须报错，不能把“文件被删/
/// 丢失”当成“已确认删除”静默跳过。目录里存在但索引没有的文件如果是 acked
/// 的残留（删除中断），同样跳过。
pub(super) fn merge_index(
    dir: &Path,
    index: &[DraftFactIndex],
    acked: &HashSet<String>,
) -> Result<Vec<DraftFactIndex>, String> {
    let files = list_fact_files(dir)?;
    let mut by_seq: HashMap<u64, DraftFactIndex> =
        index.iter().cloned().map(|fact| (fact.seq, fact)).collect();
    let present_seqs: HashSet<u64> = files.iter().map(|(seq, _)| *seq).collect();
    for (seq, fact) in &by_seq {
        if present_seqs.contains(seq) || acked.contains(fact.event_id.as_str()) {
            continue;
        }
        return Err(format!(
            "draft fact file missing: seq={seq} event={} dir={}",
            fact.event_id,
            dir.display()
        ));
    }
    for (seq, path) in files {
        if by_seq.contains_key(&seq) {
            continue;
        }
        let parsed = read_fact_file(&path)?;
        if acked.contains(parsed.event_id.as_str()) {
            continue;
        }
        by_seq.insert(
            seq,
            DraftFactIndex {
                event_id: parsed.event_id,
                revision: parsed.revision,
                session_id: parsed.session_id,
                after_text_sha256: super::projection_after_text_sha256(&parsed.projection_json),
                seq,
            },
        );
    }
    let mut facts: Vec<DraftFactIndex> = by_seq.into_values().collect();
    facts.sort_by_key(|fact| fact.seq);
    let mut seen = HashSet::new();
    facts.retain(|fact| seen.insert(fact.event_id.clone()));
    Ok(facts)
}

// ── text.txt ──────────────────────────────────────────────────────────────

pub(super) fn read_text(location: &DraftLocation) -> Option<String> {
    let path = location.dir.join(TEXT_FILE);
    match std::fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            log::warn!(
                "read chapter draft text failed: project={} chapter={} path={}: {error}",
                location.project_id,
                location.chapter_id,
                path.display()
            );
            None
        }
    }
}

pub(super) fn write_text(dir: &Path, text: &str) -> Result<(), String> {
    writer_core::storage::atomic_write_string(&dir.join(TEXT_FILE), text)
        .map_err(|error| format!("write chapter draft text {}: {error}", dir.display()))
}

// ── 状态加载与旧格式迁移 ──────────────────────────────────────────────────

/// 读取新格式草稿。
///
/// `Ok(None)` = 没有 meta.json；任何 meta/facts 读取失败都返回 Err，由恢复
/// 链显式报告并保留目录，绝不能降级成“空草稿”（Issue #1062 复审）。
/// 目录保持只读：失败路径不删除、不重写任何文件。
pub(super) fn read_new_state(
    location: &DraftLocation,
    trim: bool,
) -> Result<Option<DraftState>, String> {
    let meta = match read_meta_checked(&location.dir, &location.project_id, &location.chapter_id) {
        Ok(meta) => meta,
        Err(error) if error.is_empty() => return Ok(None),
        Err(error) => return Err(error),
    };
    let acked: HashSet<String> = meta.acked_event_ids.iter().cloned().collect();
    let mut facts = merge_index(&location.dir, &meta.index(), &acked).map_err(|error| {
        format!(
            "chapter draft facts unreadable (kept): project={} chapter={}: {error}",
            location.project_id, location.chapter_id
        )
    })?;
    facts.sort_by_key(|fact| fact.seq);
    let next_seq = facts.last().map_or(1, |fact| fact.seq.saturating_add(1));
    if trim && facts.len() > MAX_MEMORY_FACT_INDEX {
        facts.drain(0..facts.len() - MAX_MEMORY_FACT_INDEX);
    }
    // 重启后从磁盘 spool 重建待转换事件，不能只依赖进程内 failed_inputs。
    let failed_inputs = super::pending::pending_inputs(
        &location.projects_root,
        &location.project_id,
        &location.chapter_id,
    )
    .unwrap_or_default()
    .into_iter()
    .map(|input| (input.event_id, input.revision))
    .collect();
    Ok(Some(DraftState {
        project_id: location.project_id.clone(),
        volume_id: meta.volume_id,
        chapter_id: location.chapter_id.clone(),
        workspace_key: if meta.workspace_key.is_empty() {
            location.workspace_key.clone()
        } else {
            meta.workspace_key
        },
        generation: meta.generation,
        session_id: meta.session_id,
        base_generation: meta.base_generation,
        base_revision: meta.base_revision,
        base_sha256: meta.base_sha256,
        text: read_text(location),
        text_revision: meta.text_revision,
        text_sha256: meta.text_sha256.clone(),
        persisted_text_revision: meta.text_revision,
        persisted_text_sha256: meta.text_sha256,
        text_dirty: false,
        facts,
        next_seq,
        failed_facts: Vec::new(),
        failed_inputs,
        acked_event_ids: meta.acked_event_ids,
        acked_through_revision: meta.acked_through_revision,
        applied_transactions: meta.applied_transactions,
        editor_base_revision: meta.editor_base_revision,
        editor_base_sha256: meta.editor_base_sha256,
        editor_base_session: meta.editor_base_session,
        committed_text_revision: meta.committed_text_revision,
        committed_text_sha256: meta.committed_text_sha256,
        projects_root: location.projects_root.clone(),
        updated_at_ms: meta.updated_at_ms,
        meta_dirty: false,
        last_persist: None,
    }))
}

/// registry 加载入口：新格式优先，其次迁移旧格式单文件。
pub(super) fn load_state(location: &DraftLocation) -> Result<Option<DraftState>, String> {
    match read_new_state(location, true)? {
        Some(state) => Ok(Some(state)),
        None => Ok(migrate_legacy(location)),
    }
}

/// 旧格式单文件迁移：写 facts/ + text.txt + meta.json，全部成功后才删除旧文件。
pub(super) fn migrate_legacy(location: &DraftLocation) -> Option<DraftState> {
    let legacy_path = location.legacy_path();
    let legacy = read_legacy(&legacy_path, &location.project_id, &location.chapter_id)?;
    let (state, projections) = legacy_state(location, legacy);
    if location.dir.exists() {
        // 没有 meta.json 的目录只可能是上一次迁移中断的产物，旧文件才是事实来源。
        if let Err(error) = std::fs::remove_dir_all(&location.dir) {
            log::warn!(
                "clear partial chapter draft dir failed: project={} chapter={}: {error}",
                location.project_id,
                location.chapter_id
            );
        }
    }
    for (fact, projection) in state.facts.iter().zip(projections.iter()) {
        if let Err(error) = write_fact_file(
            &location.dir,
            fact.seq,
            &fact.event_id,
            fact.revision,
            fact.session_id,
            projection,
        ) {
            log::warn!(
                "migrate legacy chapter draft fact failed: project={} chapter={} event={}: {error}",
                location.project_id,
                location.chapter_id,
                fact.event_id
            );
            return None;
        }
    }
    if let Some(text) = state.text.as_deref() {
        if let Err(error) = write_text(&location.dir, text) {
            log::warn!(
                "migrate legacy chapter draft text failed: project={} chapter={}: {error}",
                location.project_id,
                location.chapter_id
            );
            return None;
        }
    }
    let meta = meta_from_state(location, &state, state.text_revision, &state.text_sha256);
    if let Err(error) = write_meta(&location.dir, &meta) {
        log::warn!(
            "migrate legacy chapter draft meta failed: project={} chapter={}: {error}",
            location.project_id,
            location.chapter_id
        );
        return None;
    }
    if let Err(error) = std::fs::remove_file(&legacy_path) {
        log::warn!(
            "remove migrated legacy chapter draft failed: project={} chapter={} path={}: {error}",
            location.project_id,
            location.chapter_id,
            legacy_path.display()
        );
    }
    log::info!(
        "migrated legacy chapter draft: project={} chapter={} facts={}",
        location.project_id,
        location.chapter_id,
        state.facts.len()
    );
    Some(state)
}

/// 章节草稿摘要（列表用；不读正文与 projection）。
pub(super) fn read_summary_from_dir(
    projects_root: &Path,
    dir: &Path,
    project_id: &str,
    chapter_id: &str,
) -> Result<Option<super::ChapterDraft>, String> {
    let meta = match read_meta_checked(dir, project_id, chapter_id) {
        Ok(meta) => meta,
        Err(error) if error.is_empty() => return Ok(None),
        Err(error) => return Err(error),
    };
    let file_count = list_fact_files(dir).map_or(0, |files| files.len());
    let fact_count = meta.index().len().max(file_count);
    // 只有 pending-input 的草稿也要被发现；spool 读坏时仍按“有待处理”计入，
    // 并把错误留给恢复 actor 显式报告（不能当成空目录）。
    let (pending_input_count, pending_input_error) =
        match super::pending::pending_inputs(projects_root, project_id, chapter_id) {
            Ok(inputs) if inputs.is_empty() => (0usize, None),
            Ok(inputs) => (inputs.len(), None),
            Err(error) => (
                super::pending::pending_input_file_count(dir).max(1),
                Some(error),
            ),
        };
    Ok(Some(super::ChapterDraft {
        volume_id: meta.volume_id,
        chapter_id: chapter_id.to_string(),
        base_generation: meta.base_generation,
        base_revision: meta.base_revision,
        base_sha256: meta.base_sha256,
        text_present: dir.join(TEXT_FILE).is_file(),
        text_revision: meta.text_revision,
        text_sha256: meta.text_sha256,
        fact_count,
        pending_input_count,
        pending_input_error,
        draft_error: None,
        session_id: meta.session_id,
        generation: meta.generation,
        updated_at_ms: meta.updated_at_ms,
    }))
}

/// 草稿目录里是否还有需要恢复的磁盘内容（meta 缺失时也据此发现草稿）。
pub(super) fn draft_dir_has_content(dir: &Path) -> bool {
    dir.join(TEXT_FILE).is_file()
        || dir.join(FACTS_DIR).is_dir()
        || dir.join(super::PENDING_INPUTS_DIR).is_dir()
        || dir.join(super::ack::ACK_DIR).is_dir()
}

/// 为“meta 不可读/缺失但有磁盘内容”的目录合成恢复候选，保留 path 与错误。
pub(super) fn unreadable_summary(
    dir: &Path,
    project_id: &str,
    chapter_id: &str,
    error: String,
) -> super::ChapterDraft {
    let fact_count = list_fact_files(dir).map_or(0, |files| files.len());
    super::ChapterDraft {
        volume_id: String::new(),
        chapter_id: chapter_id.to_string(),
        base_generation: String::new(),
        base_revision: 0,
        base_sha256: String::new(),
        text_present: dir.join(TEXT_FILE).is_file(),
        text_revision: 0,
        text_sha256: String::new(),
        fact_count,
        pending_input_count: super::pending::pending_input_file_count(dir),
        pending_input_error: None,
        draft_error: Some(if error.is_empty() {
            format!(
                "chapter draft meta missing (kept): project={project_id} chapter={chapter_id} path={}",
                dir.display()
            )
        } else {
            format!("{error} (path={})", dir.display())
        }),
        session_id: 0,
        generation: String::new(),
        updated_at_ms: 0,
    }
}

// ── 旧格式 <chapter_id>.json ──────────────────────────────────────────────

#[derive(Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct LegacyFile {
    project_id: String,
    volume_id: String,
    chapter_id: String,
    base_generation: String,
    base_revision: u64,
    base_sha256: String,
    text: Option<String>,
    text_revision: u64,
    text_sha256: String,
    updated_at_ms: i64,
    facts: Vec<LegacyFact>,
}

#[derive(Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct LegacyFact {
    event_id: String,
    revision: u64,
    projection: serde_json::Value,
}

pub(super) struct LegacyData {
    pub(super) volume_id: String,
    pub(super) base_generation: String,
    pub(super) base_revision: u64,
    pub(super) base_sha256: String,
    pub(super) text: Option<String>,
    pub(super) text_revision: u64,
    pub(super) text_sha256: String,
    pub(super) updated_at_ms: i64,
    /// (索引, projection JSON)
    pub(super) facts: Vec<(DraftFactIndex, String)>,
}

pub(super) fn read_legacy(path: &Path, project_id: &str, chapter_id: &str) -> Option<LegacyData> {
    let content = std::fs::read_to_string(path).ok()?;
    let file: LegacyFile = match serde_json::from_str(&content) {
        Ok(file) => file,
        Err(error) => {
            log::warn!(
                "legacy chapter draft invalid: path={}: {error}",
                path.display()
            );
            return None;
        }
    };
    if file.project_id != project_id || file.chapter_id != chapter_id {
        log::warn!(
            "legacy chapter draft identity mismatch, keep file: project={project_id} chapter={chapter_id} path={}",
            path.display()
        );
        return None;
    }
    let mut facts = Vec::with_capacity(file.facts.len());
    for item in file.facts {
        if item.event_id.is_empty() || item.projection.is_null() {
            continue;
        }
        let seq = u64::try_from(facts.len()).unwrap_or(0).saturating_add(1);
        facts.push((
            DraftFactIndex {
                event_id: item.event_id,
                revision: item.revision,
                session_id: 0,
                after_text_sha256: super::projection_after_text_sha256(
                    &item.projection.to_string(),
                ),
                seq,
            },
            item.projection.to_string(),
        ));
    }
    Some(LegacyData {
        volume_id: file.volume_id,
        base_generation: file.base_generation,
        base_revision: file.base_revision,
        base_sha256: file.base_sha256,
        text: file.text,
        text_revision: file.text_revision,
        text_sha256: file.text_sha256,
        updated_at_ms: file.updated_at_ms,
        facts,
    })
}

/// 旧格式只读解码结果（快照/列表用，不写盘）。
pub(super) fn legacy_state(
    location: &DraftLocation,
    legacy: LegacyData,
) -> (DraftState, Vec<String>) {
    let mut facts = Vec::with_capacity(legacy.facts.len());
    let mut projections = Vec::with_capacity(legacy.facts.len());
    for (fact, projection) in legacy.facts {
        facts.push(fact);
        projections.push(projection);
    }
    let next_seq = facts.last().map_or(1, |fact| fact.seq.saturating_add(1));
    let state = DraftState {
        project_id: location.project_id.clone(),
        volume_id: legacy.volume_id,
        chapter_id: location.chapter_id.clone(),
        workspace_key: location.workspace_key.clone(),
        generation: String::new(),
        session_id: 0,
        base_generation: legacy.base_generation,
        base_revision: legacy.base_revision,
        base_sha256: legacy.base_sha256,
        text: legacy.text,
        text_revision: legacy.text_revision,
        text_sha256: legacy.text_sha256.clone(),
        persisted_text_revision: legacy.text_revision,
        persisted_text_sha256: legacy.text_sha256,
        text_dirty: false,
        facts,
        next_seq,
        failed_facts: Vec::new(),
        failed_inputs: Vec::new(),
        acked_event_ids: Vec::new(),
        applied_transactions: Vec::new(),
        editor_base_revision: 0,
        editor_base_sha256: String::new(),
        editor_base_session: 0,
        committed_text_revision: 0,
        committed_text_sha256: String::new(),
        acked_through_revision: 0,
        projects_root: location.projects_root.clone(),
        updated_at_ms: legacy.updated_at_ms,
        meta_dirty: false,
        last_persist: None,
    };
    (state, projections)
}
