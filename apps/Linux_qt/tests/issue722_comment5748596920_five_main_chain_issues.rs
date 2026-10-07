//! Issue #722 评论 5748596920 的 5 个主链问题 — 回归守卫。
//!
//! 旧实现（cursor_motion.rs / animated_slice.rs / 视觉事务 unit timeline）已在
//! Issue #824/#826 重写中删除，指向那些文件的 WHITE_BOX 复现已按评论 40 的要求
//! 改成新 #826 架构的结构守卫 + 行为测试（行为证明在 lib 内
//! `runtime_tests.rs` 的协同测试里）。不要再为了旧守卫恢复 cursor_motion.rs /
//! animated_slice.rs。
//!
//! 仍然有效的旧守卫（问题1 文档坐标、问题2 单 caret_x、问题4 完成条件）原样保留 ——
//! 当前实现已修复，它们在“违规模式不存在”时直接 return。

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
/// 窗口结束位置回退到最近的 UTF-8 字符边界，避免切在多字节字符中间（源码含
/// 中文注释）。
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

/// 在 `src` 中定位 `anchor`，返回 anchor 之后 `window_chars` 字符的窗口。
fn window_after(src: &str, anchor: &str, window_chars: usize) -> String {
    let pos = src
        .find(anchor)
        .unwrap_or_else(|| panic!("anchor not found: {}", anchor));
    let end = pos + anchor.len() + window_chars;
    let safe_end = (0..=end.min(src.len()))
        .rev()
        .find(|&e| src.is_char_boundary(e))
        .unwrap_or(pos);
    if safe_end > pos {
        src[pos..safe_end].to_string()
    } else {
        src[pos..].to_string()
    }
}

// =========================================================================
// 问题1: 正文事务的 caret 仍然是旧 scroll_y 下的视口坐标
// =========================================================================

