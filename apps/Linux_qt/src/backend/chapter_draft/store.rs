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
    let path = dir.join(META_FILE);
    let content = std::fs::read_to_string(&path).ok()?;
    let meta: MetaData = match serde_json::from_str(&content) {
        Ok(meta) => meta,
        Err(error) => {
            log::warn!(
                "chapter draft meta invalid: path={}: {error}",
                path.display()
            );
            return None;
        }
    };
    if meta.schema_version != DRAFT_SCHEMA_VERSION
        || meta.project_id != project_id
        || meta.chapter_id != chapter_id
    {
        log::warn!(
            "chapter draft meta identity/schema mismatch, skip: project={project_id} chapter={chapter_id} schema={}",
            meta.schema_version
        );
        return None;
    }
    Some(meta)
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
/// 索引里有、目录里没有的条目必须保留：冻结时读不到文件要报错，不能把
/// “文件被删/丢失”当成“已确认删除”静默跳过（确认删除由 mark_committed
/// 同时删文件并重写索引）。
pub(super) fn merge_index(
    dir: &Path,
    index: &[DraftFactIndex],
) -> Result<Vec<DraftFactIndex>, String> {
    let files = list_fact_files(dir)?;
    let mut by_seq: HashMap<u64, DraftFactIndex> =
        index.iter().cloned().map(|fact| (fact.seq, fact)).collect();
    for (seq, path) in files {
        if by_seq.contains_key(&seq) {
            continue;
        }
        let parsed = read_fact_file(&path)?;
        by_seq.insert(
            seq,
            DraftFactIndex {
                event_id: parsed.event_id,
                revision: parsed.revision,
                session_id: parsed.session_id,
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

/// 读取新格式草稿；没有 meta.json 时返回 None。
pub(super) fn read_new_state(location: &DraftLocation, trim: bool) -> Option<DraftState> {
    let meta = read_meta(&location.dir, &location.project_id, &location.chapter_id)?;
    let mut facts = match merge_index(&location.dir, &meta.index()) {
        Ok(facts) => facts,
        Err(error) => {
            log::warn!(
                "chapter draft facts unreadable: project={} chapter={}: {error}",
                location.project_id,
                location.chapter_id
            );
            meta.index()
        }
    };
    facts.sort_by_key(|fact| fact.seq);
    let next_seq = facts.last().map_or(1, |fact| fact.seq.saturating_add(1));
    if trim && facts.len() > MAX_MEMORY_FACT_INDEX {
        facts.drain(0..facts.len() - MAX_MEMORY_FACT_INDEX);
    }
    Some(DraftState {
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
        updated_at_ms: meta.updated_at_ms,
        meta_dirty: false,
        last_persist: None,
    })
}

/// registry 加载入口：新格式优先，其次迁移旧格式单文件。
pub(super) fn load_state(location: &DraftLocation) -> Option<DraftState> {
    read_new_state(location, true).or_else(|| migrate_legacy(location))
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
    dir: &Path,
    project_id: &str,
    chapter_id: &str,
) -> Option<super::ChapterDraft> {
    let meta = read_meta(dir, project_id, chapter_id)?;
    let file_count = list_fact_files(dir).map_or(0, |files| files.len());
    let fact_count = meta.index().len().max(file_count);
    Some(super::ChapterDraft {
        volume_id: meta.volume_id,
        chapter_id: chapter_id.to_string(),
        base_generation: meta.base_generation,
        base_revision: meta.base_revision,
        base_sha256: meta.base_sha256,
        text_present: dir.join(TEXT_FILE).is_file(),
        text_revision: meta.text_revision,
        text_sha256: meta.text_sha256,
        fact_count,
        session_id: meta.session_id,
        generation: meta.generation,
        updated_at_ms: meta.updated_at_ms,
    })
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
        updated_at_ms: legacy.updated_at_ms,
        meta_dirty: false,
        last_persist: None,
    };
    (state, projections)
}
