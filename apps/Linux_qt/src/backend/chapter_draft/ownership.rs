// =============================================================================
// chapter_draft/ownership.rs — Core 草稿 ack 与本地草稿的归属判定
// =============================================================================
//
// Issue #1062 评论 #7209683560 第 1 项：Core journal 里的 draft ack 必须精确
// 应用到它所属的本地草稿/归档副本。对账前先判断归属，避免在空目录或同章新
// 会话草稿上写同 ID 的 pending-ack，把旧 base 应用到新事务或悄悄丢掉归档
// 副本的重试入口。

use std::collections::HashSet;
use std::path::Path;

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
        // 有真实文件却没有可读 meta：这是一份未决草稿，不能当成“本地没有
        // 草稿”去确认 Core ack（那样会丢掉尚未应用的事务内容）。
        if store::draft_dir_has_content(&location.dir) {
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