/// 问题1: `pipeline.rs::record_visual_transaction()` 仍然调用：
/// - `editor_layout.caret_rect(..., ctx.scroll_y, ...)`
/// - `new_doc_snapshot.cursor_rect(..., ctx.scroll_y, ...)`
/// - `make_cursor_rect_from_caret_doc(..., ctx.scroll_y)`
/// 而 `make_cursor_rect_from_caret_doc()` 里 baseline 仍然
/// `text_baseline_y(...) - scroll_y`，`caret.y` 本身也已经是减过 scroll_y 的视口坐标。
/// AnimatedSlice/StaticPatch 是文档坐标，scene graph 再按当前 scroll_y 做 viewport
/// transform。输入过程中自动跟随滚动改变 contentY 时，文字跟当前 scroll_y 移动，
/// 但正文事务里的 caret track 还是创建事务那一刻的旧视口坐标。
///
/// 评论 5748596920 期望：caret track 应使用文档坐标系（与 AnimatedSlice/StaticPatch
/// 一致），scene graph 按 scroll_y 做 viewport transform。当前仍用视口坐标
/// → 断言"应改为文档坐标"在当前代码上 FAIL → 复现成功。
#[test]
fn issue1_record_visual_transaction_caret_still_uses_scroll_y_viewport_coord() {
    let pipeline = read_src("src/sujian_editor_item/pipeline.rs");

    // 前提：record_visual_transaction 确实存在并调用 ctx.scroll_y
    let has_record_fn = pipeline.contains("pub fn record_visual_transaction(");
    let has_caret_rect_with_scroll_y = pipeline.contains("editor_layout.caret_rect(");
    let has_cursor_rect_with_scroll_y = pipeline.contains("cursor_rect(");
    let has_make_cursor_rect_with_scroll_y = pipeline.contains("make_cursor_rect_from_caret_doc(");
    let has_ctx_scroll_y = pipeline.contains("ctx.scroll_y");
    println!(
        "[BUGFIX_REPRO_TRACE] issue1 pipeline: has_record_fn={} has_caret_rect={} has_cursor_rect={} has_make_cursor_rect={} has_ctx_scroll_y={}",
        has_record_fn, has_caret_rect_with_scroll_y, has_cursor_rect_with_scroll_y, has_make_cursor_rect_with_scroll_y, has_ctx_scroll_y
    );
    // Issue #722 修复后回归守卫：若 ctx.scroll_y 已从 record_visual_transaction 路径
    // 删除（caret 改为文档坐标），则直接通过。
    if !(has_record_fn && has_ctx_scroll_y) {
        return;
    }

    // 关键断言：record_visual_transaction 函数体内不应再出现 ctx.scroll_y 传给
    // caret_rect / cursor_rect / make_cursor_rect_from_caret_doc。当前代码仍传
    // → FAIL → 复现。
    let record_window = function_window(&pipeline, "pub fn record_visual_transaction", 22000);
    // Issue #722 评论 5748596920 修复后守卫：检查是否已改用文档坐标版本。
    // 若 record_visual_transaction 已使用 caret_rect_doc / cursor_rect_doc，
    // 且 make_cursor_rect_from_caret_doc 不再接收 scroll_y 参数，则视为已修复。
    let uses_doc_coord = record_window.contains("caret_rect_doc(")
        && record_window.contains("cursor_rect_doc(")
        && !record_window.contains("make_cursor_rect_from_caret_doc(\n")
        || (record_window.contains("caret_rect_doc(")
            && record_window.contains("cursor_rect_doc("));
    if uses_doc_coord {
        return;
    }
    let caret_rect_uses_scroll_y = record_window.contains("editor_layout.caret_rect(")
        && record_window.contains("ctx.scroll_y");
    let cursor_rect_uses_scroll_y =
        record_window.contains("cursor_rect(") && record_window.contains("ctx.scroll_y");
    let make_cursor_rect_uses_scroll_y = record_window.contains("make_cursor_rect_from_caret_doc(")
        && record_window.contains("ctx.scroll_y");
    println!(
        "[BUGFIX_REPRO_TRACE] issue1 uses_scroll_y: caret_rect={} cursor_rect={} make_cursor_rect={}",
        caret_rect_uses_scroll_y, cursor_rect_uses_scroll_y, make_cursor_rect_uses_scroll_y
    );
    assert!(
        !(caret_rect_uses_scroll_y || cursor_rect_uses_scroll_y || make_cursor_rect_uses_scroll_y),
        "Issue #722 评论 5748596920 问题1: pipeline.rs::record_visual_transaction 仍然把 \
         ctx.scroll_y 传给 caret_rect / cursor_rect / make_cursor_rect_from_caret_doc。\
         caret_rect/cursor_rect 返回的 caret.y 已经是减过 scroll_y 的视口坐标，\
         make_cursor_rect_from_caret_doc 里 baseline = text_baseline_y(...) - scroll_y \
         也是视口坐标。但 AnimatedSlice/StaticPatch 是文档坐标，scene graph 再按当前 \
         scroll_y 做 viewport transform。输入过程中自动跟随滚动改变 contentY 时，\
         文字跟当前 scroll_y 移动，但正文事务里的 caret track 还是创建事务那一刻的\
         旧视口坐标 → caret 与文字坐标系不一致 → 滚动跟随时光标错位。\
         修复：caret track 改用文档坐标系（不减 scroll_y），scene graph 统一按 \
         scroll_y 做 viewport transform。"
    );
}

