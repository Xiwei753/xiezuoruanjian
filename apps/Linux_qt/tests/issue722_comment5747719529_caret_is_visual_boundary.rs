//! Issue #722 的仍有效复现：光标文档坐标、滚动偏移和软换行边界。
//! 旧的“光标就是正文动画边界”模型已由 Issue #853 的独立正文与光标状态取代。

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
/// 用于精确检查某个分支体内的代码片段。安全处理 UTF-8 字符边界。
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
// 复现 A：last_scroll_y 字段只初始化为 0.0、无写回 → scroll_changed 永真
//         → hard_snap 永真 → 滚动后光标平滑动画失效
// =========================================================================

/// 复现 A：`cursor_controller.rs` 的 `last_scroll_y` 字段只初始化为 0.0，
/// 没有任何写回路径。`build_cursor_plan()` 里 `scroll_changed =
/// (old_scroll_y - scroll_y).abs() > 0.01`，由于 `old_scroll_y` 永远 = 0.0，
/// 只要页面滚动后 `scroll_y != 0.0`，`scroll_changed` 永真，`hard_snap` 永真，
/// 光标永远走 Snap，平滑动画失效。
///
/// 评论 1 第 1 点明确要求删掉 `last_scroll_y`。当前代码仍保留该字段且无写回
/// → 断言"字段应已删除"在当前代码上 FAIL → 复现成功。
#[test]
fn repro_a_last_scroll_y_field_never_written_back_breaks_scroll_animation() {
    let cursor_ctrl = read_src("src/sujian_editor_item/cursor_controller.rs");
    let rendering = read_src("src/sujian_editor_item/rendering.rs");
    let coord = read_src("src/sujian_editor_item/animation/render_plan_builder.rs");

    // 前提：last_scroll_y 字段确实存在且被传给 build_cursor_plan
    let field_exists = cursor_ctrl.contains("pub last_scroll_y: f64,");
    let init_zero = cursor_ctrl.contains("last_scroll_y: 0.0,");
    let passed_to_plan = rendering.contains("self.cursor_ctrl.last_scroll_y");
    let plan_has_old_scroll_y = coord.contains("old_scroll_y: f64,");
    let plan_has_scroll_changed = coord.contains("let scroll_changed =");
    println!(
        "[BUGFIX_REPRO_TRACE] A last_scroll_y: field_exists={} init_zero={} passed_to_plan={} plan_has_old_scroll_y={} scroll_changed={}",
        field_exists, init_zero, passed_to_plan, plan_has_old_scroll_y, plan_has_scroll_changed
    );
    // Issue #722 修复后回归守卫：违规模式已消除（字段已删除）时直接通过。
    // 未修复时前提满足，继续检查关键断言（has_writeback）。
    if !(field_exists
        && init_zero
        && passed_to_plan
        && plan_has_old_scroll_y
        && plan_has_scroll_changed)
    {
        return;
    }

    // 关键：last_scroll_y 没有任何写回（self.last_scroll_y = ... 赋值）。
    // 搜索整个 cursor_controller.rs 是否存在对 last_scroll_y 的赋值（排除声明和初始化）。
    let has_writeback = cursor_ctrl.lines().any(|line| {
        let trimmed = line.trim();
        // 排除字段声明 "pub last_scroll_y: f64," 和初始化 "last_scroll_y: 0.0,"
        (trimmed.contains("last_scroll_y")
            && trimmed.contains('=')
            && !trimmed.starts_with("pub last_scroll_y:")
            && !trimmed.starts_with("last_scroll_y:"))
            || trimmed.starts_with("self.last_scroll_y =")
    });
    println!(
        "[BUGFIX_REPRO_TRACE] A last_scroll_y has_writeback: {}",
        has_writeback
    );
    // 复现断言：last_scroll_y 应有写回（或字段应被删除）。当前既无写回也未删除
    // → FAIL → 复现。
    assert!(
        has_writeback,
        "Issue #722 评论 5747719529 复现 A: cursor_controller.rs 的 last_scroll_y 字段\
         只初始化为 0.0，没有任何写回路径。build_cursor_plan 里 scroll_changed = \
         (old_scroll_y - scroll_y).abs() > 0.01，由于 old_scroll_y 永远 = 0.0，\
         只要页面滚动后 scroll_y != 0.0，scroll_changed 永真，hard_snap 永真，\
         光标永远走 Snap，平滑动画失效（Issue 正文：页面滚动到章首完全离开视口\
         后光标平滑动画失效）。评论 1 第 1 点要求删掉 last_scroll_y。修复：删除\
         该字段及 build_cursor_plan 的 old_scroll_y/scroll_changed 参数，hard_snap\
         只保留 force_snap_next / is_scrolling / is_selecting / !old_visible。"
    );
}

