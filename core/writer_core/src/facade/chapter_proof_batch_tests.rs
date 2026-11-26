//! `chapter_proof_batch` 的显式证明批次回归测试（Issue #1062 1.1/1.2）。
//!
//! 独立成文件以遵守“生产源码不内嵌大段测试模块”的结构约束。

use super::*;
use crate::chapter::ChapterProofProjection;
use crate::editor::{EditorChange, EditorSelection, EditorTransactionCause, Utf8ByteOffset};
use crate::writing_stats::proof::{content_digest_of_text, ProofEditPayload, PROOF_FORMAT_VERSION};
use crate::writing_stats::{EventSource, Platform, WritingInputEvent};

/// 构造一条带逐字证明的编辑事件；`before`/`after` 是事务前后真实正文。
#[allow(clippy::too_many_arguments)]
fn proof_event(
    event_id: &str,
    before: &str,
    after: &str,
    changes: Vec<EditorChange>,
    base_revision: u64,
    new_revision: u64,
    seq: u64,
    session_id: &str,
) -> WritingInputEvent {
    let inserted_chars: u32 = changes
        .iter()
        .filter_map(|change| match change {
            EditorChange::Insert { text, .. } => Some(text.chars().count() as u32),
            EditorChange::Delete { .. } => None,
        })
        .sum();
    let deleted_chars: u32 = changes
        .iter()
        .filter_map(|change| match change {
            EditorChange::Delete { text, .. } => Some(text.chars().count() as u32),
            EditorChange::Insert { .. } => None,
        })
        .sum();
    let mut event = WritingInputEvent::with_identity(
        event_id.to_string(),
        1_000 + i64::try_from(seq).unwrap_or(0) * 1_000,
        "dev-1",
        Platform::Desktop,
        "desktop",
        "p1",
        "v1",
        "c1",
        EventSource::HumanTyped,
        inserted_chars,
        deleted_chars,
        0,
        0,
        0,
        session_id,
    );
    event.proof = Some(ProofEditPayload {
        format_version: PROOF_FORMAT_VERSION,
        seq,
        session_id: session_id.to_string(),
        baseline_id: String::new(),
        starts_new_chain: seq == 1,
        base_revision,
        new_revision,
        cause: EditorTransactionCause::Typing,
        changes,
        old_selection: EditorSelection::collapsed(before, 0),
        new_selection: EditorSelection::collapsed(after, after.len()),
        before_content_hash: String::new(),
        after_content_hash: String::new(),
        before_text_sha256: content_digest_of_text(before),
        after_text_sha256: content_digest_of_text(after),
        prev_event_hash: String::new(),
        event_hash: String::new(),
        sync_source_device_id: String::new(),
        parent_event_id: String::new(),
    });
    event
}

fn projection(event: WritingInputEvent, baseline_text: Option<&str>) -> ChapterProofProjection {
    ChapterProofProjection {
        event,
        baseline_text: baseline_text.map(str::to_string),
    }
}

fn insert(index: usize, text: &str) -> EditorChange {
    EditorChange::Insert {
        index: Utf8ByteOffset::unchecked(index),
        text: text.to_string(),
    }
}

fn delete(index: usize, text: &str) -> EditorChange {
    EditorChange::Delete {
        index: Utf8ByteOffset::unchecked(index),
        text: text.to_string(),
    }
}

/// Issue #1062 1.2：显式证明批次按 revision/摘要链逐条校验，
/// 两条连续事件应通过；正文与终点摘要不一致必须返回 PROOF_BATCH_MISMATCH。
#[test]
fn validate_explicit_batch_accepts_contiguous_chain_and_rejects_wrong_content() {
    let baseline = "abc";
    // abc --插入 d--> abcd --删除 b--> acd
    let events = vec![
        projection(
            proof_event(
                "evt-1",
                baseline,
                "abcd",
                vec![insert(3, "d")],
                0,
                1,
                1,
                "session-1",
            ),
            Some(baseline),
        ),
        projection(
            proof_event(
                "evt-2",
                "abcd",
                "acd",
                vec![delete(1, "b")],
                1,
                2,
                2,
                "session-1",
            ),
            None,
        ),
    ];

    validate_explicit_editor_proof_batch("c1", &events, "acd", 2, Some(baseline))
        .expect("continuous proof batch ending at saved content must validate");

    let error = validate_explicit_editor_proof_batch("c1", &events, "abd", 2, Some(baseline))
        .expect_err("saved content that differs from replayed text must be rejected");
    assert_eq!(
        error.code(),
        "PROOF_BATCH_MISMATCH",
        "terminal content mismatch must surface as a proof batch mismatch: {error:?}"
    );
}

/// Issue #1062 1.1：从基线 replay 得到的正文就是草稿正文；
/// 事件链断开（before 摘要不接上一条 after）时必须拒绝。
#[test]
fn replay_batch_from_baseline_detects_broken_chain() {
    let baseline = "abc";
    let events = vec![
        projection(
            proof_event(
                "evt-1",
                baseline,
                "abcd",
                vec![insert(3, "d")],
                0,
                1,
                1,
                "session-1",
            ),
            Some(baseline),
        ),
        projection(
            proof_event(
                "evt-2",
                "abcd",
                "acd",
                vec![delete(1, "b")],
                1,
                2,
                2,
                "session-1",
            ),
            None,
        ),
    ];

    let replayed = replay_batch_from_baseline(&events, baseline).unwrap();
    assert_eq!(replayed, "acd");
    replay_editor_proof_events(&events, baseline, &replayed)
        .expect("replayed final text must validate against its own batch");

    // 把第 2 条事件的 before 摘要换成另一段正文，链在中间断开。
    let mut broken = events.clone();
    broken[1]
        .event
        .proof
        .as_mut()
        .expect("proof payload")
        .before_text_sha256 = content_digest_of_text("axc");
    let error = validate_explicit_editor_proof_batch("c1", &broken, "acd", 2, Some(baseline))
        .expect_err("broken before/after digest chain must be rejected");
    assert_eq!(error.code(), "PROOF_BATCH_MISMATCH");
    assert!(replay_batch_from_baseline(&broken, baseline).is_err());
}