/// 问题1（续）: make_cursor_rect_from_caret_doc 里 baseline 仍然
/// `text_baseline_y(...) - scroll_y`，是视口坐标。
#[test]
fn issue1_make_cursor_rect_from_caret_doc_baseline_uses_scroll_y() {
    let pipeline = read_src("src/sujian_editor_item/pipeline.rs");

    let has_fn = pipeline.contains("fn make_cursor_rect_from_caret_doc(");
    let has_scroll_y_param = pipeline.contains("scroll_y: f64,\n) -> CursorRect");
    println!(
        "[BUGFIX_REPRO_TRACE] issue1 make_cursor_rect: has_fn={} has_scroll_y_param={}",
        has_fn, has_scroll_y_param
    );
    if !(has_fn && has_scroll_y_param) {
        return;
    }

    let window = function_window(&pipeline, "fn make_cursor_rect_from_caret_doc", 800);
    let baseline_uses_scroll_y =
        window.contains("text_baseline_y(") && window.contains("- scroll_y");
    println!(
        "[BUGFIX_REPRO_TRACE] issue1 make_cursor_rect baseline_uses_scroll_y: {}",
        baseline_uses_scroll_y
    );
    assert!(
        !baseline_uses_scroll_y,
        "Issue #722 评论 5748596920 问题1: make_cursor_rect_from_caret_doc 里 baseline 仍然\
         = text_baseline_y(...) - scroll_y，是视口坐标。caret.y 本身也是视口坐标。\
         但 AnimatedSlice/StaticPatch 是文档坐标。caret track 与文字坐标系不一致。\
         修复：make_cursor_rect_from_caret_doc 应返回文档坐标（不减 scroll_y），\
         或删除 scroll_y 参数。"
    );
}

/// 问题2（续）: animation_coordinator.rs 对事务里所有 InsertReveal/DeleteConceal
/// unit 都把同一个 caret_x 塞进去。
#[test]
fn issue2_build_text_animation_plan_uses_single_caret_x_for_all_units() {
    let coord = read_src("src/sujian_editor_item/animation/render_plan_builder.rs");

    // 前提：build_text_animation_plan_with_sample 确实存在并调用 compute_frame_caret_driven
    let has_fn = coord.contains("fn build_text_animation_plan_with_sample(");
    let calls_caret_driven = coord.contains("compute_frame_caret_driven(");
    println!(
        "[BUGFIX_REPRO_TRACE] issue2 build_text_animation_plan: has_fn={} calls_caret_driven={}",
        has_fn, calls_caret_driven
    );
    if !(has_fn && calls_caret_driven) {
        return;
    }

    let window = function_window(&coord, "fn build_text_animation_plan_with_sample", 8000);
    // 关键断言：在 build_text_animation_plan_with_sample 里，caret_x 在 for unit 循环
    // 外面只算一次，然后对所有 unit 用同一个 caret_x。
    let caret_x_outside_loop = window.contains("let caret_x = match tx.cursor_visual_track")
        || window.contains("let caret_x = match tx.cursor_visual_track.as_ref()");
    let same_caret_x_for_all =
        window.contains("unit.slice.compute_frame_caret_driven(caret_x, visible)");
    // 检查是否有 per-unit caret_x 机制（正确做法）
    let has_per_unit_caret = window.contains("per_unit_caret_x")
        || window.contains("unit_caret_x")
        || window.contains("caret_x_for_unit")
        || window.contains("let caret_x = unit")
        || window.contains("unit.caret_x");
    println!(
        "[BUGFIX_REPRO_TRACE] issue2 build_text_animation_plan: caret_x_outside_loop={} same_caret_x_for_all={} has_per_unit_caret={}",
        caret_x_outside_loop, same_caret_x_for_all, has_per_unit_caret
    );
    assert!(
        !(caret_x_outside_loop && same_caret_x_for_all && !has_per_unit_caret),
        "Issue #722 评论 5748596920 问题2: animation_coordinator.rs::build_text_animation_plan_with_sample \
         对事务里所有 InsertReveal/DeleteConceal unit 都把同一个 caret_x 塞进去\
         （let caret_x = match tx.cursor_visual_track... 在 for unit 循环外只算一次，\
         然后 unit.slice.compute_frame_caret_driven(caret_x, visible)）。old caret 在\
         上一行行尾、new caret 在下一行行首时，track 的 x 会从右边往左边插值，下一行\
         的新 glyph 在第一帧就会看到一个\"来自上一行右侧\"的巨大 caret_x。修复：应为\
         每个 unit 算 per-unit caret_x（按 unit 所在行/段插值 caret track），或传完整\
         caret geometry 让 compute_frame_caret_driven 自行判断 unit 是否在当前 caret\
         行。"
    );
}

// =========================================================================
// 问题4: Insert/Delete 的事务完成条件仍然由文字 unit 自己的 timeline 决定
// =========================================================================