// =========================================================================
// 复现 B：build_cursor_plan 的 hard_snap 仍含 scroll_changed
// =========================================================================

/// 复现 B：`build_cursor_plan()` 的 `hard_snap` 表达式仍包含 `scroll_changed`。
/// 评论 1 第 1 点明确要求 `hard_snap` 只保留 `force_snap_next / is_scrolling /
/// is_selecting / !old_visible`，删除 `scroll_changed`。当前代码仍含
/// `scroll_changed` → 断言"应删除 scroll_changed"在当前代码上 FAIL → 复现成功。
#[test]
fn repro_b_build_cursor_plan_hard_snap_includes_scroll_changed() {
    let src = read_src("src/sujian_editor_item/animation/render_plan_builder.rs");
    // 前提：hard_snap 表达式存在
    let has_hard_snap = src.contains("let hard_snap =");
    let has_scroll_changed_var = src.contains("let scroll_changed =");
    println!(
        "[BUGFIX_REPRO_TRACE] B hard_snap: has_hard_snap={} has_scroll_changed_var={}",
        has_hard_snap, has_scroll_changed_var
    );
    // Issue #722 修复后回归守卫：scroll_changed 已删除时直接通过。
    if !(has_hard_snap && has_scroll_changed_var) {
        return;
    }
    // 复现断言：hard_snap 表达式不应包含 scroll_changed。
    let hard_snap_window = window_after(&src, "let hard_snap =", 200);
    let hard_snap_includes_scroll_changed = hard_snap_window.contains("scroll_changed");
    println!(
        "[BUGFIX_REPRO_TRACE] B hard_snap_includes_scroll_changed: {}",
        hard_snap_includes_scroll_changed
    );
    assert!(
        !hard_snap_includes_scroll_changed,
        "Issue #722 评论 5747719529 复现 B: build_cursor_plan 的 hard_snap 表达式\
         仍包含 scroll_changed。评论 1 第 1 点明确要求删除 scroll_changed，\
         hard_snap 只保留 force_snap_next / is_scrolling / is_selecting / \
         !old_visible。当前 hard_snap = {:?}。滚动状态残留导致滚动后光标动画\
         失效（与复现 A 同源）。",
        hard_snap_window
    );
}

// =========================================================================
// 复现 F：cursor_x_from_canonical 用包含区间 find 取 canonical line
// =========================================================================

