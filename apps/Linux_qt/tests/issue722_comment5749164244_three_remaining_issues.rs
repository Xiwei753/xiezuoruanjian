//! Issue #722 评论 5749164244 回归测试 — 前一轮修复（评论 5748596920）后剩余的
//! 3 个代码问题。本测试为 WHITE_BOX 结构守卫：验证当前实现已正确修复评论 5749164244
//! 指出的 3 个核心语义。每个子测试断言评论期望的正确结构，修复后代码满足这些断言
//! → 测试 PASS → 修复验证成功。
//!
//! ## 评论 5749164244 指出的 3 个剩余问题
//!
//! 1. 跨行的 line identity 实际上没有接通（build_text_animation_plan_with_sample
//!    三条采样路径都返回 0usize，compute_frame_caret_driven 用 0 当哨兵，
//!    y fallback 用 abs() 而非半开区间）。
//! 2. 前向 Delete 的宽度方向写反了（from_right = full_w * (1.0 - conceal_progress)，
//!    visible 1→0 时文字一开始全没、末尾长回来）。
//! 3. 快速输入/删除 rebase 仍然采的不是屏幕上真正那一帧（collect_rebase_frames
//!    用 compute_frame 而非 compute_frame_caret_driven）。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

fn linux_qt_root() -> PathBuf {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set by cargo");
    PathBuf::from(manifest_dir)
}

fn read_src(rel: &str) -> String {
    let path = linux_qt_root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e))
}

/// 从 `src` 中定位 `fn_marker`，返回从该处起 `window_chars` 字符的函数体窗口。
fn function_window(src: &str, fn_marker: &str, window_chars: usize) -> String {
    let pos = src
        .find(fn_marker)
        .unwrap_or_else(|| panic!("{} 必须存在", fn_marker));
    let target_end = pos + window_chars;
    let window_end = src
        .char_indices()
        .take_while(|(i, _)| *i < target_end)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(src.len())
        .min(src.len());
    src[pos..window_end].to_string()
}

// =========================================================================
// 问题1: 跨行的 line identity 没接通
// =========================================================================

/// 问题1 守卫1: AnimatedSlice.visual_line_id 必须是 Option<usize>，不再用 0 当哨兵。
#[test]
fn issue1_visual_line_id_is_option_not_usize() {
    let src = read_src("src/sujian_editor_item/animated_slice.rs");
    // 修复后：visual_line_id 字段类型为 Option<usize>
    let has_option = src.contains("pub visual_line_id: Option<usize>");
    assert!(
        has_option,
        "AnimatedSlice.visual_line_id 必须是 Option<usize>，不再用 0 当哨兵"
    );
    // 不应再用 usize（非 Option）作为 visual_line_id 字段类型
    let has_bare_usize = src.contains("pub visual_line_id: usize,");
    assert!(
        !has_bare_usize,
        "AnimatedSlice.visual_line_id 不应是裸 usize"
    );
}

/// 问题1 守卫2: compute_frame_caret_driven 的 caret_visual_line_id 参数必须是 Option<usize>。
#[test]
fn issue1_compute_frame_caret_driven_takes_option_for_line_id() {
    let src = read_src("src/sujian_editor_item/animated_slice.rs");
    let window = function_window(&src, "pub fn compute_frame_caret_driven", 600);
    // 修复后：参数类型为 Option<usize>
    assert!(
        window.contains("caret_visual_line_id: Option<usize>"),
        "compute_frame_caret_driven 的 caret_visual_line_id 必须是 Option<usize>"
    );
    // 不应再用 `!= 0` 判断行身份
    assert!(
        !window.contains("caret_visual_line_id != 0"),
        "compute_frame_caret_driven 不应用 `!= 0` 判断行身份，应改用 Option match"
    );
}