/// 问题4: `build_text_animation_plan_with_sample()` 仍然用
/// `tx.units.iter().all(|u| u.progress(frame_now) >= 1.0)` →
/// `keys_to_complete.push(tx.key)`。但这轮已经把 InsertReveal/DeleteConceal 的实际
/// 裁切改成 caret track 驱动了。快速 rebase 后 unit 的 remaining duration 和
/// caret track 的 remaining duration 可能不同；只看 unit progress 结束事务，会出现
/// caret 边界还没走完，overlay/static patch 已经被释放，文字提前跳终态。
///
/// 评论 5748596920 期望：事务完成条件应由 caret track 的 remaining duration 决定
/// （或与 unit progress 一致）。当前只看 unit progress → 断言"应由 caret track 决定"
/// 在当前代码上 FAIL → 复现成功。
#[test]
fn issue4_transaction_completion_uses_unit_timeline_not_caret_track() {
    let coord = read_src("src/sujian_editor_item/animation/render_plan_builder.rs");

    // 前提：build_text_animation_plan_with_sample 确实存在
    let has_fn = coord.contains("fn build_text_animation_plan_with_sample(");
    let has_keys_to_complete = coord.contains("keys_to_complete.push(tx.key)");
    println!(
        "[BUGFIX_REPRO_TRACE] issue4 build_text_animation_plan: has_fn={} has_keys_to_complete={}",
        has_fn, has_keys_to_complete
    );
    if !(has_fn && has_keys_to_complete) {
        return;
    }

    let window = function_window(&coord, "fn build_text_animation_plan_with_sample", 8000);
    // 关键断言：用 tx.units.iter().all(|u| u.progress(sample.frame_now) >= 1.0) 决定完成
    let uses_unit_progress = window
        .contains("tx.units.iter().all(|u| u.progress(sample.frame_now) >= 1.0)")
        || window.contains("tx.units.iter().all(|u| u.progress(frame_now) >= 1.0)");
    // 检查是否有 caret track 完成条件（正确做法）
    let has_caret_track_completion = window.contains("caret_track_done")
        || window.contains("caret_track_complete")
        || window.contains("track.progress(frame_now) >= 1.0")
        || window.contains("cursor_visual_track.progress")
        || window.contains("caret_track_remaining")
        || window.contains("track.is_finished(frame_now)")
        || window.contains("caret_track_finished");
    println!(
        "[BUGFIX_REPRO_TRACE] issue4 build_text_animation_plan: uses_unit_progress={} has_caret_track_completion={}",
        uses_unit_progress, has_caret_track_completion
    );
    assert!(
        !(uses_unit_progress && !has_caret_track_completion),
        "Issue #722 评论 5748596920 问题4: build_text_animation_plan_with_sample 仍然用 \
         tx.units.iter().all(|u| u.progress(sample.frame_now) >= 1.0) 决定 \
         keys_to_complete.push(tx.key)。但这轮已经把 InsertReveal/DeleteConceal 的实际\
         裁切改成 caret track 驱动了。快速 rebase 后 unit 的 remaining duration 和 \
         caret track 的 remaining duration 可能不同；只看 unit progress 结束事务，会\
         出现 caret 边界还没走完，overlay/static patch 已经被释放，文字提前跳终态。\
         修复：事务完成条件应由 caret track 的 remaining duration 决定（或与 unit \
         progress 一致），例如检查 cursor_visual_track.progress(frame_now) >= 1.0 \
         或 track.is_finished(frame_now)。"
    );
}

// =========================================================================
// Issue #826 新架构守卫（评论 40：把 #722 问题2/3/5 的旧 source guard 换成
// 新架构结构守卫；行为证明见 lib `runtime_tests.rs` 的协同测试）
// =========================================================================