/// 复现 F：`cursor_x_from_canonical()`（layout.rs:3037）用
/// `para.lines.iter().find(|cl| cl.qchar_start <= cursor_qchar &&
/// cursor_qchar <= cl.qchar_end)` 在包含区间里找 canonical line。
///
/// 评论 1 第 3 点明确要求"软换行边界必须使用已经选中的那条 QTextLine（不要用
/// 包含区间 find）"。当光标正好在软换行边界时，`cursor_qchar` 同时满足上一条
/// line 的 `qchar_end` 和下一条 line 的 `qchar_start`，`find` 返回第一条 →
/// 取错行 → 光标落到下一行靠右/行尾（Issue 正文：正好输入到软换行边界时，
/// 文字已经进入下一行，光标却会落到下一行靠右/行尾）。
#[test]
fn repro_f_cursor_x_from_canonical_uses_inclusive_find_for_soft_wrap_boundary() {
    let src = read_src("src/editor/layout/canonical_snapshot.rs");
    let window = function_window(&src, "fn cursor_x_from_canonical", 1800);
    // 前提：函数确实用包含区间 find 取 canonical line
    let has_find = window.contains(".find(|cl| cl.qchar_start <= cursor_qchar");
    let has_inclusive = window.contains("cursor_qchar <= cl.qchar_end");
    println!(
        "[BUGFIX_REPRO_TRACE] F cursor_x_from_canonical: has_find={} has_inclusive={}",
        has_find, has_inclusive
    );
    // Issue #722 修复后回归守卫：包含区间 find 已删除时直接通过。
    if !(has_find && has_inclusive) {
        return;
    }
    // 复现断言：应改为使用已经选中的那条 QTextLine（如直接索引、或用 half-open
    // 区间、或用 visual_line_id 关联），而非包含区间 find。
    // 检查窗口内是否有"已选中的 QTextLine"直接索引机制。
    let markers = [
        "selected_line",
        "chosen_line",
        "visual_line_index",
        "line_index",
        "qtext_line_for",
        "canonical_line_for_visual",
        "line_for_cursor",
        "explicit_line",
        "indexed_line",
        "line_by_id",
    ];
    let has_explicit_line = markers.iter().any(|m| window.contains(m));
    println!(
        "[BUGFIX_REPRO_TRACE] F cursor_x_from_canonical has_explicit_line_index: {}",
        has_explicit_line
    );
    assert!(
        has_explicit_line,
        "Issue #722 评论 5747719529 复现 F: cursor_x_from_canonical 用包含区间 find\
         (cl.qchar_start <= cursor_qchar && cursor_qchar <= cl.qchar_end) 取 canonical line。\
         评论 1 第 3 点明确要求\"软换行边界必须使用已经选中的那条 QTextLine（不要用\
         包含区间 find）\"。当光标正好在软换行边界时，cursor_qchar 同时满足上一条\
         line 的 qchar_end 和下一条 line 的 qchar_start，find 返回第一条 → 取错行 →\
         光标落到下一行靠右/行尾（Issue 正文：正好输入到软换行边界时，文字已经\
         进入下一行，光标却会落到下一行靠右/行尾）。修复：用已经选中的那条\
         QTextLine（通过 visual_line_id / 显式索引关联），不要用包含区间 find。"
    );
}

// =========================================================================
// 复现 I：rendering.rs 仍把 last_scroll_y 传给 build_cursor_plan（链路未拆）
// =========================================================================

/// 复现 I：`rendering.rs::update_cursor_visual_position()` 仍把
/// `self.cursor_ctrl.last_scroll_y` 作为 `old_scroll_y` 传给
/// `build_cursor_plan()`。评论 1 第 1 点要求删除这条链路。当前仍存在
/// → 断言"链路应已删除"在当前代码上 FAIL → 复现成功。
#[test]
fn repro_i_rendering_still_passes_last_scroll_y_to_build_cursor_plan() {
    let src = read_src("src/sujian_editor_item/rendering.rs");
    // 前提：rendering 确实调用 build_cursor_plan
    let calls_plan = src.contains(".build_cursor_plan(");
    let passes_last_scroll_y = src.contains("self.cursor_ctrl.last_scroll_y");
    println!(
        "[BUGFIX_REPRO_TRACE] I rendering: calls_plan={} passes_last_scroll_y={}",
        calls_plan, passes_last_scroll_y
    );
    // Issue #722 修复后回归守卫：last_scroll_y 传参链路已删除时直接通过。
    if !(calls_plan && passes_last_scroll_y) {
        return;
    }
    // 复现断言：不应再把 last_scroll_y 传给 build_cursor_plan。
    // 评论 1 第 1 点要求删除这条链路。
    assert!(
        !passes_last_scroll_y,
        "Issue #722 评论 5747719529 复现 I: rendering.rs::update_cursor_visual_position 仍把\
         self.cursor_ctrl.last_scroll_y 作为 old_scroll_y 传给 build_cursor_plan。评论 1\
         第 1 点明确要求删除这条链路（rendering.rs：删除向 build_cursor_plan() 传\
         cursor_ctrl.last_scroll_y 的链路）。当前链路未拆，last_scroll_y 永远 = 0.0\
         传入，scroll_changed 永真，与复现 A/B 同源导致滚动后光标动画失效。"
    );
}
