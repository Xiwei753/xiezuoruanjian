// 待重试的原始证明命令 spool 与失败事实重试。
//
// Core 侧投影构造失败或事实文件写盘失败时，原始命令/revision 不能丢；
// 这些函数只做磁盘 spool 与内存失败集合，真正重试由章节 actor 在保存/恢复
// 前调用（Issue #1062 复审第 3 项）。

use std::path::Path;

use super::store::{self, DraftLocation};
use super::{ensure_state_or_new, lock_registry, DraftFact, DraftFactIndex, PENDING_INPUTS_DIR};

const SPOOL_SCHEMA_VERSION: u32 = 2;

/// 尚未构造出证明投影的原始编辑命令（raw accepted event）。
#[derive(Clone, Debug)]
pub(crate) struct PendingFactInput {
    pub event_id: String,
    pub revision: u64,
    pub session_id: u64,
    pub volume_id: String,
    pub generation: String,
    pub base_generation: String,
    pub base_revision: u64,
    pub base_sha256: String,
    pub input_json: String,
    pub path: String,
}

/// spool 文件的结构；schemaVersion 1 是早先没有 project/base 字段的版本。
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct SpoolRecord {
    schema_version: u32,
    project_id: String,
    chapter_id: String,
    volume_id: String,
    session_id: u64,
    generation: String,
    base_generation: String,
    base_revision: u64,
    base_sha256: String,
    event_id: String,
    revision: u64,
    input: serde_json::Value,
}

impl Default for SpoolRecord {
    fn default() -> Self {
        Self {
            schema_version: 1,
            project_id: String::new(),
            chapter_id: String::new(),
            volume_id: String::new(),
            session_id: 0,
            generation: String::new(),
            base_generation: String::new(),
            base_revision: 0,
            base_sha256: String::new(),
            event_id: String::new(),
            revision: 0,
            input: serde_json::Value::Null,
        }
    }
}

/// 把一条原始编辑命令写进 spool：先持久化“已接受”的事件身份，再构造投影。
///
/// Issue #1062 复审：首次事件（meta.json 还不存在）、投影构造失败、事实文件
/// 写盘失败都必须有可跨重启恢复的原始记录，不能只留在内存 `failed_facts`。
/// 同时先写出可恢复的 meta 事务头，避免“facts/ 有事件但 meta 缺失”让恢复
/// 列表完全看不到该草稿。
#[allow(clippy::too_many_arguments)]
pub(crate) fn spool_failed_input(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
    volume_id: &str,
    session_id: u64,
    generation: &str,
    revision: u64,
    event_id: &str,
    input_json: &str,
    base: (&str, u64, &str),
) -> Result<(), String> {
    let Some(location) = DraftLocation::new(projects_root, project_id, chapter_id) else {
        return Err(format!("invalid draft identity: {project_id}/{chapter_id}"));
    };
    if !ensure_state_or_new(&location, volume_id) {
        return Err(format!(
            "chapter draft state unavailable: {project_id}/{chapter_id}"
        ));
    }
    // 先写字事务头：即使随后立刻崩溃，恢复列表也能通过 meta 发现这一章。
    super::ensure_meta_header(&location, volume_id, Some(base))?;
    let input = serde_json::from_str::<serde_json::Value>(input_json)
        .map_err(|error| format!("pending proof input invalid event={event_id}: {error}"))?;
    let record = SpoolRecord {
        schema_version: SPOOL_SCHEMA_VERSION,
        project_id: project_id.to_string(),
        chapter_id: chapter_id.to_string(),
        volume_id: volume_id.to_string(),
        session_id,
        generation: generation.to_string(),
        base_generation: base.0.to_string(),
        base_revision: base.1,
        base_sha256: base.2.to_string(),
        event_id: event_id.to_string(),
        revision,
        input,
    };
    let json = serde_json::to_string(&record)
        .map_err(|error| format!("encode pending proof input event={event_id}: {error}"))?;
    let dir = location.dir.join(PENDING_INPUTS_DIR);
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("create pending inputs dir {}: {error}", dir.display()))?;
    let path = pending_input_path(&location.dir, event_id);
    writer_core::storage::atomic_write_string(&path, &json)
        .map_err(|error| format!("write pending proof input {}: {error}", path.display()))?;
    if let Some(state) = lock_registry().get_mut(&location.key) {
        if !state
            .failed_inputs
            .iter()
            .any(|(existing, _)| existing == event_id)
        {
            state.failed_inputs.push((event_id.to_string(), revision));
        }
        state.session_id = session_id;
        if !state.generation.is_empty() {
            // 已有草稿 generation 时不能被后到事件覆盖。
        } else if !generation.is_empty() {
            state.generation = generation.to_string();
        }
        if state.base_generation.is_empty()
            && state.base_revision == 0
            && (!base.0.is_empty() || base.1 > 0)
        {
            state.base_generation = base.0.to_string();
            state.base_revision = base.1;
            state.base_sha256 = base.2.to_string();
        }
        state.meta_dirty = true;
    }
    Ok(())
}

fn pending_input_path(dir: &Path, event_id: &str) -> std::path::PathBuf {
    let safe: String = event_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    dir.join(PENDING_INPUTS_DIR).join(format!("{safe}.json"))
}

