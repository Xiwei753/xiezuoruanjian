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
                            before_hash.is_empty() || before_hash == text_sha256(baseline)
                        })
                        .map(str::to_string)
                });
            if let Some(baseline) = baseline {
                // 能拿到本批起点正文时，按事件顺序逐步 replay，并核对每条
                // before/after 摘要。只比最终摘要无法发现批次中间被替换。
                let mut replayed = baseline;
                for projection in &selected {
                    let Some(proof) = projection.event.proof.as_ref() else {
                        return Vec::new();
                    };
                    if !proof.before_text_sha256.is_empty()
                        && proof.before_text_sha256 != text_sha256(&replayed)
                    {
                        return Vec::new();
                    }
                    if !proof.changes.is_empty()
                        && crate::writing_stats::proof::apply_changes(&mut replayed, &proof.changes)
                            .is_err()
                    {
                        return Vec::new();
                    }
                    if proof.after_text_sha256 != text_sha256(&replayed) {
                        return Vec::new();
                    }
                }
                if replayed != content {
                    return Vec::new();
                }
            }
            selected
        }
        None => Vec::new(),
    }
}
