use crate::editor::transaction::composition::OffsetMap;
use crate::editor::transaction::engine::*;
use crate::editor::transaction::rebase::*;
use crate::editor::transaction::visual::*;
use crate::editor::transaction::{compute_rebase_slice_mappings, SliceMatchInput};

#[allow(deprecated)]
#[test]
fn transaction_rebase_serializes_camel_case() {
    let rebase = TransactionRebase {
        cancelled_transaction_id: 42,
        old_progress: 0.6,
        old_frame_snapshot: Some(RebaseFrameSnapshot {
            slice_rects: vec![Rect {
                x: 10.0,
                y: 20.0,
                w: 30.0,
                h: 40.0,
            }],
            slice_alphas: vec![0.8],
            cursor_rect: Some(CursorRect {
                x: 10.0,
                top: 5.0,
                bottom: 25.0,
                baseline_y: 20.0,
            }),
        }),
        slice_mappings: vec![RebaseSliceMapping {
            old_slice_index: 0,
            new_slice_index: 1,
            continuation: RebaseContinuation::Continue,
            reason: RebaseReason::SameByteRange,
        }],
    };
    let json = serde_json::to_string(&rebase).unwrap();
    assert!(json.contains("\"cancelledTransactionId\":"));
    assert!(json.contains("\"oldProgress\":"));
    assert!(json.contains("\"oldFrameSnapshot\":"));
    assert!(json.contains("\"sliceRects\":"));
    assert!(json.contains("\"sliceAlphas\":"));
    assert!(json.contains("\"cursorRect\":"));
    // #606: sliceMappings 序列化为 camelCase
    assert!(json.contains("\"sliceMappings\":"));
    assert!(json.contains("\"oldSliceIndex\":"));
    assert!(json.contains("\"newSliceIndex\":"));
    assert!(json.contains("\"continuation\":\"continue\""));
    assert!(json.contains("\"reason\":\"sameByteRange\""));
}

#[test]
fn transaction_rebase_skips_none() {
    let rebase = TransactionRebase {
        cancelled_transaction_id: 1,
        old_progress: 0.0,
        old_frame_snapshot: None,
        slice_mappings: Vec::new(),
    };
    let json = serde_json::to_string(&rebase).unwrap();
    assert!(!json.contains("\"oldFrameSnapshot\":"));
    // #606: 空 slice_mappings 不序列化
    assert!(!json.contains("\"sliceMappings\":"));
}

#[test]
fn compute_rebase_creates_transaction_rebase() {
    let rebase = compute_rebase(
        42,
        0.6,
        Some(RebaseFrameSnapshot {
            slice_rects: vec![Rect {
                x: 10.0,
                y: 20.0,
                w: 30.0,
                h: 40.0,
            }],
            slice_alphas: vec![0.8],
            cursor_rect: None,
        }),
        // #606: 旧事务 1 个 Insert slice @ [0,3)，新事务 1 个 Insert slice @ [0,3)
        SliceMatchInput {
            old_slice_roles: &[AnimatedSliceRole::Insert],
            old_slice_byte_ranges: &[(0, 3)],
            new_slice_roles: &[AnimatedSliceRole::Insert],
            new_slice_byte_ranges: &[(0, 3)],
            offset_map: None,
        },
    );
    assert_eq!(rebase.cancelled_transaction_id, 42);
    assert!((rebase.old_progress - 0.6).abs() < f64::EPSILON);
    assert!(rebase.old_frame_snapshot.is_some());
    // #606: slice_mappings 应包含 1 个 Continue 映射
    assert_eq!(rebase.slice_mappings.len(), 1);
    assert_eq!(rebase.slice_mappings[0].old_slice_index, 0);
    assert_eq!(rebase.slice_mappings[0].new_slice_index, 0);
    assert_eq!(
        rebase.slice_mappings[0].continuation,
        RebaseContinuation::Continue
    );
    assert_eq!(rebase.slice_mappings[0].reason, RebaseReason::SameByteRange);
}

#[test]
fn transactions_overlap_cursor_only_always_conflicts() {
    assert!(transactions_overlap(
        UnifiedTransactionKind::CursorOnly,
        (0, 0),
        UnifiedTransactionKind::BodyEdit,
        (5, 10),
    ));
}

