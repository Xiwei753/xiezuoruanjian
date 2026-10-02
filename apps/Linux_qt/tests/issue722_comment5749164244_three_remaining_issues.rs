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
//!
//! ## Issue #727 / #785 之后的变化（2026-09）
//!
//! 问题 1、2 的载体 `AnimatedSlice::compute_frame_caret_driven`（caret 驱动裁切）已被
//! 生产路径整体取代：文字 unit 统一用 `unit.current_visible_fraction(now)` 的 Timed
//! 时间线，再 `slice.compute_frame(visible)`（`rebase.rs::collect_rebase_frame_for_unit_without_caret`
//! 与 `render_plan_builder.rs::build_text_animation_plan_with_sample` 都是这条路径）。
//! 该函数在 `--bin` 构建下无调用方，源码连同行身份判断、y 半开区间 fallback、
//! 前向 Delete 宽度方向一起删除 —— `tools/check_rust_safety_patterns.py` 禁止用
//! `#[allow(dead_code)]` 或 `#[cfg_attr(..., allow(dead_code))]` 保留未使用代码。
//! 因此原来针对该函数体的 4 个子测试一并退役，守卫改到「旧机制不复活」+「Timed 路径在位」。

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

/// 问题1 守卫4: build_text_animation_plan_with_sample 的采样路径不再硬编码 0usize，
/// 且 InsertReveal/DeleteConceal 从统一的 CoordinatedMotionFrame.caret 消费 visual_line_id。
#[test]
fn issue1_build_text_animation_plan_no_longer_hardcodes_zero_line_id() {
    let src = read_src("src/sujian_editor_item/animation/render_plan_builder.rs");
    let window = function_window(&src, "fn build_text_animation_plan_with_sample", 14000);
    // Issue #727 约束 3: 不应有 `(r.x, r.top, 0usize)` 或 `(x, y, 0usize)` 等硬编码
    let has_hardcoded_zero = window.contains("(r.x, r.top, 0usize)")
        || window.contains("(x, y, 0usize)")
        || window.contains("(f.x + f.w, f.y, 0usize)");
    assert!(
        !has_hardcoded_zero,
        "build_text_animation_plan_with_sample 不应硬编码 0usize 作为 caret_line_id"
    );
    // Issue #785: 文字 unit 统一走 Timed 路径（current_visible_fraction + compute_frame(visible)），
    // 不再从 caret_frame 消费 visual_line_id。caret frame 只负责画 caret，不驱动文字。
    // 即使 cursor ownership/epoch 发生切换，文字动画也不会凭空消失。
    assert!(
        window.contains("current_visible_fraction")
            && window.contains("compute_frame(visible)"),
        "build_text_animation_plan_with_sample 应走 Timed 路径（current_visible_fraction + compute_frame(visible)），不从 caret_frame 消费"
    );
}

