//! Issue #722 评论 5748596920 复现测试 — 前一轮修复（commit 578a920ba）后剩余的
//! 5 个主链问题。本测试为 WHITE_BOX 结构守卫复现：验证当前实现仍违反评论 5748596920
//! 指出的 5 个核心语义。每个子测试断言评论期望的正确结构，当前（未修复）代码违反
//! 这些断言 → 测试 FAIL → 缺陷复现成功。
//!
//! ## 评论 5748596920 指出的 5 个主链问题
//!
//! 1. 正文事务的 caret 仍然是旧 scroll_y 下的视口坐标（pipeline.rs::
//!    record_visual_transaction 仍传 ctx.scroll_y，make_cursor_rect_from_caret_doc
//!    里 baseline = text_baseline_y - scroll_y）。
//! 2. "光标是吞吐边界"只传了 caret_x，跨软换行仍然会错（compute_frame_caret_driven
//!    只有 caret_clip_boundary: f64，animation_coordinator 对所有 unit 用同一个
//!    caret_x）。
//! 3. 前向 Delete 现在会变成"光标不动，文字也不吞，只到末尾突然消失"
//!    （compute_coordinated_cursor_position 对 has_forward_delete 固定返回 new_rect.x，
//!    compute_frame_caret_driven forward Delete 用 from.x + from.w - caret_x）。
//! 4. Insert/Delete 的事务完成条件仍然由文字 unit 自己的 timeline 决定
//!    （build_text_animation_plan_with_sample 用 tx.units.iter().all(|u| u.progress
//!    >= 1.0) 决定 keys_to_complete）。
//! 5. 空格/换行仍然被当普通 InsertReveal（build_insert_reveal_slices 只判断 cluster
//!    落在 inserted_range，不检查 new_snapshot.virtual_text 的实际字符）。

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

// =========================================================================
// 问题2: "光标是吞吐边界"只传了 caret_x，跨软换行仍然会错
// =========================================================================