#[test]
fn transactions_overlap_overlapping_ranges() {
    assert!(transactions_overlap(
        UnifiedTransactionKind::BodyEdit,
        (0, 10),
        UnifiedTransactionKind::BodyEdit,
        (5, 15),
    ));
}

#[test]
fn transactions_overlap_non_overlapping_ranges() {
    assert!(!transactions_overlap(
        UnifiedTransactionKind::BodyEdit,
        (0, 5),
        UnifiedTransactionKind::BodyEdit,
        (10, 15),
    ));
}

#[test]
fn rebase_covers_all_transaction_kinds() {
    // #516: rebase 必须覆盖四种事务
    // 测试 CursorOnly 与 BodyEdit 冲突
    assert!(transactions_overlap(
        UnifiedTransactionKind::CursorOnly,
        (0, 0),
        UnifiedTransactionKind::BodyEdit,
        (0, 5),
    ));
    // 测试 CompositionUpdate 与 CompositionCommitOrCancel 冲突
    assert!(transactions_overlap(
        UnifiedTransactionKind::CompositionUpdate,
        (0, 5),
        UnifiedTransactionKind::CompositionCommitOrCancel,
        (3, 8),
    ));
}

// #606: compute_rebase_slice_mappings 行为测试

#[test]
fn compute_rebase_slice_mappings_empty_inputs() {
    let mappings = compute_rebase_slice_mappings(SliceMatchInput {
        old_slice_roles: &[],
        old_slice_byte_ranges: &[],
        new_slice_roles: &[],
        new_slice_byte_ranges: &[],
        offset_map: None,
    });
    assert!(mappings.is_empty());
}

#[test]
fn compute_rebase_slice_mappings_same_byte_range_compatible_roles() {
    // 旧/新 slice 完全相同 + 角色兼容 → 1 个 Continue + SameByteRange
    let old_roles = [AnimatedSliceRole::Insert, AnimatedSliceRole::Delete];
    let old_ranges = [(0, 3), (5, 8)];
    let new_roles = [AnimatedSliceRole::Insert, AnimatedSliceRole::Delete];
    let new_ranges = [(0, 3), (5, 8)];
    let mappings = compute_rebase_slice_mappings(SliceMatchInput {
        old_slice_roles: &old_roles,
        old_slice_byte_ranges: &old_ranges,
        new_slice_roles: &new_roles,
        new_slice_byte_ranges: &new_ranges,
        offset_map: None,
    });
    assert_eq!(mappings.len(), 2);
    assert_eq!(mappings[0].old_slice_index, 0);
    assert_eq!(mappings[0].new_slice_index, 0);
    assert_eq!(mappings[0].continuation, RebaseContinuation::Continue);
    assert_eq!(mappings[0].reason, RebaseReason::SameByteRange);
    assert_eq!(mappings[1].old_slice_index, 1);
    assert_eq!(mappings[1].new_slice_index, 1);
}

#[test]
fn compute_rebase_slice_mappings_move_compatible_with_insert() {
    // Move 与 Insert 兼容（都是"新出现的文字"动画）
    let old_roles = [AnimatedSliceRole::Insert];
    let old_ranges = [(0, 3)];
    let new_roles = [AnimatedSliceRole::Move];
    let new_ranges = [(0, 3)];
    let mappings = compute_rebase_slice_mappings(SliceMatchInput {
        old_slice_roles: &old_roles,
        old_slice_byte_ranges: &old_ranges,
        new_slice_roles: &new_roles,
        new_slice_byte_ranges: &new_ranges,
        offset_map: None,
    });
    assert_eq!(mappings.len(), 1);
    assert_eq!(mappings[0].continuation, RebaseContinuation::Continue);
}

#[test]
fn compute_rebase_slice_mappings_crossfade_pair_compatible() {
    // CrossfadeOld 与 Delete 兼容；CrossfadeNew 与 Insert 兼容
    let old_roles = [
        AnimatedSliceRole::CrossfadeOld,
        AnimatedSliceRole::CrossfadeNew,
    ];
    let old_ranges = [(0, 3), (3, 6)];
    let new_roles = [AnimatedSliceRole::Delete, AnimatedSliceRole::Insert];
    let new_ranges = [(0, 3), (3, 6)];
    let mappings = compute_rebase_slice_mappings(SliceMatchInput {
        old_slice_roles: &old_roles,
        old_slice_byte_ranges: &old_ranges,
        new_slice_roles: &new_roles,
        new_slice_byte_ranges: &new_ranges,
        offset_map: None,
    });
    assert_eq!(mappings.len(), 2);
}

