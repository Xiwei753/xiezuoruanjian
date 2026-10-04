//! Issue #826: 遮罩前沿单元测试。
//!
//! 覆盖：前沿采样 / extend 不产生第二个对象 / 跨 revision 范围累计 /
//! 吐字裁剪矩形 / 吞字 overlay 首帧可见。

use std::time::{Duration, Instant};

use writer_core::editor::OffsetMap;

use crate::editor::layout::{CaretAffinity, LayoutSnapshot};
use crate::sujian_editor_item::animation::edit_frontier::{
    EditFrontierKind, EditFrontierState, FrontierRect,
};
use crate::sujian_editor_item::edit_motion::CursorRect;
use crate::sujian_editor_item::layout_snapshot::EditorLayoutSnapshot;

fn rect(x: f64, top: f64, bottom: f64) -> CursorRect {
    CursorRect {
        x,
        top,
        bottom,
        baseline_y: bottom,
    }
}

fn instant_at(base: Instant, ms: u64) -> Instant {
    base + Duration::from_millis(ms)
}

fn empty_snapshot() -> EditorLayoutSnapshot {
    EditorLayoutSnapshot::new(
        LayoutSnapshot::empty_for_tests(),
        Vec::new(),
        None,
        None,
        CaretAffinity::Downstream,
    )
}

#[test]
fn frontier_sample_progresses_from_zero_to_one() {
    let now = Instant::now();
    let state = EditFrontierState::begin_insert(
        empty_snapshot(),
        String::from("a"),
        (0, 1),
        rect(0.0, 0.0, 20.0),
        rect(100.0, 0.0, 20.0),
        now,
        100,
    );
    assert_eq!(state.sample(now).progress, 0.0);
    assert_eq!(state.sample(instant_at(now, 100)).progress, 1.0);
    let mid = state.sample(instant_at(now, 50)).progress;
    assert!(mid > 0.0 && mid < 1.0, "中点进度必须在 (0,1)，实际 {mid}");
}

#[test]
fn frontier_is_finished_only_after_full_duration() {
    let now = Instant::now();
    let state = EditFrontierState::begin_insert(
        empty_snapshot(),
        String::from("a"),
        (0, 1),
        rect(0.0, 0.0, 20.0),
        rect(100.0, 0.0, 20.0),
        now,
        160,
    );
    assert!(!state.is_finished(instant_at(now, 80)));
    assert!(state.is_finished(instant_at(now, 160)));
    assert!(state.is_finished(instant_at(now, 500)));
}

/// Issue #826 评论 3 问题 4：吞字第一帧旧字必须完整可见。
///
/// `needs_old_overlay` 之前写的是 `progress > 0.0`，导致 progress=0 时不画旧字，
/// 下一帧 progress>0 又画出来 —— 存在首帧闪烁窗口。正确语义是
/// `progress < 1.0`：从第一帧到结束前一帧都画，完全收掉后不再画。
#[test]
fn delete_overlay_is_visible_on_the_first_frame() {
    let now = Instant::now();
    let state = EditFrontierState::begin_delete(
        empty_snapshot(),
        String::from("ABCDEF"),
        empty_snapshot(),
        String::from("ABCEF"),
        (3, 4),
        rect(30.0, 0.0, 20.0),
        rect(30.0, 0.0, 20.0),
        now,
        160,
    );
    assert!(
        state.sample(now).needs_old_overlay(),
        "吞字第一帧旧字必须完整可见，不能是空白"
    );
    assert!(
        state.sample(instant_at(now, 80)).needs_old_overlay(),
        "吞字中途旧字仍可见"
    );
    assert!(
        !state.sample(instant_at(now, 160)).needs_old_overlay(),
        "吞字结束旧字必须完全消失"
    );
}