/// 问题1 守卫5: PreparedCursorVisualTrack 必须保存 from/to visual_line_id。
#[test]
fn issue1_prepared_cursor_visual_track_saves_line_ids() {
    let src = read_src("src/sujian_editor_item/animation/transaction/types.rs");
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
    // slice 构造已拆到 transaction_builder/slices.rs（见该文件头注释）
    let src = read_src("src/sujian_editor_item/animation/transaction_builder/slices.rs");
    // 函数体较大，取 8000 字符确保覆盖完整调用
    let insert_window = function_window(&src, "fn build_insert_reveal_slices", 10600);
    let delete_window = function_window(&src, "fn build_delete_conceal_slices", 9000);
    // Issue #815 评论 5949097065: 行身份只认当前这侧的 canonical 行序。
    // Insert 用 new snapshot 自己的 inserted_range.start 行序；Delete 用 old
    // snapshot 自己的 old caret 行序，绝不跨 snapshot 比大小。
    assert!(
        insert_window
            .contains("same_side_start_x(new_snapshot, ingest_from_line_ord, range_start)"),
        "build_insert_reveal_slices 的吐字起点必须来自 new snapshot 自己的行序与 x"
    );
    assert!(
        delete_window
            .contains("line_ordinal_for_visual_line_id(old_snapshot, old_cursor_visual_line_id)"),
        "build_delete_conceal_slices 的吞字起点行序必须来自 old snapshot 自己的 visual_line_id"
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

// =========================================================================
// 问题3: 快速输入/删除 rebase 仍然采的不是屏幕上真正那一帧
// =========================================================================

/// 问题3 守卫1: take_rebase_frames 走两条交棒路径（#815 后文字 Timed + caret track 统一采样）。
/// - 文字交棒：collect_rebase_frame_for_unit_without_caret(...)，文字进度来自 Timed unit。
/// - caret 交棒：sample_caret_track_frame(track, now)，拿到本帧 caret 的 x/y/visual_line_id/
///   rect，新 track 从这个**当前** caret 连到新目标 caret，不退回逻辑旧 caret。
/// 旧的 sample_caret_geometry_for_caret_driven_clip 已删除，不应复活。
#[test]
fn issue3_take_rebase_frames_uses_timed_text_and_coordinated_caret_handoff() {
    let src = read_src("src/sujian_editor_item/animation/rebase.rs");
    let window = function_window(&src, "fn take_rebase_frames", 8000);
    // 修复后：不应调 tx.collect_rebase_frames(now)
    let has_old_collect = window.contains("tx.collect_rebase_frames(now)");
    assert!(
        !has_old_collect,
        "take_rebase_frames 不应再调 tx.collect_rebase_frames(now)，\
         应改用 collect_rebase_frame_for_unit_without_caret 逐 unit 采集 rebase 帧"
    );
    // 文字交棒路径：应使用 collect_rebase_frame_for_unit_without_caret(
    assert!(
        window.contains("collect_rebase_frame_for_unit_without_caret("),
        "take_rebase_frames 应使用 collect_rebase_frame_for_unit_without_caret( 采集文字 rebase 帧（文字进度来自 Timed unit）"
    );
    // caret 交棒路径：应使用 Issue #815 的统一采样入口 sample_caret_track_frame(
    assert!(
        window.contains("sample_caret_track_frame("),
        "take_rebase_frames 应使用 sample_caret_track_frame( 采样本帧 caret handoff"
    );
    // Issue #815: 新 track 从采样到的当前 caret 起算，不得退回逻辑旧 caret
    assert!(
        window.contains("let sampled = frame.rect;"),
        "take_rebase_frames 的 caret handoff 起点必须是本帧采样到的 caret rect"
    );
    // 旧机制不复活：函数体里不应有真正的 sample_caret_geometry_for_caret_driven_clip( 调用
    assert!(
        !window.contains("sample_caret_geometry_for_caret_driven_clip("),
        "take_rebase_frames 不应再调用已删除的 sample_caret_geometry_for_caret_driven_clip(（旧 caret 驱动裁切机制不应复活）"
    );
}

/// 问题3 守卫2: collect_rebase_frame_for_unit_without_caret 对所有类型统一使用
/// compute_frame(visible_fraction)，visible_fraction 直接取 unit.current_visible_fraction(now)，
/// effective_fraction 也直接等于 visible_fraction——不再从局部 frame 宽度反推。
/// Issue #808 评论 5921324618: 多个 slice 共用 line_mask 后，从 frame.w / slice.w
/// 反推会把统一 timeline 进度拆成不同局部进度，导致跳变。
#[test]
fn issue3_collect_rebase_frame_for_unit_branches_by_kind() {
    let src = read_src("src/sujian_editor_item/animation/rebase.rs");
    let window = function_window(&src, "fn collect_rebase_frame_for_unit_without_caret", 3000);
    // Issue #727 约束 4: 统一使用 compute_frame(visible_fraction)
    assert!(
        window.contains("compute_frame(visible_fraction)"),
        "collect_rebase_frame_for_unit_without_caret 必须用 compute_frame(visible_fraction)"
    );
    // Issue #808 评论 5921324618: visible_fraction 直接取 timeline，effective_fraction = visible_fraction
    assert!(
        window.contains("let effective_fraction = visible_fraction;"),
        "collect_rebase_frame_for_unit_without_caret 的 effective_fraction 必须直接等于 visible_fraction（timeline），不从 frame.w 反推"
    );
    // 不应再从 frame.w / w 反推
    assert!(
        !window.contains("frame.w / w"),
        "collect_rebase_frame_for_unit_without_caret 不应再从 frame.w / w 反推 visible_fraction（Issue #808 评论 5921324618）"
    );
}

/// 问题3 守卫3: sample_caret_geometry_for_caret_driven_clip 已被删除（Issue #727 约束 4）。
/// 文字 unit（InsertReveal/DeleteConceal/Reflow）不消费 caret frame，统一走
/// current_visible_fraction + compute_frame（Timed 时间线）。caret frame 只负责画/交棒 caret，
/// 不驱动文字。协同只传递"同事务协同"语义，不再把 caret duration 强绑到 typing duration。
#[test]
fn issue3_caret_sampling_uses_unified_coordinated_motion_frame() {
    let cursor_motion = read_src("src/sujian_editor_item/animation/cursor_motion.rs");
    let render_plan = read_src("src/sujian_editor_item/animation/render_plan_builder.rs");
    // Issue #727 约束 4: sample_caret_geometry_for_caret_driven_clip 已删除
    let has_deleted_fn = cursor_motion.contains("fn sample_caret_geometry_for_caret_driven_clip");
    assert!(
        !has_deleted_fn,
        "sample_caret_geometry_for_caret_driven_clip 应已被删除（Issue #727 约束 4）"
    );
    // Issue #727 约束 3: sample_coordinated_motion_frame 采样统一 caret frame
    let has_sample_fn = cursor_motion.contains("fn sample_coordinated_motion_frame");
    assert!(
        has_sample_fn,
        "应有 sample_coordinated_motion_frame 采样统一 CoordinatedMotionFrame"
    );
    // Issue #785: 文字 unit 统一走 Timed 路径（current_visible_fraction），不消费 caret_frame。
    // caret frame 只负责画 caret，不驱动文字。协同只传递"同事务协同"语义（同首帧/同 rebase），
    // 不再把 caret duration 强绑到 typing duration。
    let btap_window = function_window(
        &render_plan,
        "fn build_text_animation_plan_with_sample",
        14000,
    );
    // Issue #815 评论 5949097065: 两条路径并存但分工明确——协同吞吐字吃
    // cursor track 当前帧（compute_frame_by_caret_ingest），非协同文字动画与
    // Reflow 才走 Timed 路径（current_visible_fraction）。
    assert!(
        btap_window.contains("compute_frame_by_caret_ingest("),
        "build_text_animation_plan_with_sample 必须让协同吞吐字吃 cursor track 当前帧"
    );
    assert!(
        btap_window.contains("current_visible_fraction"),
        "build_text_animation_plan_with_sample 必须让非协同 Timed unit 走自己的 visible fraction"
    );
}

// =========================================================================
// 综合守卫：3 个问题全部修复
// =========================================================================

#[test]
fn all_three_remaining_issues_fixed() {
    let animated_slice = read_src("src/sujian_editor_item/animated_slice.rs");
    let anim_coord = read_src("src/sujian_editor_item/animation/rebase.rs");
    let tx = read_src("src/sujian_editor_item/animation/transaction/types.rs");

    // 问题1: visual_line_id 改 Option<usize>，不再用 0 当哨兵
    assert!(animated_slice.contains("pub visual_line_id: Option<usize>"));
    assert!(!animated_slice.contains("caret_visual_line_id != 0"));
    assert!(tx.contains("pub from_visual_line_id: Option<usize>"));

    // 问题2（前向 Delete 宽度方向）原由 `compute_frame_caret_driven` 承载。
    // Issue #727 约束 4 + Issue #785 之后，文字 unit 统一走
    // `unit.current_visible_fraction(now)` + `slice.compute_frame(visible)`，
    // 该函数在生产路径已无调用方，源码连同它的行身份判断、y 半开区间 fallback、
    // 前向 Delete 宽度方向一起删除（check_rust_safety_patterns 禁止用
    // `#[allow(dead_code)]` 保留）。这里改为守卫"旧机制不会复活"。
    assert!(
        !animated_slice.contains("pub fn compute_frame_caret_driven"),
        "compute_frame_caret_driven 已被 #727/#785 的 Timed 路径取代，不应复活"
    );
    let rebase = read_src("src/sujian_editor_item/animation/rebase.rs");
    assert!(
        rebase.contains("unit.slice.compute_frame(visible_fraction)"),
        "所有 unit（含 Reveal/Conceal）都应从自己的时间线取 visible 并调 compute_frame"
    );

    // 问题3: take_rebase_frames 用 collect_rebase_frame_for_unit
    let reb_window = function_window(&anim_coord, "fn take_rebase_frames", 5000);
    assert!(!reb_window.contains("tx.collect_rebase_frames(now)"));
    assert!(reb_window.contains("collect_rebase_frame_for_unit"));

    println!("[BUGFIX_VERIFY] Issue #722 评论 5749164244: 3 个剩余问题全部修复验证通过");
}
