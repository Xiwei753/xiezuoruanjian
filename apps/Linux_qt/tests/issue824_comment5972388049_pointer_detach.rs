//! Issue #824 评论 5972388049 结构守卫 — PointerClick 只解绑 caret，文字继续收口。
//!
//! 复核结论：主路线（patch 驱动分类 / retarget 新 route / 单次全局 easing /
//! IME patch 事实源）已经对了，剩下一个阻塞：
//! `click_at()` → `hand_over_caret_ownership_to_pointer_click()` 复用了
//! `retire_caret_driven_units_for_transaction()`，把 `CaretTrack` 吞吐字直接
//! retire 到 progress=1，用户点击正文时半吞/半吐的字会瞬间跳到终态。
//!
//! 本守卫锁死修复后的职责划分：
//! 1. PointerClick 走独立的 detach 路径（采样当前屏幕帧 → Timed 继续收口），
//!    不再调用全局 retire；
//! 2. detach 转换与 builder 的 carried retarget 共用同一个 helper，不复制第二套；
//! 3. `click_at` 顺序是「先 detach → 再 bump epoch → 最后 Tween」；
//! 4. 正式诊断事件带上 `detached_caret_driven_units` / `snapped_caret_driven_units`，
//!    正常路径 snapped 必须为 0。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/source_guard.rs"]
mod source_guard;

use source_guard::{function_window, read_src};

const COORDINATOR: &str = "src/sujian_editor_item/animation/coordinator.rs";
const RETARGET: &str = "src/sujian_editor_item/animation/retarget_motion.rs";
const BUILDER: &str = "src/sujian_editor_item/animation/transaction_builder.rs";
const EDITING: &str = "src/sujian_editor_item/editing.rs";

const DETACH_MARKER: &str = "fn hand_over_caret_ownership_to_pointer_click";

/// 守卫 1a: PointerClick detach 不再复用全局 retire（那会把吞吐字直接掐到终态）。
#[test]
fn guard1a_pointer_detach_does_not_reuse_global_retire() {
    let src = read_src(COORDINATOR);
    let window = function_window(&src, DETACH_MARKER, 9000);
    assert!(
        !window.contains("retire_caret_driven_units_for_transaction("),
        "Issue #824 评论 5972388049: PointerClick detach 不得再调用 \
         retire_caret_driven_units_for_transaction()——它把 CaretTrack 吞吐字直接 \
         retire 到 progress=1（文字和 caret 一起被掐到终态）。"
    );
    assert!(
        window.contains("detach_caret_track_to_timed("),
        "Issue #824 评论 5972388049: PointerClick detach 必须把仍在 caret 驱动的 \
         InsertReveal/DeleteConceal 转成“从当前屏幕帧继续到终态”的 Timed unit。"
    );
    assert!(
        window.contains("sample_transaction_visual_state("),
        "Issue #824 评论 5972388049: detach 必须在同一个 now 采样 owner 的 coordinated \
         caret + CaretTrack glyph 当前屏幕帧。"
    );
    assert!(
        window.contains("caret_motion_retired = true"),
        "Issue #824 评论 5972388049: detach 之后必须设置 caret_motion_retired = true，\
         让旧事务永久失去 caret ownership。"
    );
}

/// 守卫 1b: 全局 retire 的“直接终态”语义保持不变（滚动 / layout basis 失效仍需要）。
#[test]
fn guard1b_global_retire_keeps_terminal_semantics() {
    let src = read_src(COORDINATOR);
    let window = function_window(&src, "fn retire_caret_driven_units_for_transaction", 2600);
    assert!(
        window.contains("retire_caret_driven_units()"),
        "Issue #824 评论 5972388049: 全局 retire 必须保持原语义（CaretTrack 直接收口到终态）；\
         PointerClick 只是不再复用它。"
    );
    assert!(
        !window.contains("detach_caret_track_to_timed("),
        "Issue #824 评论 5972388049: 不要把全局 retire 也改成“从当前帧继续”——\
         滚动 / layout 失效路径需要“直接终态”。"
    );
}