#[test]
fn compute_rebase_slice_mappings_incompatible_roles_no_mapping() {
    // Insert 与 Delete 不兼容（byte range 相同也不匹配）
    let old_roles = [AnimatedSliceRole::Insert];
    let old_ranges = [(0, 3)];
    let new_roles = [AnimatedSliceRole::Delete];
    let new_ranges = [(0, 3)];
    let mappings = compute_rebase_slice_mappings(SliceMatchInput {
        old_slice_roles: &old_roles,
        old_slice_byte_ranges: &old_ranges,
        new_slice_roles: &new_roles,
        new_slice_byte_ranges: &new_ranges,
        offset_map: None,
    });
    // 不兼容 → 无映射（平台端按 End 处理）
    assert!(mappings.is_empty());
}

#[test]
fn compute_rebase_slice_mappings_different_byte_range_no_mapping() {
    // 角色兼容但 byte range 不同 → 无映射
    let old_roles = [AnimatedSliceRole::Insert];
    let old_ranges = [(0, 3)];
    let new_roles = [AnimatedSliceRole::Insert];
    let new_ranges = [(5, 8)];
    let mappings = compute_rebase_slice_mappings(SliceMatchInput {
        old_slice_roles: &old_roles,
        old_slice_byte_ranges: &old_ranges,
        new_slice_roles: &new_roles,
        new_slice_byte_ranges: &new_ranges,
        offset_map: None,
    });
    assert!(mappings.is_empty());
}

#[test]
fn compute_rebase_slice_mappings_offset_map_matched() {
    // 正: 旧 slice range [5,8) 经 OffsetMap 映射（旧 [5,8) → 新 [7,10)）后与
    // 新 slice range [7,10) 相等 + 角色兼容 → OffsetMapMatched + Continue。
    let old_roles = [AnimatedSliceRole::Move];
    let old_ranges = [(5, 8)];
    let new_roles = [AnimatedSliceRole::Move];
    let new_ranges = [(7, 10)];
    let offset_map = OffsetMap {
        entries: vec![crate::editor::OffsetMapEntry {
            old_byte_offset: crate::editor::strong_types::Utf8ByteOffset::unchecked(5),
            new_byte_offset: crate::editor::strong_types::Utf8ByteOffset::unchecked(7),
            length: 3,
            kind: crate::editor::OffsetMapKind::Shifted,
        }],
    };
    let mappings = compute_rebase_slice_mappings(SliceMatchInput {
        old_slice_roles: &old_roles,
        old_slice_byte_ranges: &old_ranges,
        new_slice_roles: &new_roles,
        new_slice_byte_ranges: &new_ranges,
        offset_map: Some(&offset_map),
    });
    assert_eq!(mappings.len(), 1);
    assert_eq!(mappings[0].old_slice_index, 0);
    assert_eq!(mappings[0].new_slice_index, 0);
    assert_eq!(mappings[0].continuation, RebaseContinuation::Continue);
    assert_eq!(mappings[0].reason, RebaseReason::OffsetMapMatched);
}

#[test]
fn compute_rebase_slice_mappings_offset_map_unmapped_range_no_mapping() {
    // 反: 旧 slice range [5,8) 不在 OffsetMap 覆盖范围内 → 无映射。
    let old_roles = [AnimatedSliceRole::Move];
    let old_ranges = [(5, 8)];
    let new_roles = [AnimatedSliceRole::Move];
    let new_ranges = [(7, 10)];
    let offset_map = OffsetMap {
        entries: vec![crate::editor::OffsetMapEntry {
            old_byte_offset: crate::editor::strong_types::Utf8ByteOffset::unchecked(0),
            new_byte_offset: crate::editor::strong_types::Utf8ByteOffset::unchecked(0),
            length: 3,
            kind: crate::editor::OffsetMapKind::Identity,
        }],
    };
    let mappings = compute_rebase_slice_mappings(SliceMatchInput {
        old_slice_roles: &old_roles,
        old_slice_byte_ranges: &old_ranges,
        new_slice_roles: &new_roles,
        new_slice_byte_ranges: &new_ranges,
        offset_map: Some(&offset_map),
    });
    assert!(mappings.is_empty());
}

