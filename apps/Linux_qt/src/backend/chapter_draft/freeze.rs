// 按 snapshot/session/generation 冻结连续事实前缀。

use std::collections::HashSet;
use std::path::Path;

use super::pending;
use super::store::{self, DraftLocation};
use super::{ensure_state_result, lock_registry, DraftFactIndex, LoadedFact, MIXED_SESSION_ERROR};

pub(crate) fn freeze_batch_for(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
    session_id: u64,
    generation: &str,
    snapshot_revision: u64,
) -> Result<Vec<LoadedFact>, String> {
    let Some(location) = DraftLocation::new(projects_root, project_id, chapter_id) else {
        return Err(format!("invalid draft identity: {project_id}/{chapter_id}"));
    };
    // 先把 Core 已确认、草稿侧尚未完成的 ack 幂等补做掉；否则旧事件会以
    // 旧 base 的 before digest 再次进入批次，永久 PROOF_BATCH_MISMATCH。
    super::replay_pending_acks(projects_root, project_id, chapter_id)?;
    match ensure_state_result(&location) {
        Ok(true) => {}
        Ok(false) => {
            if location.legacy_path().exists() {
                return Err(format!(
                    "legacy draft cannot migrate: {project_id}/{chapter_id}"
                ));
            }
            return Ok(Vec::new());
        }
        // 磁盘事实/索引读不出来时是“未决”，绝不能退化成空批次通过保存。
        Err(error) => {
            return Err(format!(
                "chapter draft unreadable (kept): {project_id}/{chapter_id}: {error}"
            ))
        }
    }
    // 先把上一次写盘失败的事实重试掉；仍失败就不能继续，否则会带着
    // 缺失事件把正文提交 durable（Issue #1062 复审第 3 项）。
    pending::retry_failed_facts(&location)?;
    let (index, draft_generation, acked, failed_revisions) = {
        let drafts = lock_registry();
        let Some(state) = drafts.get(&location.key) else {
            return Ok(Vec::new());
        };
        let acked: HashSet<String> = state.acked_event_ids.iter().cloned().collect();
        let failed: Vec<u64> = state
            .failed_facts
            .iter()
            .map(|(_, fact)| fact.revision)
            .chain(state.failed_inputs.iter().map(|(_, revision)| *revision))
            .collect();
        (state.facts.clone(), state.generation.clone(), acked, failed)
    };
    if let Some(revision) = failed_revisions
        .iter()
        .copied()
        .filter(|revision| *revision <= snapshot_revision)
        .min()
    {
        return Err(format!(
            "proof fact persistence failed at revision {revision}: project={project_id} chapter={chapter_id}"
        ));
    }
    // 磁盘上的 pending-inputs 与进程内 failed_facts 一样是“未转换的原始
    // 事件”：重启后内存失败列表为空，但 spool 文件仍在。任何 revision 不晚于
    // 本次快照的待转换事件都必须阻断保存，不能当作“本来没有证明”。
    let pending_inputs = pending::pending_inputs(projects_root, project_id, chapter_id)?;
    if let Some(input) = pending_inputs
        .iter()
        .find(|input| input.revision <= snapshot_revision)
    {
        return Err(format!(
            "pending proof event not projected: event={} revision={} path={} project={project_id} chapter={chapter_id}",
            input.event_id, input.revision, input.path
        ));
    }
    let merged = store::merge_index(&location.dir, &index, &acked)?;
    let selected = select_prefix(
        &merged,
        session_id,
        &draft_generation,
        generation,
        snapshot_revision,
    )?;
    // 空前缀但草稿还有事实：这些事实都比快照新（或快照缺失）。不能返回空
    // 批次——保存成功后调用方会据此删掉整章草稿，未提交证明就丢了。
    if selected.is_empty() && !merged.is_empty() {
        return Err(format!(
            "draft facts are beyond snapshot {snapshot_revision}: project={project_id} chapter={chapter_id}"
        ));
    }
    // 本次快照之前发生过编辑、但最后一条事实的 revision 还落在快照之后
    // （例如事件上报丢失/被丢弃）：不能把“找不到”当成“本来没有”，否则
    // 编辑后的正文会带着不完整的证明链落盘。只有事实为空（无变化或该
    // 路径本来不产生事件）才允许无证明提交。
    if let Some(last) = selected.last() {
        if last.revision < snapshot_revision {
            return Err(format!(
                "proof events not current: last event revision {} < snapshot {snapshot_revision} project={project_id} chapter={chapter_id}",
                last.revision
            ));
        }
    }
    let mut batch = Vec::with_capacity(selected.len());
    let mut previous_new_revision: Option<u64> = None;
    for fact in selected {
        let path = store::fact_file_path(&location.dir, fact.seq, &fact.event_id);
        let parsed = store::read_fact_file(&path)?;
        if parsed.event_id != fact.event_id
            || parsed.revision != fact.revision
            || parsed.session_id != fact.session_id
        {
            return Err(format!(
                "draft fact mismatch: {project_id}/{chapter_id} event={} path={}",
                fact.event_id,
                path.display()
            ));
        }
        // Issue #1069：不得只看最后一条 revision。相邻事实必须首尾相接，
        // 中间缺失时后面的正文不能带着断开的证明链提交成 durable。
        let projection: serde_json::Value = serde_json::from_str(&parsed.projection_json)
            .map_err(|error| {
                format!(
                    "draft fact projection decode failed: event={} path={}: {error}",
                    parsed.event_id,
                    path.display()
                )
            })?;
        let base_revision = projection
            .pointer("/event/proof/baseRevision")
            .and_then(serde_json::Value::as_u64);
        let new_revision = projection
            .pointer("/event/proof/newRevision")
            .and_then(serde_json::Value::as_u64);
        if let (Some(previous), Some(base)) = (previous_new_revision, base_revision) {
            if base != previous {
                return Err(format!(
                    "draft fact revision chain broken: event={} base={base} previous_new={previous} project={project_id} chapter={chapter_id}",
                    parsed.event_id
                ));
            }
        }
        if let (Some(base), Some(new)) = (base_revision, new_revision) {
            if new <= base {
                return Err(format!(
                    "draft fact revision does not advance: event={} base={base} new={new} project={project_id} chapter={chapter_id}",
                    parsed.event_id
                ));
            }
        }
        previous_new_revision = new_revision;
        batch.push(LoadedFact {
            event_id: parsed.event_id,
            revision: parsed.revision,
            projection_json: parsed.projection_json,
        });
    }
    Ok(batch)
}

