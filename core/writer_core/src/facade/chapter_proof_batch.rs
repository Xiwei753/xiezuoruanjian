//! 编辑器正文保存的证明批次选择。
//!
//! 从 pending 队列中选出与本次保存快照严格对应的连续事件前缀；显式批次
//! 只认平台给出的 event ID，并尽量从 base 正文逐步 replay 校验。

/// 只选择 pending 队列队首开始、以本次保存正文为终点的连续证明前缀。
///
/// 旧客户端没有正文摘要时无法逐步验证，保留原行为（整批提交）；新事件
/// 必须从队首连续衔接，禁止跳过队首去匹配更晚的摘要，否则会把快照之前
/// 尚未 durable 的编辑漏掉，或把快照之后的新编辑混进同一 journal。
pub(crate) fn select_pending_editor_proof(
    events: Vec<crate::chapter::ChapterProofProjection>,
    content: &str,
    snapshot_revision: Option<u64>,
    event_ids: &[String],
    provided_baseline: Option<&str>,
) -> Vec<crate::chapter::ChapterProofProjection> {
    use crate::writing_stats::proof::{content_digest_of_text, text_sha256};

    if events.is_empty() {
        return events;
    }
    let events = if event_ids.is_empty() {
        events
    } else {
        // 平台显式给出本批 event ID 时，只取这些事件，不再按摘要猜批次。
        // ID 必须全部存在且顺序与请求一致；少一条就整批拒绝，避免正文
        // durable 后留下无法归属的证明事件。
        let mut selected = Vec::with_capacity(event_ids.len());
        let mut seen = std::collections::HashSet::new();
        for event_id in event_ids {
            if event_id.is_empty() || !seen.insert(event_id.as_str()) {
                return Vec::new();
            }
            let Some(position) = events
                .iter()
                .position(|projection| projection.event_id() == event_id)
            else {
                return Vec::new();
            };
            selected.push(events[position].clone());
        }
        selected
    };
    let all_legacy = events.iter().all(|projection| {
        projection
            .event
            .proof
            .as_ref()
            .is_some_and(|proof| proof.after_text_sha256.is_empty())
    });
    if all_legacy {
        return events;
    }
    let digest = content_digest_of_text(content);
    let legacy_digest = text_sha256(content);
    // 只从 pending 队列的队首开始取连续前缀，不再从末尾找摘要相同的事件
    // 再向前拼。队首之前的编辑如果已经被更晚的保存覆盖，它们必须已经在
    // 那次保存中 durable；若仍在队列里，说明本批快照不包含它们，不能跳过。
    let mut end = None;
    let mut last_included = None;
    for (index, projection) in events.iter().enumerate() {
        let Some(proof) = projection.event.proof.as_ref() else {
            return Vec::new();
        };
        if proof.after_text_sha256.is_empty() {
            return Vec::new();
        }
        if let Some(limit) = snapshot_revision {
            if proof.new_revision > limit {
                break;
            }
        }
        last_included = Some(index);
        if index > 0 {
            let Some(previous) = events[index - 1].event.proof.as_ref() else {
                return Vec::new();
            };
            if previous.after_text_sha256 != proof.before_text_sha256 {
                return Vec::new();
            }
            if !previous.session_id.is_empty()
                && !proof.session_id.is_empty()
                && previous.session_id != proof.session_id
            {
                return Vec::new();
            }
            if previous.seq > 0 && proof.seq > 0 && proof.seq != previous.seq.saturating_add(1) {
                return Vec::new();
            }
        }
        if proof.after_text_sha256 == digest || proof.after_text_sha256 == legacy_digest {
            // A→B→A→B：同一最终摘要会再次出现，必须取最靠后的连续前缀，
            // 否则会把快照内的后半段编辑留成 pending。
            end = Some(index);
        }
    }
    if snapshot_revision.is_some() && last_included != end {
        // 快照 revision 内的最后一条事件必须正好等于保存正文；否则说明
        // 快照与事件批次不一致，不能提交正文。
        return Vec::new();
    }
    match end {
        Some(end) => {
            let selected = events[..=end].to_vec();
            let first = selected.first();
            let baseline = first
                .and_then(|projection| projection.baseline_text.clone())
                .or_else(|| {
                    let before_hash = first
                        .and_then(|projection| projection.event.proof.as_ref())
                        .map(|proof| proof.before_text_sha256.as_str())?;
                    provided_baseline
                        .filter(|baseline| {
                            before_hash.is_empty() || proof_digest_matches(before_hash, baseline)
                        })
                        .map(str::to_string)
                });
            if let Some(baseline) = baseline {
                // 能拿到本批起点正文时，按事件顺序逐步 replay，并核对每条
                // before/after 摘要。只比最终摘要无法发现批次中间被替换。
                if replay_editor_proof_events(&selected, &baseline, content).is_err() {
                    return Vec::new();
                }
            }
            selected
        }
        None => Vec::new(),
    }
}

/// 事件摘要是否等于给定正文的摘要。
///
/// 新事件保存增量内容根摘要（`content-root-v1:`），旧事件保存完整
/// SHA-256（`sha256:v1:`）；两种格式都必须能校验，不能只认其中一种。
pub(crate) fn proof_digest_matches(value: &str, text: &str) -> bool {
    use crate::writing_stats::proof::{content_digest_of_text, CONTENT_DIGEST_VERSION};
    if value.starts_with(CONTENT_DIGEST_VERSION) {
        value == content_digest_of_text(text)
    } else {
        value == crate::writing_stats::proof::text_sha256(text)
    }
}