#[test]
fn compute_rebase_slice_mappings_offset_map_wrong_destination_no_mapping() {
    // 反: OffsetMap 映射后的 range 与新 slice range 不等 → 无映射。
    let old_roles = [AnimatedSliceRole::Move];
    let old_ranges = [(5, 8)];
    let new_roles = [AnimatedSliceRole::Move];
    let new_ranges = [(8, 11)];
    let offset_map = OffsetMap {
        entries: vec![crate::editor::OffsetMapEntry {
            old_byte_offset: crate::editor::strong_types::Utf8ByteOffset::unchecked(5),
            new_byte_offset: crate::editor::strong_types::Utf8ByteOffset::unchecked(7),
            length: 3,
            kind: crate::editor::OffsetMapKind::Shifted,
        }],
    };
    let mappings = compute_rebase_slice_mappings(SliceMatchInput {
        old_slice_roles: &old_roles,
        old_slice_byte_ranges: &old_ranges,
        new_slice_roles: &new_roles,
        new_slice_byte_ranges: &new_ranges,
        offset_map: Some(&offset_map),
    });
    assert!(mappings.is_empty());
}

#[test]
fn compute_rebase_slice_mappings_offset_map_incompatible_roles_no_mapping() {
    // 反: OffsetMap 映射后 range 相等但角色不兼容（Move 与 Delete 不能接续）→ 无映射。
    let old_roles = [AnimatedSliceRole::Delete];
    let old_ranges = [(5, 8)];
    let new_roles = [AnimatedSliceRole::Move];
    let new_ranges = [(7, 10)];
    let offset_map = OffsetMap {
        entries: vec![crate::editor::OffsetMapEntry {
            old_byte_offset: crate::editor::strong_types::Utf8ByteOffset::unchecked(5),
            new_byte_offset: crate::editor::strong_types::Utf8ByteOffset::unchecked(7),
            length: 3,
            kind: crate::editor::OffsetMapKind::Shifted,
        }],
    };
    let mappings = compute_rebase_slice_mappings(SliceMatchInput {
        old_slice_roles: &old_roles,
        old_slice_byte_ranges: &old_ranges,
        new_slice_roles: &new_roles,
        new_slice_byte_ranges: &new_ranges,
        offset_map: Some(&offset_map),
    });
    assert!(mappings.is_empty());
}

#[test]
fn compute_rebase_slice_mappings_offset_map_does_not_override_same_byte_range() {
    // 优先级: byte range 完全相同 → SameByteRange，不降级为 OffsetMapMatched。
    let old_roles = [AnimatedSliceRole::Move];
    let old_ranges = [(0, 3)];
    let new_roles = [AnimatedSliceRole::Move];
    let new_ranges = [(0, 3)];
    let offset_map = OffsetMap {
        entries: vec![crate::editor::OffsetMapEntry {
            old_byte_offset: crate::editor::strong_types::Utf8ByteOffset::unchecked(0),
            new_byte_offset: crate::editor::strong_types::Utf8ByteOffset::unchecked(7),
            length: 3,
            kind: crate::editor::OffsetMapKind::Shifted,
        }],
    };
    let mappings = compute_rebase_slice_mappings(SliceMatchInput {
        old_slice_roles: &old_roles,
        old_slice_byte_ranges: &old_ranges,
        new_slice_roles: &new_roles,
        new_slice_byte_ranges: &new_ranges,
        offset_map: Some(&offset_map),
    });
    assert_eq!(mappings.len(), 1);
    assert_eq!(mappings[0].reason, RebaseReason::SameByteRange);
}

#[test]
fn compute_rebase_slice_mappings_offset_map_range_spanning_entry_no_mapping() {
    // 反: range 跨越映射条目边界 → 不指向同一逻辑对象 → 无映射。
    let old_roles = [AnimatedSliceRole::Move];
    let old_ranges = [(2, 6)];
    let new_roles = [AnimatedSliceRole::Move];
    let new_ranges = [(4, 8)];
    let offset_map = OffsetMap {
        entries: vec![crate::editor::OffsetMapEntry {
            old_byte_offset: crate::editor::strong_types::Utf8ByteOffset::unchecked(0),
            new_byte_offset: crate::editor::strong_types::Utf8ByteOffset::unchecked(2),
            length: 3,
            kind: crate::editor::OffsetMapKind::Shifted,
        }],
    };
    let mappings = compute_rebase_slice_mappings(SliceMatchInput {
        old_slice_roles: &old_roles,
        old_slice_byte_ranges: &old_ranges,
        new_slice_roles: &new_roles,
        new_slice_byte_ranges: &new_ranges,
        offset_map: Some(&offset_map),
    });
    assert!(mappings.is_empty());
}