/// 读取待重试的原始命令（按 revision 升序）。
///
/// 任何不可读/损坏/身份不一致的 spool 文件都保留原 path 并返回 Err；
/// 调用方（保存 actor / 恢复）必须把它当成“仍有未提交证明”，不能当成
/// “没有待提交事件”继续空批次保存（Issue #1062 复审第 2 项）。
pub(crate) fn pending_inputs(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
) -> Result<Vec<PendingFactInput>, String> {
    let Some(location) = DraftLocation::new(projects_root, project_id, chapter_id) else {
        return Err(format!("invalid draft identity: {project_id}/{chapter_id}"));
    };
    let dir = location.dir.join(PENDING_INPUTS_DIR);
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(format!(
                "read pending proof inputs dir {}: {error}",
                dir.display()
            ))
        }
    };
    let mut inputs: Vec<PendingFactInput> = Vec::new();
    for entry in entries {
        let entry = entry
            .map_err(|error| format!("read pending proof inputs dir {}: {error}", dir.display()))?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let content = std::fs::read_to_string(&path)
            .map_err(|error| format!("read pending proof input {}: {error}", path.display()))?;
        let record: SpoolRecord = serde_json::from_str(&content).map_err(|error| {
            format!(
                "pending proof input corrupted (kept): {}: {error}",
                path.display()
            )
        })?;
        if record.event_id.is_empty() || record.input.is_null() {
            return Err(format!(
                "pending proof input incomplete (kept): {}",
                path.display()
            ));
        }
        if (!record.project_id.is_empty() && record.project_id != project_id)
            || (!record.chapter_id.is_empty() && record.chapter_id != chapter_id)
        {
            return Err(format!(
                "pending proof input identity mismatch (kept): path={} project={} chapter={}",
                path.display(),
                record.project_id,
                record.chapter_id
            ));
        }
        inputs.push(PendingFactInput {
            event_id: record.event_id,
            revision: record.revision,
            session_id: record.session_id,
            volume_id: record.volume_id,
            generation: record.generation,
            base_generation: record.base_generation,
            base_revision: record.base_revision,
            base_sha256: record.base_sha256,
            input_json: record.input.to_string(),
            path: path.to_string_lossy().into_owned(),
        });
    }
    inputs.sort_by(|left, right| {
        left.revision
            .cmp(&right.revision)
            .then_with(|| left.event_id.cmp(&right.event_id))
    });
    let mut seen = std::collections::HashSet::new();
    inputs.retain(|input| seen.insert(input.event_id.clone()));
    Ok(inputs)
}

/// 原始命令重试成功（或已确认无证明）后，删除 spool 文件与内存失败标记。
pub(crate) fn clear_pending_input(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
    event_id: &str,
) {
    let Some(location) = DraftLocation::new(projects_root, project_id, chapter_id) else {
        return;
    };
    let path = pending_input_path(&location.dir, event_id);
    if let Err(error) = std::fs::remove_file(&path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            log::warn!("remove pending proof input {}: {error}", path.display());
        }
    }
    if let Some(state) = lock_registry().get_mut(&location.key) {
        state
            .failed_inputs
            .retain(|(existing, _)| existing != event_id);
        state.meta_dirty = true;
    }
}

/// 重试所有写盘失败的事实；仍失败时返回首个错误。
///
/// 失败事件与保存命令同属一个 actor 线程，因此这里不需要额外锁顺序；
/// 成功重试后重新进入索引，失败则继续保留（正文快照会被
/// [`freeze_batch_for`] 拦在失败 revision 之前）。
pub(super) fn retry_failed_facts(location: &DraftLocation) -> Result<(), String> {
    let pending: Vec<(u64, DraftFact)> = {
        let drafts = lock_registry();
        drafts
            .get(&location.key)
            .map(|state| state.failed_facts.clone())
            .unwrap_or_default()
    };
    for (seq, fact) in pending {
        {
            let mut drafts = lock_registry();
            let Some(state) = drafts.get_mut(&location.key) else {
                return Err(format!(
                    "chapter draft state unavailable: {}/{}",
                    location.project_id, location.chapter_id
                ));
            };
            if state
                .facts
                .iter()
                .any(|existing| existing.event_id == fact.event_id)
            {
                state
                    .failed_facts
                    .retain(|(_, existing)| existing.event_id != fact.event_id);
                continue;
            }
            state.facts.push(DraftFactIndex {
                event_id: fact.event_id.clone(),
                revision: fact.revision,
                session_id: fact.session_id,
                seq,
            });
            state.trim_index();
            state.meta_dirty = true;
        }
        if let Err(error) = store::write_fact_file(
            &location.dir,
            seq,
            &fact.event_id,
            fact.revision,
            fact.session_id,
            &fact.projection_json,
        ) {
            if let Some(state) = lock_registry().get_mut(&location.key) {
                state
                    .facts
                    .retain(|existing| existing.event_id != fact.event_id);
            }
            return Err(format!(
                "retry draft fact failed: event={} revision={}: {error}",
                fact.event_id, fact.revision
            ));
        }
        if let Some(state) = lock_registry().get_mut(&location.key) {
            state
                .failed_facts
                .retain(|(_, existing)| existing.event_id != fact.event_id);
        }
    }
    Ok(())
}