/// 按事件顺序 replay 一批证明事实，校验摘要链与终点正文。
///
/// `baseline` 是批次起点正文；每条事件的 `changes` 依次应用，逐条核对
/// before/after 摘要，最后要求 replay 结果等于保存正文。任何不连续都返回
/// `Err`，调用方必须拒绝本次保存。
pub(crate) fn replay_editor_proof_events(
    events: &[crate::chapter::ChapterProofProjection],
    baseline: &str,
    content: &str,
) -> crate::Result<()> {
    let replayed = replay_batch_from_baseline(events, baseline)?;
    if replayed != content {
        return Err(crate::Error::Other(
            "replayed proof batch does not end at saved content".to_string(),
        ));
    }
    Ok(())
}

/// 从基线正文逐条 replay 原始证明批次，返回最终正文。
///
/// 逐条核对 before/after 摘要（同时兼容增量内容摘要与旧 SHA-256），
/// 任一条不一致即返回 `Err`。终点正文由调用方自行核对。
pub(crate) fn replay_batch_from_baseline(
    events: &[crate::chapter::ChapterProofProjection],
    baseline: &str,
) -> crate::Result<String> {
    use crate::writing_stats::proof::apply_changes;

    let mut replayed = baseline.to_string();
    for projection in events {
        let event_id = projection.event_id();
        let Some(proof) = projection.event.proof.as_ref() else {
            return Err(crate::Error::Other(format!(
                "proof event {event_id} has no replayable payload"
            )));
        };
        if proof.changes.is_empty() {
            return Err(crate::Error::Other(format!(
                "proof event {event_id} has empty changes"
            )));
        }
        if !proof.before_text_sha256.is_empty()
            && !proof_digest_matches(&proof.before_text_sha256, &replayed)
        {
            return Err(crate::Error::Other(format!(
                "proof event {event_id} before digest does not match replayed text"
            )));
        }
        apply_changes(&mut replayed, &proof.changes)?;
        if !proof.after_text_sha256.is_empty()
            && !proof_digest_matches(&proof.after_text_sha256, &replayed)
        {
            return Err(crate::Error::Other(format!(
                "proof event {event_id} after digest does not match replayed text"
            )));
        }
    }
    Ok(replayed)
}

/// 校验平台显式传入的原始证明批次（Issue #1062 1.2）。
///
/// 与旧的“按正文摘要从全局 pending 队列猜批次”不同，正式路径只认调用方
/// 随保存一起冻结的原始事实：这里逐条 replay 校验 revision 顺序、session、
/// 摘要链与终点正文。任何不一致返回 [`crate::Error::ProofBatchMismatch`]，
/// 保存不发布正文。
pub(crate) fn validate_explicit_editor_proof_batch(
    chapter_id: &str,
    events: &[crate::chapter::ChapterProofProjection],
    content: &str,
    snapshot_revision: u64,
    base_content: Option<&str>,
) -> crate::Result<()> {
    use crate::writing_stats::proof::content_digest_of_text;

    if events.is_empty() {
        return Ok(());
    }
    let mismatch = |detail: String| crate::error::Error::ProofBatchMismatch {
        chapter_id: chapter_id.to_string(),
        detail,
    };
    let mut previous_revision: Option<u64> = None;
    let mut previous_after: Option<&str> = None;
    let mut session: Option<&str> = None;
    for projection in events {
        let event_id = projection.event_id();
        let Some(proof) = projection.event.proof.as_ref() else {
            return Err(mismatch(format!("event {event_id} has no proof payload")));
        };
        if proof.before_text_sha256.is_empty() || proof.after_text_sha256.is_empty() {
            return Err(mismatch(format!(
                "event {event_id} missing before/after digest"
            )));
        }
        if proof.changes.is_empty() {
            return Err(mismatch(format!("event {event_id} has empty changes")));
        }
        if snapshot_revision > 0 && proof.new_revision > snapshot_revision {
            return Err(mismatch(format!(
                "event {event_id} revision {} is after snapshot revision {snapshot_revision}",
                proof.new_revision
            )));
        }
        if proof.new_revision <= proof.base_revision {
            return Err(mismatch(format!(
                "event {event_id} revision {} does not advance base {}",
                proof.new_revision, proof.base_revision
            )));
        }
        if let Some(previous) = previous_revision {
            if proof.base_revision != previous {
                return Err(mismatch(format!(
                    "event {event_id} base revision {} does not continue previous {}",
                    proof.base_revision, previous
                )));
            }
        }
        if !proof.session_id.is_empty() {
            match session {
                Some(expected) if expected != proof.session_id => {
                    return Err(mismatch(format!(
                        "event {event_id} session {} differs from batch session {expected}",
                        proof.session_id
                    )));
                }
                None => session = Some(proof.session_id.as_str()),
                _ => {}
            }
        }
        match previous_after {
            Some(after) if after != proof.before_text_sha256 => {
                return Err(mismatch(format!(
                    "event {event_id} before digest does not continue previous event"
                )));
            }
            Some(_) => {}
            None => {
                if let Some(base) = base_content {
                    if !proof_digest_matches(&proof.before_text_sha256, base) {
                        return Err(mismatch(format!(
                            "event {event_id} before digest does not match committed base content"
                        )));
                    }
                }
            }
        }
        previous_after = Some(proof.after_text_sha256.as_str());
        previous_revision = Some(proof.new_revision);
    }
    let terminal = previous_after.unwrap_or_default();
    if terminal != content_digest_of_text(content) && !proof_digest_matches(terminal, content) {
        return Err(mismatch(
            "proof batch terminal digest does not match saved content".to_string(),
        ));
    }
    if let Some(baseline) = events
        .first()
        .and_then(|projection| projection.baseline_text.as_deref())
    {
        replay_editor_proof_events(events, baseline, content)
            .map_err(|error| mismatch(format!("replay failed: {error}")))?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "chapter_proof_batch_tests.rs"]
mod tests;