#[test]
fn compute_rebase_slice_mappings_each_new_slice_matched_at_most_once() {
    // 两个旧 slice 都想匹配同一新 slice — 只允许一个匹配
    let old_roles = [AnimatedSliceRole::Insert, AnimatedSliceRole::Insert];
    let old_ranges = [(0, 3), (0, 3)];
    let new_roles = [AnimatedSliceRole::Insert];
    let new_ranges = [(0, 3)];
    let mappings = compute_rebase_slice_mappings(SliceMatchInput {
        old_slice_roles: &old_roles,
        old_slice_byte_ranges: &old_ranges,
        new_slice_roles: &new_roles,
        new_slice_byte_ranges: &new_ranges,
        offset_map: None,
    });
    // 只有第一个旧 slice 匹配到新 slice 0；第二个旧 slice 无可用新 slice
    assert_eq!(mappings.len(), 1);
    assert_eq!(mappings[0].old_slice_index, 0);
    assert_eq!(mappings[0].new_slice_index, 0);
}

#[test]
fn compute_rebase_slice_mappings_partial_match() {
    // 旧事务 3 个 slice，新事务只有 1 个匹配
    let old_roles = [
        AnimatedSliceRole::Insert,
        AnimatedSliceRole::Delete,
        AnimatedSliceRole::Move,
    ];
    let old_ranges = [(0, 3), (5, 8), (10, 12)];
    let new_roles = [AnimatedSliceRole::Move, AnimatedSliceRole::Delete];
    let new_ranges = [(10, 12), (15, 18)];
    let mappings = compute_rebase_slice_mappings(SliceMatchInput {
        old_slice_roles: &old_roles,
        old_slice_byte_ranges: &old_ranges,
        new_slice_roles: &new_roles,
        new_slice_byte_ranges: &new_ranges,
        offset_map: None,
    });
    // 旧 slice 2 (Move @ [10,12)) 匹配新 slice 0 (Move @ [10,12))
    // 旧 slice 1 (Delete @ [5,8)) 不匹配新 slice 1 (Delete @ [15,18)) — byte range 不同
    // 旧 slice 0 (Insert @ [0,3)) 无匹配
    assert_eq!(mappings.len(), 1);
    assert_eq!(mappings[0].old_slice_index, 2);
    assert_eq!(mappings[0].new_slice_index, 0);
}

#[test]
fn compute_rebase_includes_slice_mappings() {
    // compute_rebase 应将 slice_mappings 包含在返回的 TransactionRebase 中
    let rebase = compute_rebase(
        100,
        0.5,
        None,
        SliceMatchInput {
            old_slice_roles: &[AnimatedSliceRole::Insert, AnimatedSliceRole::Delete],
            old_slice_byte_ranges: &[(0, 3), (5, 8)],
            new_slice_roles: &[AnimatedSliceRole::Insert, AnimatedSliceRole::Delete],
            new_slice_byte_ranges: &[(0, 3), (5, 8)],
            offset_map: None,
        },
    );
    assert_eq!(rebase.cancelled_transaction_id, 100);
    assert_eq!(rebase.slice_mappings.len(), 2);
}

#[test]
fn rebase_slice_mapping_serializes_camel_case() {
    let m = RebaseSliceMapping {
        old_slice_index: 2,
        new_slice_index: 5,
        continuation: RebaseContinuation::End,
        reason: RebaseReason::NoMapping,
    };
    let json = serde_json::to_string(&m).unwrap();
    assert!(json.contains("\"oldSliceIndex\":2"));
    assert!(json.contains("\"newSliceIndex\":5"));
    assert!(json.contains("\"continuation\":\"end\""));
    assert!(json.contains("\"reason\":\"noMapping\""));
}

