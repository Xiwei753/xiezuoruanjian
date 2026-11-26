// 待重试的原始证明命令 spool 与失败事实重试。
//
// Core 侧投影构造失败或事实文件写盘失败时，原始命令/revision 不能丢；
// 这些函数只做磁盘 spool 与内存失败集合，真正重试由章节 actor 在保存/恢复
// 前调用（Issue #1062 复审第 3 项）。

use std::path::Path;

use super::store::{self, DraftLocation};
use super::{ensure_state_or_new, lock_registry, DraftFact, DraftFactIndex, PENDING_INPUTS_DIR};

#[derive(Clone, Debug)]
pub(crate) struct PendingFactInput {
    pub event_id: String,
    pub revision: u64,
    pub session_id: u64,
    pub volume_id: String,
    pub generation: String,
    pub input_json: String,
}

/// 把 Core 投影构造失败的原始命令持久化到草稿目录，等待 actor 用同一
/// event ID 重试；同时把该 revision 记入内存失败集合，阻断更晚正文快照。
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
) -> Result<(), String> {
    let Some(location) = DraftLocation::new(projects_root, project_id, chapter_id) else {
        return Err(format!("invalid draft identity: {project_id}/{chapter_id}"));
    };
    if !ensure_state_or_new(&location, volume_id) {
        return Err(format!(
            "chapter draft state unavailable: {project_id}/{chapter_id}"
        ));
    }
    let dir = location.dir.join(PENDING_INPUTS_DIR);
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("create pending inputs dir {}: {error}", dir.display()))?;
    let record = serde_json::json!({
        "eventId": event_id,
        "revision": revision,
        "sessionId": session_id,
        "volumeId": volume_id,
        "generation": generation,
        "input": serde_json::from_str::<serde_json::Value>(input_json)
            .map_err(|error| format!("pending proof input invalid event={event_id}: {error}"))?,
    });
    let json = serde_json::to_string(&record)
        .map_err(|error| format!("encode pending proof input event={event_id}: {error}"))?;
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
        if !generation.is_empty() {
            state.generation = generation.to_string();
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

/// 读取待重试的原始命令（按 revision 升序）；坏文件跳过并保留在磁盘上。
pub(crate) fn pending_inputs(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
) -> Vec<PendingFactInput> {
    let Some(location) = DraftLocation::new(projects_root, project_id, chapter_id) else {
        return Vec::new();
    };
    let dir = location.dir.join(PENDING_INPUTS_DIR);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut inputs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
            log::warn!(
                "pending proof input corrupted, keep file: {}",
                path.display()
            );
            continue;
        };
        let event_id = value
            .get("eventId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        if event_id.is_empty() {
            continue;
        }
        inputs.push(PendingFactInput {
            event_id,
            revision: value
                .get("revision")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
            session_id: value
                .get("sessionId")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
            volume_id: value
                .get("volumeId")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            generation: value
                .get("generation")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            input_json: value
                .get("input")
                .map(serde_json::Value::to_string)
                .unwrap_or_default(),
        });
    }
    inputs.sort_by_key(|input| input.revision);
    inputs
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