/// journal 确认后删除已 ack 的前缀事实文件并推进基线；正文与剩余事实保留。
///

fn select_prefix(
    facts: &[DraftFactIndex],
    session_id: u64,
    draft_generation: &str,
    generation: &str,
    snapshot_revision: u64,
) -> Result<Vec<DraftFactIndex>, String> {
    let generation_matches = draft_generation.is_empty() || draft_generation == generation;
    let mut selected: Vec<DraftFactIndex> = Vec::new();
    let mut leading_foreign = false;
    for fact in facts {
        if fact.revision > snapshot_revision {
            break;
        }
        if fact.session_id == session_id && generation_matches {
            if leading_foreign {
                return Err(MIXED_SESSION_ERROR.to_string());
            }
            selected.push(fact.clone());
        } else if selected.is_empty() {
            // 先看到别的会话/generation 的旧事实：若后面还有本会话事实，
            // 这些旧事实的 revision 更小，绝不能跳过。
            leading_foreign = true;
        } else if fact.revision < selected[0].revision {
            return Err(MIXED_SESSION_ERROR.to_string());
        } else {
            // 本会话连续前缀到此结束，之后的事实留给后续快照。
            break;
        }
    }
    if leading_foreign && selected.is_empty() {
        return Err(MIXED_SESSION_ERROR.to_string());
    }
    Ok(selected)
}