// ── #639 评论 5420317382 修复后验证测试 ─────────────────────────────

/// #639 评论 5420317382 问题 1 修复后验证：旧 Move slice rebase 到新
/// CrossfadeOld+CrossfadeNew pair 时，Core 应把旧 Move 映射到 CrossfadeOld
/// （index 0）而非 CrossfadeNew（index 1）。
///
/// 修复前：`compatible_rebase_roles()` 把 Move/Insert/CrossfadeNew 全当成同一
/// 出现态，`try_match_slice` 跳过不兼容的 CrossfadeOld，命中 CrossfadeNew →
/// 旧 Move 映射到 CrossfadeNew (index 1)，`RebasePlanner` 把旧 Move 的
/// currentAlpha(==1) 填给新 CrossfadeNew 的 startAlpha → 新位置字直接全亮，
/// 同时配对 CrossfadeOld 在旧位置继续淡出 → 闪一下/跳一下。
///
/// 修复后：`compute_rebase_slice_mappings` 对同一 byte range 存在 Crossfade
/// pair 时，旧 emergence role（Move/Insert/CrossfadeNew）优先映射到 CrossfadeOld，
/// 配对 CrossfadeNew 不接旧状态（保持 startAlpha=0 在新 Layout 自己淡入）。
#[test]
fn repro_639_comment_5420317382_move_rebased_to_crossfade_new_not_crossfade_old() {
    // 旧事务：1 个 Move slice（同行位置插值，currentAlpha 通常 == 1）
    let old_roles = [AnimatedSliceRole::Move];
    let old_ranges = [(5, 8)];

    // 新事务：跨视觉行 pair — CrossfadeOld（旧位置淡出）+ CrossfadeNew（新位置淡入）
    // 两者 byte range 相同（同一逻辑字符，只是跨行/形状变化）
    let new_roles = [
        AnimatedSliceRole::CrossfadeOld,
        AnimatedSliceRole::CrossfadeNew,
    ];
    let new_ranges = [(5, 8), (5, 8)];

    let mappings = compute_rebase_slice_mappings(SliceMatchInput {
        old_slice_roles: &old_roles,
        old_slice_byte_ranges: &old_ranges,
        new_slice_roles: &new_roles,
        new_slice_byte_ranges: &new_ranges,
        offset_map: None,
    });

    // 修复后断言：旧 Move 映射到新 slice index 0（CrossfadeOld），
    // 而非 index 1（CrossfadeNew）。旧出现态从当前位置退场，新位置自己淡入。
    assert_eq!(mappings.len(), 1, "旧 Move 应恰好匹配到一个新 slice");
    assert_eq!(
        mappings[0].old_slice_index, 0,
        "旧 slice index 应为 0（唯一的旧 Move）"
    );
    // 修复后：new_slice_index == 0（CrossfadeOld），而非 1（CrossfadeNew）
    assert_eq!(
        mappings[0].new_slice_index, 0,
        "修复后 #639 评论 5420317382 问题 1：旧 Move 应映射到 CrossfadeOld (index 0) \
         而非 CrossfadeNew (index 1)，从当前位置退场，新位置自己淡入"
    );
    assert_eq!(
        mappings[0].reason,
        RebaseReason::SameByteRange,
        "byte range 相同 → SameByteRange"
    );
    // 新 slice 1（CrossfadeNew）未被任何旧 slice 匹配 → 保持新规划 startAlpha=0，
    // 在新 Layout 自己淡入，不会突然全亮。
    let matched_new_indices: std::collections::HashSet<usize> =
        mappings.iter().map(|m| m.new_slice_index).collect();
    assert!(
        matched_new_indices.contains(&0),
        "CrossfadeOld (index 0) 被旧 Move 匹配，从当前位置退场"
    );
    assert!(
        !matched_new_indices.contains(&1),
        "CrossfadeNew (index 1) 未被匹配，保持 startAlpha=0 在新 Layout 自己淡入"
    );
}