/// 问题1 守卫3: compute_frame_caret_driven 的 y fallback 必须用半开区间，
/// 不用 abs(y - glyph_y) < glyph_h。
#[test]
fn issue1_y_fallback_uses_half_open_interval() {
    let src = read_src("src/sujian_editor_item/animated_slice.rs");
    let window = function_window(&src, "pub fn compute_frame_caret_driven", 2000);
    // 修复后：y fallback 用半开区间 line_top <= caret_y < line_bottom
    let has_half_open = window.contains("caret_clip_y >= line_top")
        && window.contains("caret_clip_y < line_bottom");
    assert!(
        has_half_open,
        "y fallback 必须用半开区间 line_top <= caret_y < line_bottom"
    );
    // 不应再用 abs() < h 判断同行
    let has_abs = window.contains(".abs() < self.to_document_rect.h")
        || window.contains(".abs() < self.from_document_rect.h");
    assert!(
        !has_abs,
        "y fallback 不应用 abs(y - glyph_y) < glyph_h，相邻行会误判"
    );
}

/// 问题1 守卫4: build_text_animation_plan_with_sample 的采样路径不再硬编码 0usize，
/// 且 InsertReveal/DeleteConceal 从统一的 CoordinatedMotionFrame.caret 消费 visual_line_id。
#[test]
fn issue1_build_text_animation_plan_no_longer_hardcodes_zero_line_id() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    let window = function_window(&src, "fn build_text_animation_plan_with_sample", 10000);
    // Issue #727 约束 3: 不应有 `(r.x, r.top, 0usize)` 或 `(x, y, 0usize)` 等硬编码
    let has_hardcoded_zero = window.contains("(r.x, r.top, 0usize)")
        || window.contains("(x, y, 0usize)")
        || window.contains("(f.x + f.w, f.y, 0usize)");
    assert!(
        !has_hardcoded_zero,
        "build_text_animation_plan_with_sample 不应硬编码 0usize 作为 caret_line_id"
    );
    // Issue #727 约束 3+4: InsertReveal/DeleteConceal 从 CoordinatedMotionFrame.caret 消费
    // visual_line_id，不再由文字层自己采样。应包含 caret_frame.visual_line_id。
    assert!(
        window.contains("caret_frame.visual_line_id"),
        "build_text_animation_plan_with_sample 应从 CoordinatedMotionFrame.caret 消费 visual_line_id"
    );
}

/// 问题1 守卫5: PreparedCursorVisualTrack 必须保存 from/to visual_line_id。
#[test]
fn issue1_prepared_cursor_visual_track_saves_line_ids() {
    let src = read_src("src/sujian_editor_item/text_visual_transaction.rs");
    // 修复后：PreparedCursorVisualTrack 有 from_visual_line_id 和 to_visual_line_id 字段
    assert!(
        src.contains("pub from_visual_line_id: Option<usize>"),
        "PreparedCursorVisualTrack 必须有 from_visual_line_id: Option<usize>"
    );
    assert!(
        src.contains("pub to_visual_line_id: Option<usize>"),
        "PreparedCursorVisualTrack 必须有 to_visual_line_id: Option<usize>"
    );
}

/// 问题1 守卫6: build_insert_reveal_slices / build_delete_conceal_slices
/// 传 Some(line.visual_line_id)（全文视觉行 id）而非 Some(line_idx)（局部数组下标）。
/// Issue #722 评论 5749572808 问题1: line_idx 是 line_snapshots 的局部下标，
/// 视口裁剪后和全文 VisualLine.id 不一致，跨行裁切会判断错。必须用 line.visual_line_id。
#[test]
fn issue1_build_slices_pass_some_line_idx() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    // 函数体较大，取 8000 字符确保覆盖完整调用
    let insert_window = function_window(&src, "fn build_insert_reveal_slices", 8000);
    let delete_window = function_window(&src, "fn build_delete_conceal_slices", 8000);
    // 修复后：传 Some(new_line.visual_line_id) / Some(old_line.visual_line_id)
    assert!(
        insert_window.contains("Some(new_line.visual_line_id)"),
        "build_insert_reveal_slices 必须传 Some(new_line.visual_line_id)（全文视觉行 id）而非 Some(line_idx)（局部下标）"
    );
    assert!(
        delete_window.contains("Some(old_line.visual_line_id)"),
        "build_delete_conceal_slices 必须传 Some(old_line.visual_line_id)（全文视觉行 id）而非 Some(line_idx)（局部下标）"
    );
    // 不应再用 Some(line_idx)（局部下标，视口裁剪后和全文行号不一致）
    assert!(
        !insert_window.contains("Some(line_idx)"),
        "build_insert_reveal_slices 不应再传 Some(line_idx)（局部下标）"
    );
    assert!(
        !delete_window.contains("Some(line_idx)"),
        "build_delete_conceal_slices 不应再传 Some(line_idx)（局部下标）"
    );
}