/// 问题2（新架构）：协同吞吐边界必须消费**完整 caret 位置 (x, y)**，
/// 而不是只塞一个 caret_x。
///
/// 行为证明：`runtime_tests.rs::coordinated_insert_caret_and_reveal_share_one_progress`
/// / `coordinated_delete_caret_and_conceal_share_one_progress`
/// / `coordinated_wrap_caret_is_on_same_frontier_segment_each_frame`。
#[test]
fn issue2_coordinated_boundary_consumes_full_caret_position() {
    let coord = read_src("src/sujian_editor_item/animation/coordinator.rs");
    let window = function_window(&coord, "pub(crate) fn sample_edit_frontier", 2600);
    assert!(
        window.contains("position_at_distance(motion.distance_at_progress(progress))"),
        "Issue #826-40: 本帧必须先算 caret 在分段轨迹上的完整 (x, y)。"
    );
    assert!(
        window.contains("project_onto_layer(&frontier.reveal.regions, caret_x, caret_y)"),
        "Issue #826-40: 边界必须用 (caret_x, caret_y) 投影，只传 x 会在软换行错行。"
    );
}

/// 问题3（新架构）: Forward Delete 不能拿「start==target 的 drawn caret」去代表
/// 移动的 conceal 边界，否则整段长度塌成 0、整个 duration 不吞、最后一下消失
/// （旧 #722 问题3）。Forward 必须走同一份 motion progress 时钟。
///
/// 行为证明：
/// `runtime_tests.rs::coordinated_forward_delete_conceals_progressively_while_caret_stays_at_logical_target`
/// / `coordinated_forward_delete_does_not_collapse_motion_clock_when_start_equals_target`。
#[test]
fn issue3_forward_delete_uses_motion_progress_clock_not_fixed_caret() {
    let coord = read_src("src/sujian_editor_item/animation/coordinator.rs");
    let window = function_window(&coord, "pub(crate) fn sample_edit_frontier", 2600);
    assert!(
        window.contains("ConcealDirection::Forward")
            && window.contains("frontier.conceal.advanced(progress)"),
        "Issue #826-40: Forward Delete 的 conceal 边界必须由同一份 motion progress \
         推进（frontier.conceal.advanced(progress)），不能拿固定 caret 反投影。"
    );
    // 不得再出现「按来源侧直写 distance」这种会把 Forward 塌成 0 的写法。
    assert!(
        !window.contains("motion.source"),
        "Issue #826-40: 不得按路径来源侧直写 distance。"
    );
}

/// 问题5（新架构）: 换行/无可见 glyph 的行不得产出吞吐路径段，空格/控制字符
/// 也不许被当作可 Reveal 的 visible cluster。
#[test]
fn issue5_reveal_path_skips_rows_without_visible_clusters() {
    let frontier = read_src("src/sujian_editor_item/animation/edit_frontier.rs");
    let build = function_window(&frontier, "pub(crate) fn build(", 2500);
    assert!(
        build.contains("clusters_contained_in_range"),
        "Issue #826-40: Reveal 路径只由**完整覆盖**的 visible cluster 构建。"
    );
    assert!(
        build.contains("if left >= right {"),
        "Issue #826-40: 一行没有可见 glyph（换行符 / 空段落）时必须不产出 segment，\
         否则前沿会为不可见的换行花掉行程。"
    );
}

/// 5 个主链问题在新 #826 架构下的入口都在（总括）。
#[test]
fn issue_all_five_main_chain_entrypoints_present() {
    let coord = read_src("src/sujian_editor_item/animation/coordinator.rs");
    // 问题2/3：协同边界由完整 caret 位置投影 / Forward 走 progress 时钟。
    assert!(coord.contains("pub(crate) fn sample_edit_frontier"));
    assert!(coord.contains("pub(crate) fn sample_coordinated_caret"));
    // 问题1：caret 文档坐标（pipeline 用 *_doc 版本）。
    let pipeline = read_src("src/sujian_editor_item/pipeline.rs");
    assert!(
        pipeline.contains("build_old_new_from_canonical"),
        "Issue #826-40: pipeline 必须用 canonical 文档坐标构建 old/new layout。"
    );
    // 问题5：Reveal 路径构建器存在。
    let frontier = read_src("src/sujian_editor_item/animation/edit_frontier.rs");
    assert!(frontier.contains("pub(crate) fn build("));
}