/// #639 评论 5420317382 问题 1 修复后验证：Crossfade pair 优先映射 + 没有 pair
/// 时现有 continuation 规则保留。
///
/// 修复后行为：
/// 1. 旧 Move/Insert/CrossfadeNew → 新 CrossfadeOld+CrossfadeNew pair：优先映射到
///    CrossfadeOld，CrossfadeNew 不接旧状态。
/// 2. 没有 pair 时，旧 Move → 新单独 CrossfadeNew 仍兼容（现有 continuation 保留）。
/// 3. 旧 CrossfadeNew → 新单独 Move 仍兼容（现有 continuation 保留）。
#[test]
fn repro_639_comment_5420317382_move_and_crossfade_new_treated_as_same_emergence_class() {
    // 修复后 1：旧 Move → 新 CrossfadeOld+CrossfadeNew pair → 优先映射到 CrossfadeOld
    let mappings_move_to_pair = compute_rebase_slice_mappings(SliceMatchInput {
        old_slice_roles: &[AnimatedSliceRole::Move],
        old_slice_byte_ranges: &[(0, 3)],
        new_slice_roles: &[
            AnimatedSliceRole::CrossfadeOld,
            AnimatedSliceRole::CrossfadeNew,
        ],
        new_slice_byte_ranges: &[(0, 3), (0, 3)],
        offset_map: None,
    });
    assert_eq!(
        mappings_move_to_pair.len(),
        1,
        "旧 Move 应映射到 pair 中的一个 slice"
    );
    assert_eq!(
        mappings_move_to_pair[0].new_slice_index, 0,
        "修复后：旧 Move 优先映射到 CrossfadeOld (index 0)，而非 CrossfadeNew (index 1)"
    );

    // 修复后 2：旧 Insert → 新 CrossfadeOld+CrossfadeNew pair → 优先映射到 CrossfadeOld
    let mappings_insert_to_pair = compute_rebase_slice_mappings(SliceMatchInput {
        old_slice_roles: &[AnimatedSliceRole::Insert],
        old_slice_byte_ranges: &[(0, 3)],
        new_slice_roles: &[
            AnimatedSliceRole::CrossfadeOld,
            AnimatedSliceRole::CrossfadeNew,
        ],
        new_slice_byte_ranges: &[(0, 3), (0, 3)],
        offset_map: None,
    });
    assert_eq!(
        mappings_insert_to_pair.len(),
        1,
        "旧 Insert 应映射到 pair 中的一个 slice"
    );
    assert_eq!(
        mappings_insert_to_pair[0].new_slice_index, 0,
        "修复后：旧 Insert 优先映射到 CrossfadeOld (index 0)"
    );

    // 修复后 3：没有 pair 时，旧 Move → 新单独 CrossfadeNew 仍兼容（现有规则保留）
    let mappings_move_to_new = compute_rebase_slice_mappings(SliceMatchInput {
        old_slice_roles: &[AnimatedSliceRole::Move],
        old_slice_byte_ranges: &[(0, 3)],
        new_slice_roles: &[AnimatedSliceRole::CrossfadeNew],
        new_slice_byte_ranges: &[(0, 3)],
        offset_map: None,
    });
    assert_eq!(
        mappings_move_to_new.len(),
        1,
        "没有 pair 时：compatible_rebase_roles(CrossfadeNew, Move) == true，旧 Move 可映射到新 CrossfadeNew（现有 continuation 保留）"
    );

    // 修复后 4：没有 pair 时，旧 CrossfadeNew → 新单独 Move 仍兼容（现有规则保留）
    let mappings_new_to_move = compute_rebase_slice_mappings(SliceMatchInput {
        old_slice_roles: &[AnimatedSliceRole::CrossfadeNew],
        old_slice_byte_ranges: &[(0, 3)],
        new_slice_roles: &[AnimatedSliceRole::Move],
        new_slice_byte_ranges: &[(0, 3)],
        offset_map: None,
    });
    assert_eq!(
        mappings_new_to_move.len(),
        1,
        "没有 pair 时：compatible_rebase_roles(Move, CrossfadeNew) == true，旧 CrossfadeNew 可映射到新 Move（现有 continuation 保留）"
    );

    // 对照组：旧 Move → 新单独 CrossfadeOld（没有配对 CrossfadeNew）→ 不兼容
    let mappings_move_to_old = compute_rebase_slice_mappings(SliceMatchInput {
        old_slice_roles: &[AnimatedSliceRole::Move],
        old_slice_byte_ranges: &[(0, 3)],
        new_slice_roles: &[AnimatedSliceRole::CrossfadeOld],
        new_slice_byte_ranges: &[(0, 3)],
        offset_map: None,
    });
    assert!(
        mappings_move_to_old.is_empty(),
        "对照：compatible_rebase_roles(CrossfadeOld, Move) == false，旧 Move 不可映射到单独的 CrossfadeOld"
    );
}