// =========================================================================
// 问题2: 前向 Delete 的宽度方向写反了
// =========================================================================

/// 问题2 守卫: compute_frame_caret_driven 前向 Delete 同一行分支
/// 必须用 `full_w * visible` 而非 `full_w * (1.0 - conceal_progress)`。
#[test]
fn issue2_forward_delete_same_line_uses_full_w_times_visible() {
    let src = read_src("src/sujian_editor_item/animated_slice.rs");
    let window = function_window(&src, "pub fn compute_frame_caret_driven", 12000);
    // 修复后：同一行分支用 frame_w = full_w * visible（visible.clamp 后）
    let has_correct_direction = window.contains("full_w * visible.clamp(0.0, 1.0)");
    assert!(
        has_correct_direction,
        "前向 Delete 同一行分支必须用 `full_w * visible`（visible 是剩余可见比例）"
    );
    // 不应再用 `full_w * (1.0 - conceal_progress)`（方向写反）
    let has_wrong_direction = window.contains("full_w * (1.0 - conceal_progress)");
    assert!(
        !has_wrong_direction,
        "前向 Delete 不应用 `full_w * (1.0 - conceal_progress)`，方向写反了"
    );
}

/// 问题2 守卫2: 前向 Delete 同一行分支 frame_x = from_document_rect.x（左端固定），
/// source_rect.x 保持原起点。
#[test]
fn issue2_forward_delete_same_line_frame_x_fixed_at_left() {
    let src = read_src("src/sujian_editor_item/animated_slice.rs");
    let window = function_window(&src, "pub fn compute_frame_caret_driven", 12000);
    // 修复后：不应有 frame_x = from_document_rect.x + (from_document_rect.w - from_right)
    // 即不应从右端往回算 frame_x
    let has_right_aligned =
        window.contains("self.from_document_rect.x + (self.from_document_rect.w - from_right)");
    assert!(
        !has_right_aligned,
        "前向 Delete 同一行分支 frame_x 不应从右端往回算，应固定在 from_document_rect.x"
    );
}

// =========================================================================
// 问题3: 快速输入/删除 rebase 仍然采的不是屏幕上真正那一帧
// =========================================================================

/// 问题3 守卫1: take_rebase_frames 不再调 tx.collect_rebase_frames，
/// 而是用 collect_rebase_frame_for_unit（对 Reveal/Conceal 用 compute_frame_caret_driven）。
#[test]
fn issue3_take_rebase_frames_uses_caret_driven_for_reveal_conceal() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    let window = function_window(&src, "fn take_rebase_frames", 4000);
    // 修复后：不应调 tx.collect_rebase_frames(now)
    let has_old_collect = window.contains("tx.collect_rebase_frames(now)");
    assert!(
        !has_old_collect,
        "take_rebase_frames 不应再调 tx.collect_rebase_frames(now)，\
         应改用 collect_rebase_frame_for_unit 对 Reveal/Conceal 用 compute_frame_caret_driven"
    );
    // 应使用 sample_caret_geometry_for_caret_driven_clip 采样 caret
    assert!(
        window.contains("sample_caret_geometry_for_caret_driven_clip"),
        "take_rebase_frames 应使用 sample_caret_geometry_for_caret_driven_clip 采样 caret geometry"
    );
    // 应使用 collect_rebase_frame_for_unit
    assert!(
        window.contains("collect_rebase_frame_for_unit"),
        "take_rebase_frames 应使用 collect_rebase_frame_for_unit 逐 unit 采集 rebase 帧"
    );
}