/// 守卫 2a: detach 转换抽成共用 helper，builder 的 carried retarget 调用它。
#[test]
fn guard2a_detach_conversion_is_a_shared_helper() {
    let retarget = read_src(RETARGET);
    assert!(
        retarget.contains("pub(crate) fn detach_caret_track_to_timed("),
        "Issue #824 评论 5972388049: 「当前 CaretTrack 视觉帧 → 脱离 caret 后的 Timed \
         收口状态」必须是 retarget_motion 里的共用 helper。"
    );
    let window = function_window(
        &retarget,
        "pub(crate) fn detach_caret_track_to_timed(",
        2200,
    );
    for required in [
        // 起点 = 当前可见比例，目标是终态，时长由调用方给。
        "start_fraction",
        "target_fraction",
        "AnimatedSliceKind::DeleteConceal => 0.0",
    ] {
        assert!(
            window.contains(required),
            "Issue #824 评论 5972388049: detach helper 必须处理 {required}。"
        );
    }
    // caret-driven 采样帧的可见比例反解（含 Reflow 防御分支）留在 helper 模块里。
    let fraction_window =
        function_window(&retarget, "fn caret_track_sampled_visible_fraction(", 2200);
    assert!(
        fraction_window
            .contains("AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade =>"),
        "Issue #824 评论 5972388049: 可见比例反解必须给 Reflow 留防御分支（它始终是 Timed，\
         不走 caret 几何）。"
    );

    let builder = read_src(BUILDER);
    assert!(
        builder.contains("detach_caret_track_to_timed("),
        "Issue #824 评论 5972388049: builder 的 carried retarget 必须调用共用 helper，\
         不在别处再复制一套。"
    );
    assert!(
        !builder.contains("compute_frame(mid)"),
        "Issue #824 评论 5972388049: 二分反解可见比例只能存在于共用 helper；\
         builder 里不得再有第二份。"
    );
}

/// 守卫 3: `click_at` 先 detach，再 bump epoch，最后 Tween 到点击目标。
#[test]
fn guard3_click_at_detaches_before_bumping_epoch() {
    let src = read_src(EDITING);
    let window = function_window(&src, "pub(crate) fn click_at(", 6000);
    let detach_pos = window
        .find("hand_over_caret_ownership_to_pointer_click(")
        .expect("Issue #824 评论 5972388049: click_at 必须调 PointerClick detach");
    let bump_pos = window
        .find("begin_manual_cursor_move()")
        .expect("Issue #824 评论 5972388049: click_at 仍必须 bump cursor owner epoch");
    // 用真实调用点（`let _ = self.update_cursor_visual_position();`）定位，
    // 不用注释里出现的同名文本。
    let tween_pos = window
        .find("_ = self.update_cursor_visual_position()")
        .expect("Issue #824 评论 5972388049: click_at 必须从当前视觉位置 Tween 到点击目标");
    assert!(
        detach_pos < bump_pos,
        "Issue #824 评论 5972388049: click_at 必须**先** detach（从当前屏幕帧继续收口）\
         **再** bump epoch；反过来会先让旧文字失去 caret 又不给它采样起点。"
    );
    assert!(
        bump_pos < tween_pos,
        "Issue #824 评论 5972388049: bump epoch 之后才更新光标视觉位置。"
    );
    assert!(
        window.contains("caret_target_before"),
        "Issue #824 评论 5972388049: click_at 必须保存 detach 前的 caret 视觉位置，\
         让诊断能报出 retarget 前后的 target。"
    );
}

/// 守卫 4: 正式诊断事件带上 detach 结果字段，正常路径 snapped 必须为 0。
#[test]
fn guard4_pointer_event_reports_detach_counts() {
    let src = read_src(EDITING);
    let window = function_window(&src, "fn record_pointer_click_caret_handover", 4200);
    for field in [
        "detached_caret_driven_units",
        "snapped_caret_driven_units",
        "editor.anim.pointer_caret_handover",
        "writer_diagnostics::record_event",
    ] {
        assert!(
            window.contains(field),
            "Issue #824 评论 5972388049: pointer caret handover 正式事件必须包含 {field}。"
        );
    }
    let coordinator = read_src(COORDINATOR);
    let outcome_window = function_window(
        &coordinator,
        "pub(crate) struct PointerCaretDetachOutcome",
        1200,
    );
    assert!(
        outcome_window.contains("detached_caret_driven_units")
            && outcome_window.contains("snapped_caret_driven_units"),
        "Issue #824 评论 5972388049: detach 结果必须显式回报 detached / snapped 两类计数。"
    );
}