/// #639 评论 5421085782 问题1 复现：`build_crossfade_pair_index` 只在
/// `CrossfadeOld.range == CrossfadeNew.range` 时才认成一对。但 CrossfadeOld.range
/// 是 old 文档坐标、CrossfadeNew.range 是 new 文档坐标，被 OffsetMap 平移后不同
/// （例如插字/回车后保留字符 old [30,33) → new [33,36)）。pair 建不出来 →
/// `compute_rebase_slice_mappings` 掉回 `try_match_slice`，旧 Move 经
/// OffsetMap 映射后接到 **CrossfadeNew**（新位置），而非期望的 CrossfadeOld
/// （从旧位置退场）。这会让旧 Move 的 currentAlpha 被填给新 CrossfadeNew 的
/// startAlpha，新位置字突然全亮，上一轮修的"新位置突然全亮"漏洞仍存在。
///
/// 此测试断言**期望行为**（旧 Move → CrossfadeOld），当前实现因 pair 建不出来
/// 会把旧 Move 映射到 CrossfadeNew，测试失败，暴露漏洞。
#[test]
fn compute_rebase_slice_mappings_crossfade_pair_offset_mapped_old_to_new() {
    // 场景：在 byte offset 30 处插字，后面保留的 3 字符 cluster 从 old [30,33)
    // 平移到 new [33,36)。新事务为这一对生成 CrossfadeOld(range=[30,33)，old 坐标)
    // + CrossfadeNew(range=[33,36)，new 坐标)。旧事务的 slice 是 Move(range=[30,33)，
    // old 坐标，emergence role)。
    let old_roles = [AnimatedSliceRole::Move];
    let old_ranges = [(30, 33)];
    let new_roles = [
        AnimatedSliceRole::CrossfadeOld,
        AnimatedSliceRole::CrossfadeNew,
    ];
    // CrossfadeOld.range 是 old 坐标 [30,33)；CrossfadeNew.range 是 new 坐标 [33,36)。
    // 两者不同 → build_crossfade_pair_index 配对失败。
    let new_ranges = [(30, 33), (33, 36)];
    // OffsetMap 把 old [30,33) 映射到 new [33,36)（保留字符整体平移 3 字节）。
    let offset_map = OffsetMap {
        entries: vec![crate::editor::OffsetMapEntry {
            old_byte_offset: crate::editor::strong_types::Utf8ByteOffset::unchecked(30),
            new_byte_offset: crate::editor::strong_types::Utf8ByteOffset::unchecked(33),
            length: 3,
            kind: crate::editor::OffsetMapKind::Shifted,
        }],
    };
    let mappings = compute_rebase_slice_mappings(SliceMatchInput {
        old_slice_roles: &old_roles,
        old_slice_byte_ranges: &old_ranges,
        new_slice_roles: &new_roles,
        new_slice_byte_ranges: &new_ranges,
        offset_map: Some(&offset_map),
    });

    // 期望：旧 Move 映射到 CrossfadeOld（new_slice_index == 0），从旧位置退场。
    // 当前实现：pair 建不出来 → 掉回 try_match_slice → 旧 Move 经 OffsetMap 映射
    // 后接到 CrossfadeNew（new_slice_index == 1），新位置突然全亮。
    assert_eq!(
        mappings.len(),
        1,
        "旧 Move 应映射到 Crossfade pair 中的一个 slice"
    );
    assert_eq!(
        mappings[0].old_slice_index, 0,
        "旧 slice index 应为 0（唯一一个旧 Move）"
    );
    assert_eq!(
        mappings[0].new_slice_index, 0,
        "旧 Move 应映射到 CrossfadeOld（new_slice_index == 0，从旧位置退场）。\
         当前实现因 build_crossfade_pair_index 要求 CrossfadeOld.range == CrossfadeNew.range\
         而建不出 pair，掉回 try_match_slice 后旧 Move 经 OffsetMap 映射接到 CrossfadeNew\
         （new_slice_index == 1），新位置突然全亮 — 这正是 #639 评论 5421085782 问题1。"
    );
}