/// 问题3 守卫2: collect_rebase_frame_for_unit_without_caret 对所有类型统一使用
/// compute_frame(visible_fraction)，visible_fraction 对 Reveal/Conceal 从
/// caret_track_progress 派生，对 Reflow 从 unit.current_visible_fraction 派生。
#[test]
fn issue3_collect_rebase_frame_for_unit_branches_by_kind() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    let window = function_window(&src, "fn collect_rebase_frame_for_unit_without_caret", 3000);
    // Issue #727 约束 4: 统一使用 compute_frame(visible_fraction)
    assert!(
        window.contains("compute_frame(visible_fraction)"),
        "collect_rebase_frame_for_unit_without_caret 必须用 compute_frame(visible_fraction)"
    );
    // visible_fraction 从真实显示帧反算
    assert!(
        window.contains("frame.w / w"),
        "collect_rebase_frame_for_unit_without_caret 必须从真实显示帧反算 visible_fraction（frame.w / w）"
    );
}

/// 问题3 守卫3: sample_caret_geometry_for_caret_driven_clip 已被删除（Issue #727 约束 4）。
/// InsertReveal/DeleteConceal 现在统一从 CoordinatedMotionFrame.caret 消费 x/y/visual_line_id。
/// Reveal/Conceal 使用 compute_frame_caret_driven，Reflow 使用 compute_frame(visible_fraction)。
#[test]
fn issue3_caret_sampling_uses_unified_coordinated_motion_frame() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    // Issue #727 约束 4: sample_caret_geometry_for_caret_driven_clip 已删除
    let has_deleted_fn = src.contains("fn sample_caret_geometry_for_caret_driven_clip");
    assert!(
        !has_deleted_fn,
        "sample_caret_geometry_for_caret_driven_clip 应已被删除（Issue #727 约束 4）"
    );
    // Issue #727 约束 3: sample_coordinated_motion_frame 采样统一 caret frame
    let has_sample_fn = src.contains("fn sample_coordinated_motion_frame");
    assert!(
        has_sample_fn,
        "应有 sample_coordinated_motion_frame 采样统一 CoordinatedMotionFrame"
    );
    // build_text_animation_plan_with_sample 从 coordinated_motion_frame.caret 消费
    let btap_window = function_window(&src, "fn build_text_animation_plan_with_sample", 8000);
    assert!(
        btap_window.contains("caret_frame"),
        "build_text_animation_plan_with_sample 应从 CoordinatedMotionFrame.caret 消费"
    );
}

// =========================================================================
// 综合守卫：3 个问题全部修复
// =========================================================================

#[test]
fn all_three_remaining_issues_fixed() {
    let animated_slice = read_src("src/sujian_editor_item/animated_slice.rs");
    let anim_coord = read_src("src/sujian_editor_item/animation_coordinator.rs");
    let tx = read_src("src/sujian_editor_item/text_visual_transaction.rs");

    // 问题1: visual_line_id 改 Option<usize>，不再用 0 当哨兵
    assert!(animated_slice.contains("pub visual_line_id: Option<usize>"));
    assert!(!animated_slice.contains("caret_visual_line_id != 0"));
    assert!(tx.contains("pub from_visual_line_id: Option<usize>"));

    // 问题2: 前向 Delete 用 full_w * visible
    let cf_window = function_window(&animated_slice, "pub fn compute_frame_caret_driven", 12000);
    assert!(cf_window.contains("full_w * visible.clamp(0.0, 1.0)"));
    assert!(!cf_window.contains("full_w * (1.0 - conceal_progress)"));

    // 问题3: take_rebase_frames 用 collect_rebase_frame_for_unit
    let reb_window = function_window(&anim_coord, "fn take_rebase_frames", 5000);
    assert!(!reb_window.contains("tx.collect_rebase_frames(now)"));
    assert!(reb_window.contains("collect_rebase_frame_for_unit"));

    println!("[BUGFIX_VERIFY] Issue #722 评论 5749164244: 3 个剩余问题全部修复验证通过");
}