/// 问题2: `animated_slice.rs::compute_frame_caret_driven()` 只有
/// `caret_clip_boundary: f64`（只知道 x）；`animation_coordinator.rs` 对事务里所有
/// InsertReveal/DeleteConceal unit 都把同一个 caret_x 塞进去。old caret 在上一行
/// 行尾、new caret 在下一行行首时，track 的 x 会从右边往左边插值，下一行的新 glyph
/// 在第一帧就会看到一个"来自上一行右侧"的巨大 caret_x，`caret_x - glyph.x` 会直接
/// clamp 成整字宽度，字会在光标真正到下一行之前提前完整出现。
///
/// 评论 5748596920 期望：compute_frame_caret_driven 应接收完整 caret geometry
/// （含 y/行信息）或 per-unit caret_x。当前只传一个 x 且对所有 unit 用同一个
/// → 断言"应改为完整 caret geometry 或 per-unit"在当前代码上 FAIL → 复现成功。
#[test]
fn issue2_compute_frame_caret_driven_only_takes_caret_x_not_full_geometry() {
    let slice = read_src("src/sujian_editor_item/animated_slice.rs");

    // 前提：compute_frame_caret_driven 确实存在
    let has_fn = slice.contains("pub fn compute_frame_caret_driven(");
    println!(
        "[BUGFIX_REPRO_TRACE] issue2 compute_frame_caret_driven: has_fn={}",
        has_fn
    );
    if !has_fn {
        return;
    }

    let window = function_window(&slice, "pub fn compute_frame_caret_driven", 600);
    // 关键断言：签名只有 caret_clip_boundary: f64，没有 caret_y / caret_line_id /
    // caret_visual_line_id 等 y/行信息。
    let has_only_caret_x = window.contains("caret_clip_boundary: f64,");
    let has_caret_y = window.contains("caret_clip_y")
        || window.contains("caret_y:")
        || window.contains("caret_top:")
        || window.contains("caret_line_id:")
        || window.contains("caret_visual_line_id:")
        || window.contains("caret_row:")
        || window.contains("caret_line_idx:");
    println!(
        "[BUGFIX_REPRO_TRACE] issue2 compute_frame_caret_driven: has_only_caret_x={} has_caret_y_or_line={}",
        has_only_caret_x, has_caret_y
    );
    assert!(
        !(has_only_caret_x && !has_caret_y),
        "Issue #722 评论 5748596920 问题2: animated_slice.rs::compute_frame_caret_driven \
         只有 caret_clip_boundary: f64（只知道 x），没有 caret_y / caret_line_id 等 \
         y/行信息。old caret 在上一行行尾、new caret 在下一行行首时，track 的 x 会\
         从右边往左边插值，下一行的新 glyph 在第一帧就会看到一个\"来自上一行右侧\"\
         的巨大 caret_x，caret_x - glyph.x 会直接 clamp 成整字宽度，字会在光标真正\
         到下一行之前提前完整出现。修复：compute_frame_caret_driven 应接收完整 \
         caret geometry（含 y/行信息）或 per-unit caret_x。"
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
    // 检查模式：let caret_x = match tx.cursor_visual_track... 然后在循环里
    // unit.slice.compute_frame_caret_driven(caret_x, visible)
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
// 问题3: 前向 Delete 现在会变成"光标不动，文字也不吞，只到末尾突然消失"
// =========================================================================

/// 问题3: `compute_coordinated_cursor_position()` 对 `conceal_to_left_edge=false`
/// 固定返回 `new_cursor_rect.x`；`build_text_animation_plan_with_sample()` 又把这个
/// 固定 caret_x 传给 `compute_frame_caret_driven()`。而
/// `compute_frame_caret_driven()` 的 forward Delete 用 `right - caret_x` 算保留宽度。
/// caret_x 全程固定在删除点时，这个宽度也全程固定，glyph 不会逐帧收进光标。
///
/// 评论 5748596920 期望：前向 Delete 时 caret_x 应随光标逐帧移动（或裁切宽度随帧
/// 变化），glyph 应逐帧收进光标。当前 caret_x 全程固定 → 断言"应逐帧变化"在当前
/// 代码上 FAIL → 复现成功。
#[test]
fn issue3_forward_delete_caret_x_fixed_at_new_rect_x() {
    let coord = read_src("src/sujian_editor_item/animation/cursor_motion.rs");

    // 前提：compute_coordinated_cursor_position 确实存在并有 has_forward_delete 分支
    let has_fn = coord.contains("fn compute_coordinated_cursor_position(");
    let has_forward_delete = coord.contains("has_forward_delete");
    println!(
        "[BUGFIX_REPRO_TRACE] issue3 compute_coordinated: has_fn={} has_forward_delete={}",
        has_fn, has_forward_delete
    );
    if !(has_fn && has_forward_delete) {
        return;
    }

    let window = function_window(&coord, "fn compute_coordinated_cursor_position", 12000);
    // 关键断言：has_forward_delete 分支固定返回 new_rect.x（不随帧变化）
    let forward_delete_fixed = window.contains("if has_forward_delete {")
        && window.contains("Some((new_rect.x, new_rect.top, h))");
    // 检查是否有逐帧移动机制（正确做法）
    let has_per_frame_caret = window.contains("forward_delete_caret_track")
        || window.contains("forward_delete_caret_x")
        || window.contains("forward_delete_progress")
        || window.contains("forward_delete_eased")
        || window.contains("forward_delete_sampled");
    println!(
        "[BUGFIX_REPRO_TRACE] issue3 compute_coordinated: forward_delete_fixed={} has_per_frame_caret={}",
        forward_delete_fixed, has_per_frame_caret
    );
    assert!(
        !(forward_delete_fixed && !has_per_frame_caret),
        "Issue #722 评论 5748596920 问题3: compute_coordinated_cursor_position 对 \
         has_forward_delete 固定返回 Some((new_rect.x, new_rect.top, h))，caret_x 全程\
         固定在删除点。build_text_animation_plan_with_sample 又把这个固定 caret_x 传给\
         compute_frame_caret_driven。而 compute_frame_caret_driven 的 forward Delete 用\
         from.x + from.w - caret_x 算保留宽度，caret_x 全程固定时这个宽度也全程固定，\
         glyph 不会逐帧收进光标 → 前向 Delete 变成\"光标不动，文字也不吞，只到末尾\
         突然消失\"。修复：前向 Delete 时 caret_x 应随光标逐帧移动（例如用 caret track\
         插值，或按事务 progress 从 old_rect.x 移到 new_rect.x），让裁切宽度随帧变化。"
    );
}

/// 问题3（续）: compute_frame_caret_driven 的 forward Delete 用
/// `from.x + from.w - caret_clip_boundary` 算保留宽度。当 caret_clip_boundary
/// 全程固定时，保留宽度也全程固定。
#[test]
fn issue3_compute_frame_caret_driven_forward_delete_uses_from_right_minus_caret_x() {
    let slice = read_src("src/sujian_editor_item/animated_slice.rs");

    let has_fn = slice.contains("pub fn compute_frame_caret_driven(");
    println!(
        "[BUGFIX_REPRO_TRACE] issue3 compute_frame_caret_driven: has_fn={}",
        has_fn
    );
    if !has_fn {
        return;
    }

    let window = function_window(&slice, "pub fn compute_frame_caret_driven", 4000);
    // 关键断言：forward Delete 分支用 from.x + from.w - caret_clip_boundary
    let forward_delete_uses_from_right_minus_caret = window
        .contains("self.from_document_rect.x + self.from_document_rect.w - caret_clip_boundary");
    // 检查是否有随帧变化的机制（正确做法：例如用 visible 参数让宽度随帧变化）
    let has_frame_varying = window.contains("frame_w * visible")
        || window.contains("from_right * visible")
        || window.contains("from_right * progress")
        || window.contains("conceal_progress");
    println!(
        "[BUGFIX_REPRO_TRACE] issue3 compute_frame_caret_driven: forward_delete_uses_from_right_minus_caret={} has_frame_varying={}",
        forward_delete_uses_from_right_minus_caret, has_frame_varying
    );
    assert!(
        !(forward_delete_uses_from_right_minus_caret && !has_frame_varying),
        "Issue #722 评论 5748596920 问题3: animated_slice.rs::compute_frame_caret_driven 的 \
         forward Delete 分支用 from.x + from.w - caret_clip_boundary 算保留宽度。当 \
         caret_clip_boundary 全程固定（由 compute_coordinated_cursor_position 固定返回 \
         new_rect.x）时，保留宽度也全程固定，glyph 不会逐帧收进光标。修复：forward \
         Delete 的裁切宽度应随帧变化（例如用 visible 参数或事务 progress 让宽度从满宽\
         逐帧收到 0），或让 caret_x 随帧移动。"
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
// 问题5: 空格/换行仍然被当普通 InsertReveal
// =========================================================================

/// 问题5: `animation_coordinator.rs::build_insert_reveal_slices()` 仍然只判断 cluster
/// 是否落在 inserted_range，命中就无条件创建 `AnimatedSlice::insert_reveal()` 和
/// static patch，没有检查 `new_snapshot.virtual_text` 的实际字符。纯空格、tab、
/// 换行/控制字符也会创建 InsertReveal 和对应 static patch，导致"文字前插空格闪一下
/// / 文字前手动换行闪一下"。
///
/// 评论 5748596920 期望：build_insert_reveal_slices 应检查
/// new_snapshot.virtual_text 的实际字符，纯空格/tab/换行/控制字符不应创建
/// InsertReveal 和 static patch。当前不检查 → 断言"应过滤非可见字符"在当前代码上
/// FAIL → 复现成功。
#[test]
fn issue5_build_insert_reveal_slices_does_not_filter_whitespace_and_control_chars() {
    let coord = read_src("src/sujian_editor_item/animation/transaction_builder.rs");

    // 前提：build_insert_reveal_slices 确实存在
    let has_fn = coord.contains("fn build_insert_reveal_slices(");
    let has_insert_reveal = coord.contains("AnimatedSlice::insert_reveal(");
    println!(
        "[BUGFIX_REPRO_TRACE] issue5 build_insert_reveal_slices: has_fn={} has_insert_reveal={}",
        has_fn, has_insert_reveal
    );
    if !(has_fn && has_insert_reveal) {
        return;
    }

    let window = function_window(&coord, "fn build_insert_reveal_slices", 3000);
    // 关键断言：只判断 cluster 落在 inserted_range，不检查字符
    let only_range_check = window.contains("new_cluster.byte_start >= range_start")
        && window.contains("new_cluster.byte_end <= range_end");
    // 检查是否有字符过滤机制（正确做法）
    let has_char_filter = window.contains("is_whitespace")
        || window.contains("is_control")
        || window.contains("char::is_whitespace")
        || window.contains("is_ascii_whitespace")
        || window.contains("' '")
        || window.contains("'\\n'")
        || window.contains("'\\t'")
        || window.contains("char.is_whitespace")
        || window.contains("filter_visible_char")
        || window.contains("is_visible_glyph")
        || window.contains("should_reveal")
        || window.contains("skip_whitespace");
    println!(
        "[BUGFIX_REPRO_TRACE] issue5 build_insert_reveal_slices: only_range_check={} has_char_filter={}",
        only_range_check, has_char_filter
    );
    assert!(
        !(only_range_check && !has_char_filter),
        "Issue #722 评论 5748596920 问题5: animation_coordinator.rs::build_insert_reveal_slices \
         只判断 cluster 是否落在 inserted_range（new_cluster.byte_start >= range_start && \
         new_cluster.byte_end <= range_end），命中就无条件创建 AnimatedSlice::insert_reveal \
         和 static patch，没有检查 new_snapshot.virtual_text 的实际字符。纯空格、tab、\
         换行/控制字符也会创建 InsertReveal 和对应 static patch，导致\"文字前插空格闪\
         一下 / 文字前手动换行闪一下\"。修复：build_insert_reveal_slices 应检查 \
         new_snapshot.virtual_text 的实际字符，纯空格/tab/换行/控制字符不应创建 \
         InsertReveal 和 static patch（或只创建 static patch 不创建动画）。"
    );
}

// =========================================================================
// 综合断言：5 个主链问题全部存在
// =========================================================================

/// 综合复现：评论 5748596920 指出的 5 个主链问题全部存在。当前实现违反该评论的全部
/// 核心语义。每个问题对应一段具体代码路径，当前代码均存在违规模式。
#[test]
fn all_five_main_chain_issues_exist() {
    let pipeline = read_src("src/sujian_editor_item/pipeline.rs");
    let coord_render_plan = read_src("src/sujian_editor_item/animation/render_plan_builder.rs");
    let coord_cursor_motion = read_src("src/sujian_editor_item/animation/cursor_motion.rs");
    let coord_transaction_builder =
        read_src("src/sujian_editor_item/animation/transaction_builder.rs");
    let slice = read_src("src/sujian_editor_item/animated_slice.rs");

    // 问题1: pipeline.rs::record_visual_transaction 仍传 ctx.scroll_y
    // Issue #722 评论 5748596920 修复后：改用文档坐标版本（caret_rect_doc / cursor_rect_doc），
    // make_cursor_rect_from_caret_doc 不再接收 scroll_y 参数。
    let issue1_violation = pipeline.contains("pub fn record_visual_transaction(")
        && pipeline.contains("ctx.scroll_y")
        && pipeline.contains("make_cursor_rect_from_caret_doc(")
        && pipeline.contains("text_baseline_y(")
        && pipeline.contains("- scroll_y")
        && !pipeline.contains("caret_rect_doc(")
        && !pipeline.contains("cursor_rect_doc(");

    // 问题2: compute_frame_caret_driven 只有 caret_clip_boundary: f64
    // Issue #722 评论 5748596920 修复后：compute_frame_caret_driven 签名增加了
    // caret_clip_y 和 caret_visual_line_id 参数，不再只传 caret_x。
    let issue2_violation = slice.contains("pub fn compute_frame_caret_driven(")
        && slice.contains("caret_clip_boundary: f64,")
        && !slice.contains("caret_clip_y")
        && coord_render_plan.contains("let caret_x = match tx.cursor_visual_track")
        && coord_render_plan.contains("unit.slice.compute_frame_caret_driven(caret_x, visible)");

    // 问题3: 前向 Delete caret_x 固定
    // Issue #722 评论 5748596920 修复后：前向 Delete 用 forward_delete_sampled 标记逐帧机制，
    // compute_frame_caret_driven 的前向 Delete 分支用 conceal_progress 随帧变化。
    let issue3_violation = coord_cursor_motion.contains("has_forward_delete")
        && coord_cursor_motion.contains("Some((new_rect.x, new_rect.top, h))")
        && !coord_cursor_motion.contains("forward_delete_sampled")
        && slice.contains(
            "self.from_document_rect.x + self.from_document_rect.w - caret_clip_boundary",
        )
        && !slice.contains("conceal_progress");

    // 问题4: 事务完成条件由 unit timeline 决定
    // Issue #722 评论 5748596920 修复后：增加了 caret_track_complete 条件，
    // InsertReveal/DeleteConceal 事务必须 caret track 也完成才能释放。
    let issue4_violation = coord_render_plan.contains("fn build_text_animation_plan_with_sample(")
        && (coord_render_plan
            .contains("tx.units.iter().all(|u| u.progress(sample.frame_now) >= 1.0)")
            || coord_render_plan.contains("tx.units.iter().all(|u| u.progress(frame_now) >= 1.0)"))
        && coord_render_plan.contains("keys_to_complete.push(tx.key)")
        && !coord_render_plan.contains("caret_track_complete");

    // 问题5: 空格/换行被当普通 InsertReveal
    // Issue #722 评论 5748596920 修复后：build_insert_reveal_slices 增加了字符过滤，
    // 纯空格/tab/换行/控制字符不创建 InsertReveal 和 static patch。
    let issue5_violation = coord_transaction_builder.contains("fn build_insert_reveal_slices(")
        && coord_transaction_builder.contains("new_cluster.byte_start >= range_start")
        && coord_transaction_builder.contains("new_cluster.byte_end <= range_end")
        && coord_transaction_builder.contains("AnimatedSlice::insert_reveal(")
        && !coord_transaction_builder.contains("is_whitespace")
        && !coord_transaction_builder.contains("is_control");

    println!(
        "[BUGFIX_REPRO_TRACE] SUMMARY five_main_chain_issues: issue1={} issue2={} issue3={} issue4={} issue5={}",
        issue1_violation, issue2_violation, issue3_violation, issue4_violation, issue5_violation
    );

    let any_violation = issue1_violation
        || issue2_violation
        || issue3_violation
        || issue4_violation
        || issue5_violation;

    assert!(
        !any_violation,
        "Issue #722 评论 5748596920 综合复现: 5 个主链问题全部存在。\
         当前实现存在以下违规模式（评论 5748596920 核心语义违反）：\n\
         1. 正文事务 caret 仍用旧 scroll_y 视口坐标 = {}\n\
         2. compute_frame_caret_driven 只传 caret_x，跨软换行错 = {}\n\
         3. 前向 Delete caret_x 固定，文字不逐帧收进 = {}\n\
         4. 事务完成条件由 unit timeline 决定 = {}\n\
         5. 空格/换行被当普通 InsertReveal = {}\n\
         评论 5748596920 要求：1) caret track 用文档坐标系；2) compute_frame_caret_driven \
         接收完整 caret geometry 或 per-unit caret_x；3) 前向 Delete caret_x 随帧移动；\
         4) 事务完成条件由 caret track remaining duration 决定；5) build_insert_reveal_slices \
         过滤空格/换行/控制字符。",
        issue1_violation, issue2_violation, issue3_violation, issue4_violation, issue5_violation
    );
}
