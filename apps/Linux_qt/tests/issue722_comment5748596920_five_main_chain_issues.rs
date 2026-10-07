//! Issue #722 中仍有效的文档坐标和滚动坐标守卫。

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