/// Issue #826 评论 3 问题 1：连续吐字必须把先前还没吐完的字一起留在遮罩范围里。
///
/// 之前 `extend_insert` 直接 `self.new_range = Some(new_range)`，
/// 第二笔一到，第一笔尚未露出的范围就从遮罩里消失，canonical 会把它瞬间补全。
/// 正确语义：把已累计范围用 OffsetMap 映到最新坐标再 union。
#[test]
fn extend_insert_accumulates_new_range_across_revisions() {
    let now = Instant::now();
    let mut state = EditFrontierState::begin_insert(
        empty_snapshot(),
        String::from("a"),
        (1, 2),
        rect(10.0, 0.0, 20.0),
        rect(20.0, 0.0, 20.0),
        now,
        160,
    );
    assert_eq!(state.new_range, Some((1, 2)));

    // 第一笔：先在半程采样，再扩展。正文 "a" -> "ab"（在位置 1 插入 b）。
    let half = instant_at(now, 80);
    let prev_target_to_new = OffsetMap::build("a", "ab");
    state.extend_insert(
        empty_snapshot(),
        String::from("ab"),
        (1, 2),
        rect(20.0, 0.0, 20.0),
        &prev_target_to_new,
        half,
    );
    assert_eq!(state.new_range, Some((1, 2)));

    // 第二笔：正文 "ab" -> "abc"（在位置 2 插入 c）。
    // 第一次的 [1,2) 映射到新坐标仍是 [1,2)，本次新增 [2,3)，
    // 累计后必须是 [1,3) —— 不能只剩 [2,3)。
    let prev_target_to_new = OffsetMap::build("ab", "abc");
    state.extend_insert(
        empty_snapshot(),
        String::from("abc"),
        (2, 3),
        rect(30.0, 0.0, 20.0),
        &prev_target_to_new,
        half,
    );
    assert_eq!(
        state.new_range,
        Some((1, 3)),
        "连续吐字必须累计整轮 burst 的新字范围，否则前一个字会被瞬间补全"
    );
}

/// Issue #826 评论 3 问题 2：连续 Delete 的 old_range 必须映射回 burst 最初坐标。
///
/// 例子 `ABC|DEF` 连续 Delete：第一次删 D，本次 old range = [3,4)；
/// 第二次删 E，本次 old range 仍是 [3,4)（因为 E 往前挪了一位），
/// 但在 burst 最初的 `ABCDEF` 坐标里应该累计成 [3,5)。
/// 靠 byte 数字碰巧一致是错的，必须用 OffsetMap 映射。
#[test]
fn extend_delete_maps_old_range_back_to_base_coordinates() {
    let now = Instant::now();
    let mut state = EditFrontierState::begin_delete(
        empty_snapshot(),
        String::from("ABCDEF"),
        empty_snapshot(),
        String::from("ABCEF"),
        (3, 4),
        rect(30.0, 0.0, 20.0),
        rect(30.0, 0.0, 20.0),
        now,
        160,
    );
    assert_eq!(state.old_range, Some((3, 4)));

    // 第二次删 E：base_text = "ABCDEF"，本次编辑前正文 = "ABCEF"，
    // 本次删除区间在 "ABCEF" 坐标里是 [3,4)（删掉 E）。
    // 映回 base 坐标是 [4,5)，与第一次的 [3,4) 合并成 [3,5)。
    let base_to_current = OffsetMap::build("ABCDEF", "ABCEF");
    state.extend_delete(
        empty_snapshot(),
        String::from("ABCE"),
        (3, 4),
        rect(30.0, 0.0, 20.0),
        &base_to_current,
        instant_at(now, 80),
    );
    assert_eq!(
        state.old_range,
        Some((3, 5)),
        "第二次删除必须映射回 base 坐标再 union，不能靠数字碰巧一致"
    );
    // base_snapshot 必须保持 burst 开始前的旧正文。
    assert_eq!(state.base_text, "ABCDEF");
}

#[test]
fn kind_can_extend_only_with_same_kind() {
    assert!(EditFrontierKind::Insert.can_extend(EditFrontierKind::Insert));
    assert!(!EditFrontierKind::Insert.can_extend(EditFrontierKind::Delete));
    assert!(!EditFrontierKind::Delete.can_extend(EditFrontierKind::Replace));
}

#[test]
fn kind_flags_match_mask_and_overlay_needs() {
    assert!(EditFrontierKind::Insert.needs_new_mask());
    assert!(!EditFrontierKind::Insert.needs_old_overlay());
    assert!(EditFrontierKind::Delete.needs_old_overlay());
    assert!(!EditFrontierKind::Delete.needs_new_mask());
    assert!(EditFrontierKind::Replace.needs_new_mask());
    assert!(EditFrontierKind::Replace.needs_old_overlay());
}

#[test]
fn degenerate_frontier_rect_is_detected() {
    assert!(FrontierRect {
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 10.0
    }
    .is_degenerate());
    assert!(!FrontierRect {
        x: 0.0,
        y: 0.0,
        w: 5.0,
        h: 10.0
    }
    .is_degenerate());
}
