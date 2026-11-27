// =============================================================================
// chapter_draft/ownership.rs — Core 草稿 ack 与本地草稿的归属判定
// =============================================================================
//
// Issue #1062 评论 #7209683560 第 1 项：Core journal 里的 draft ack 必须精确
// 应用到它所属的本地草稿/归档副本。对账前先判断归属，避免在空目录或同章新
// 会话草稿上写同 ID 的 pending-ack，把旧 base 应用到新事务或悄悄丢掉归档
// 副本的重试入口。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::store::{self, DraftLocation};

/// Core journal 的一笔草稿 ack 与本地活动草稿的归属关系。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DraftEventOwnership {
    /// 本地没有草稿（目录不存在或没有可恢复事务内容）。
    NoDraft,
    /// 本地草稿拥有全部事件：ack 可以精确应用到这个事务。
    Owns,
    /// 本地草稿存在但事件不属于它（例如同章的新会话草稿）：绝不能把旧 ack
    /// 应用到新槽位。
    Foreign,
}

pub(crate) fn draft_event_ownership(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
    event_ids: &[String],
) -> Result<DraftEventOwnership, String> {
    let Some(location) = DraftLocation::new(projects_root, project_id, chapter_id) else {
        return Ok(DraftEventOwnership::NoDraft);
    };
    if !location.dir.exists() {
        return Ok(DraftEventOwnership::NoDraft);
    }
    let Some(state) = store::read_new_state(&location, false)? else {
        // terminal 已经结算：事务完成，ack 不需要再应用到本地。
        if store::is_terminal(&location) {
            return Ok(DraftEventOwnership::NoDraft);
        }
        // 只有事务本体还在时才算“未决草稿”；只剩真实 pending-ack 的槽位
        // 没有可应用的正文/事实，对账会把它精确消费到 Core journal。
        if store::draft_dir_has_transaction_content(&location.dir) {
            return Err(format!(
                "chapter draft meta unreadable (kept): project={project_id} chapter={chapter_id}"
            ));
        }
        return Ok(DraftEventOwnership::NoDraft);
    };
    if event_ids.is_empty() {
        return Ok(DraftEventOwnership::Owns);
    }
    let known: HashSet<&str> = state
        .facts
        .iter()
        .map(|fact| fact.event_id.as_str())
        .chain(state.acked_event_ids.iter().map(String::as_str))
        .chain(
            state
                .failed_facts
                .iter()
                .map(|(_, fact)| fact.event_id.as_str()),
        )
        .chain(
            state
                .failed_inputs
                .iter()
                .map(|(event_id, _)| event_id.as_str()),
        )
        .collect();
    Ok(if event_ids.iter().all(|id| known.contains(id.as_str())) {
        DraftEventOwnership::Owns
    } else {
        DraftEventOwnership::Foreign
    })
}

/// 归档区某章的事件归属：返回拥有这些 event ID 的归档目录。
///
/// Issue #1062 评论 #7210360212 第 1 项：对账不能因为“同章还有别的归档事务”
/// 就跳过这一章，必须按具体事务判定；只有事件明确属于仍在归档区的某份草稿
/// 时才延后这一笔 Core ack。材料读坏返回 Err（调用方保留 journal）。
pub(crate) fn archived_draft_ownership(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
    event_ids: &[String],
) -> Result<Option<String>, String> {
    if event_ids.is_empty() {
        return Ok(None);
    }
    let mut unreadable: Option<String> = None;
    for candidate in super::list_retries(projects_root, project_id) {
        if candidate.marker.chapter_id != chapter_id {
            continue;
        }
        let dir = PathBuf::from(&candidate.marker.archived_path);
        match archived_known_event_ids(&dir, project_id, chapter_id) {
            Ok(known) => {
                if event_ids.iter().all(|id| known.contains(id.as_str())) {
                    return Ok(Some(candidate.marker.archived_path));
                }
            }
            Err(error) => {
                unreadable.get_or_insert(error);
            }
        }
    }
    match unreadable {
        Some(error) => Err(error),
        None => Ok(None),
    }
}

fn archived_known_event_ids(
    dir: &Path,
    project_id: &str,
    chapter_id: &str,
) -> Result<HashSet<String>, String> {
    let mut known: HashSet<String> = HashSet::new();
    match store::read_meta_checked(dir, project_id, chapter_id) {
        Ok(meta) => {
            for fact in &meta.facts {
                known.insert(fact.event_id.clone());
            }
            known.extend(meta.acked_event_ids.iter().cloned());
        }
        Err(error) if error.is_empty() => {}
        Err(error) => {
            return Err(format!(
                "read archived draft meta {}: {error}",
                dir.display()
            ))
        }
    }
    for (_, path) in store::list_fact_files(dir)? {
        known.insert(store::read_fact_file(&path)?.event_id);
    }
    known.extend(archived_pending_event_ids(dir)?);
    Ok(known)
}

/// 归档目录里 spool 的原始事件 ID（尚未转换成事实的编辑事实）。
fn archived_pending_event_ids(dir: &Path) -> Result<Vec<String>, String> {
    let pending = dir.join(super::PENDING_INPUTS_DIR);
    let entries = match std::fs::read_dir(&pending) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(format!(
                "read archived pending inputs {}: {error}",
                pending.display()
            ))
        }
    };
    let mut ids = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let content = std::fs::read_to_string(&path)
            .map_err(|error| format!("read archived pending input {}: {error}", path.display()))?;
        let value: serde_json::Value = serde_json::from_str(&content).map_err(|error| {
            format!(
                "archived pending input corrupted (kept): {}: {error}",
                path.display()
            )
        })?;
        if let Some(event_id) = value.get("eventId").and_then(|value| value.as_str()) {
            ids.push(event_id.to_string());
        }
    }
    Ok(ids)
}
