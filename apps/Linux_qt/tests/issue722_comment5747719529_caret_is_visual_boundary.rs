//! Issue #722 评论 5747719529 复现测试 — 光标不是吞字/吐字的视觉边界，
//! 而是被文字 glyph 切片反推出来，导致协同动画闪烁、软换行光标落点错误、
//! 滚动后光标动画失效。
//!
//! 本测试为 WHITE_BOX 结构守卫复现：验证当前实现违反评论 5747719529 的核心
//! 语义——"光标本身就是吞字/吐字的视觉边界"。每个子测试断言评论期望的正确
//! 结构，当前（未修复）代码违反这些断言 → 测试 FAIL → 缺陷复现成功。
//!
//! ## 评论 5747719529 核心语义
//!
//! - 吐字：光标往前走到哪里，文字就显示到哪里；已经被光标"带出来"的部分就是
//!   已经吐出来，不能后面再自己补一个淡入进度。
//! - 吞字：光标往回走到哪里，文字就消失到哪里；已经被光标扫过去的部分就是
//!   已经吞掉，不能还留着等另一个 unit timeline 再结束。
//! - 快速连续输入/删除时，新事务必须从当前这条视觉边界继续。上一帧光标已经
//!   扫过的部分保持最终状态，尚未扫过的部分继续跟着新的光标边界走。
//! - 文字不能再维护一套会和 caret 分叉的"自己什么时候完全出现/完全消失"的
//!   位置/可见度进度。真正决定当前 reveal/conceal 截止位置的是这一帧的
//!   caret geometry。
//! - 实现上，`InsertReveal` / `DeleteConceal` 的裁切边界应直接消费本帧
//!   coordinated caret 的位置；caret 与文字使用同一个 `frame_now` 和同一个
//!   from→to 几何轨迹。快速 rebase 时先采样当前 caret 边界，再把这个边界
//!   作为下一段动画起点。不要再用 `rightmost_x.max()`、`conceal_edge.min()`
//!   或独立 glyph progress 去反推出光标。
//!
//! ## 复现的违规模式（对应 Issue 正文 + 评论 1 改法 + 评论 3 语义纠正）
//!
//! 1. `last_scroll_y` 字段只初始化为 0.0、无写回，`scroll_changed` 永真，
//!    `hard_snap` 永真 → 滚动后光标平滑动画失效。
//! 2. `build_cursor_plan()` 的 `hard_snap` 仍含 `scroll_changed`，滚动状态
//!    残不干净。
//! 3. `compute_coordinated_cursor_position()` / `sample_coordinated_cursor_rect_at()`
//!    用 `rightmost_x.max()` / `conceal_edge.min()` 从文字 glyph 切片反推
//!    光标位置——正是评论 3 明确禁止的反方向。
//! 4. `cursor_x_from_canonical()` 用包含区间 `find` 取 canonical line，软换行
//!    边界取错行 → 光标落到下一行靠右/行尾。
//! 5. `InsertReveal` / `DeleteConceal` 的 `compute_frame` 裁切宽度由 unit 自己
//!    的 `visible_fraction` 决定，不消费本帧 coordinated caret 的位置。
//! 6. 文字 unit 维护独立 timeline / visible fraction，会与 caret 分叉。

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

/// 检查窗口内是否出现"caret 驱动裁切边界"的正确结构标识符。
///
/// 评论 5747719529 期望：先采样本帧 coordinated caret 的位置，再把该位置作为
/// InsertReveal/DeleteConceal 的裁切边界。这里列举一组合理的命名候选，只要
/// 窗口内出现任一即视为"已改为 caret 驱动裁切"。
fn has_caret_driven_clip_guard(window: &str) -> bool {
    let markers = [
        "caret_clip_boundary",
        "caret_boundary",
        "caret_edge",
        "coordinated_caret_x",
        "caret_reveal_edge",
        "caret_conceal_edge",
        "clip_from_caret",
        "reveal_boundary_from_caret",
        "conceal_boundary_from_caret",
        "caret_driven_clip",
        "caret_visual_boundary",
        "consume_caret",
        "caret_geometry_clip",
        "clip_width_from_caret",
        "reveal_from_caret",
        "conceal_from_caret",
    ];
    markers.iter().any(|m| window.contains(m))
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
// 复现 C：compute_coordinated_cursor_position 用 rightmost_x.max() 反推光标
// =========================================================================

/// 复现 C：`compute_coordinated_cursor_position()`（animation_coordinator.rs:2678）
/// 在 Insert 分支遍历 `tx.units`，对每个 `InsertReveal` unit 计算
/// `edge_x = frame.x + frame.w`，再 `rightmost_x = Some(prev.max(edge_x))`，
/// 最后 `match rightmost_x { Some(x) => Some((x, cursor_y, h)), ... }`。
///
/// 这正是评论 5747719529 明确禁止的"用 `rightmost_x.max()` 或独立 glyph progress
/// 去反推出光标"。正确做法是先采样本帧 coordinated caret 的位置，再把该位置
/// 作为 InsertReveal 的裁切边界。当前代码反方向 → 断言"不应有 rightmost_x.max()
/// 反推"在当前代码上 FAIL → 复现成功。
#[test]
fn repro_c_compute_coordinated_cursor_uses_rightmost_x_max_to_infer_cursor() {
    let src = read_src("src/sujian_editor_item/animation/cursor_motion.rs");
    // compute_coordinated_cursor_position 函数体较大（Insert 分支 ~2736 行，
    // Delete 分支 ~2776 行），需足够大窗口覆盖两个分支。
    let window = function_window(&src, "fn compute_coordinated_cursor_position", 14000);
    // 前提：函数确实有 Insert 分支遍历 units 算 rightmost_x
    let has_rightmost_x = window.contains("let mut rightmost_x: Option<f64> = None;");
    let has_max_edge = window.contains("prev.max(edge_x)");
    let has_insert_reveal_filter = window.contains("AnimatedSliceKind::InsertReveal");
    println!(
        "[BUGFIX_REPRO_TRACE] C compute_coordinated: has_rightmost_x={} has_max_edge={} insert_reveal_filter={}",
        has_rightmost_x, has_max_edge, has_insert_reveal_filter
    );
    // Issue #722 修复后回归守卫：rightmost_x.max() 已删除时直接通过。
    if !(has_rightmost_x && has_max_edge && has_insert_reveal_filter) {
        return;
    }
    // 复现断言：不应从 glyph 切片反推光标，应改为 caret 驱动裁切。
    let has_caret_driven = has_caret_driven_clip_guard(&window);
    println!(
        "[BUGFIX_REPRO_TRACE] C compute_coordinated has_caret_driven_clip: {}",
        has_caret_driven
    );
    assert!(
        has_caret_driven,
        "Issue #722 评论 5747719529 复现 C: compute_coordinated_cursor_position 在 Insert 分支\
         用 rightmost_x.max() 从文字 glyph 切片反推光标位置（edge_x = frame.x + frame.w，\
         rightmost_x = prev.max(edge_x)）。评论 5747719529 明确禁止：\"不要再用\
         rightmost_x.max()、conceal_edge.min() 或独立 glyph progress 去反推出光标\"。\
         正确做法：先采样本帧 coordinated caret 的位置（caret track / old-new rect 插值），\
         再把该位置作为 InsertReveal 的裁切边界。当前反方向导致吐字时文字自己维护一套\
         reveal 进度，光标被文字反推，快速连续输入时两者分叉 → 闪烁。"
    );
}

// =========================================================================
// 复现 D：compute_coordinated_cursor_position 用 conceal_edge.min() 反推光标
// =========================================================================

/// 复现 D：`compute_coordinated_cursor_position()` 在 Delete 分支遍历 `tx.units`，
/// 对每个 `DeleteConceal` unit（conceal_to_left_edge=true）计算
/// `edge = frame.x + frame.w`，再 `conceal_edge = Some(prev.min(edge))`，
/// 最后 `if let Some(x) = conceal_edge { Some((x, cursor_y, h)) }`。
///
/// 这正是评论 5747719529 明确禁止的"用 `conceal_edge.min()` 反推光标"。当前代码
/// → 断言"不应有 conceal_edge.min() 反推"在当前代码上 FAIL → 复现成功。
#[test]
fn repro_d_compute_coordinated_cursor_uses_conceal_edge_min_to_infer_cursor() {
    let src = read_src("src/sujian_editor_item/animation/cursor_motion.rs");
    let window = function_window(&src, "fn compute_coordinated_cursor_position", 14000);
    // 前提：函数确实有 Delete 分支遍历 units 算 conceal_edge
    let has_conceal_edge = window.contains("let mut conceal_edge: Option<f64> = None;");
    let has_min_edge = window.contains("prev.min(edge)");
    let has_delete_conceal_filter = window.contains("AnimatedSliceKind::DeleteConceal");
    println!(
        "[BUGFIX_REPRO_TRACE] D compute_coordinated: has_conceal_edge={} has_min_edge={} delete_conceal_filter={}",
        has_conceal_edge, has_min_edge, has_delete_conceal_filter
    );
    // Issue #722 修复后回归守卫：conceal_edge.min() 已删除时直接通过。
    if !(has_conceal_edge && has_min_edge && has_delete_conceal_filter) {
        return;
    }
    // 复现断言：不应从 glyph 切片反推光标，应改为 caret 驱动裁切。
    let has_caret_driven = has_caret_driven_clip_guard(&window);
    println!(
        "[BUGFIX_REPRO_TRACE] D compute_coordinated has_caret_driven_clip: {}",
        has_caret_driven
    );
    assert!(
        has_caret_driven,
        "Issue #722 评论 5747719529 复现 D: compute_coordinated_cursor_position 在 Delete 分支\
         用 conceal_edge.min() 从文字 glyph 切片反推光标位置（edge = frame.x + frame.w，\
         conceal_edge = prev.min(edge)）。评论 5747719529 明确禁止：\"不要再用\
         conceal_edge.min() 或独立 glyph progress 去反推出光标\"。吞字时应由 caret\
         往回走到哪里决定文字消失到哪里，当前反方向导致吞字时文字自己维护一套\
         conceal 进度，光标被文字反推，快速连续删除时两者分叉 → 闪烁。"
    );
}

// =========================================================================
// 复现 E：sample_coordinated_cursor_rect_at 同样用 glyph 反推光标
// =========================================================================

/// 复现 E：`sample_coordinated_cursor_rect_at()`（animation_coordinator.rs:300）
/// 与 `compute_coordinated_cursor_position` 同源，同样在 Insert 分支用
/// `rightmost_x.max()`、Delete 分支用 `conceal_edge.min()` 从 glyph 反推光标。
/// 这是另一条调用路径上的同源违规。
#[test]
fn repro_e_sample_coordinated_cursor_rect_at_uses_glyph_inference() {
    let src = read_src("src/sujian_editor_item/animation/cursor_motion.rs");
    // sample_coordinated_cursor_rect_at 从第 300 行起，Insert 分支 ~331 行，
    // Delete 分支 ~369 行，需足够大窗口覆盖两个分支。
    let window = function_window(&src, "fn sample_coordinated_cursor_rect_at", 9000);
    // 前提：函数确实用 glyph 反推
    let has_rightmost_x = window.contains("let mut rightmost_x: Option<f64> = None;");
    let has_conceal_edge = window.contains("let mut conceal_edge: Option<f64> = None;");
    let has_max = window.contains("prev.max(edge_x)");
    let has_min = window.contains("prev.min(edge)");
    println!(
        "[BUGFIX_REPRO_TRACE] E sample_coordinated: has_rightmost_x={} has_conceal_edge={} has_max={} has_min={}",
        has_rightmost_x, has_conceal_edge, has_max, has_min
    );
    // Issue #722 修复后回归守卫：glyph 反推已删除时直接通过。
    if !(has_rightmost_x && has_conceal_edge && has_max && has_min) {
        return;
    }
    // 复现断言：不应从 glyph 切片反推光标。
    let has_caret_driven = has_caret_driven_clip_guard(&window);
    println!(
        "[BUGFIX_REPRO_TRACE] E sample_coordinated has_caret_driven_clip: {}",
        has_caret_driven
    );
    assert!(
        has_caret_driven,
        "Issue #722 评论 5747719529 复现 E: sample_coordinated_cursor_rect_at 同样在 Insert 分支\
         用 rightmost_x.max()、Delete 分支用 conceal_edge.min() 从文字 glyph 切片反推光标。\
         这是 compute_coordinated_cursor_position 之外另一条调用路径上的同源违规，\
         会导致两条路径上光标都被文字反推，与 caret 分叉。"
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
// 复现 G：InsertReveal/DeleteConceal 裁切边界由 unit 自己的 visible fraction
//         决定，不消费本帧 coordinated caret 的位置
// =========================================================================

/// 复现 G：`animated_slice.rs::compute_frame()`（第 283 行）对 InsertReveal
/// 计算 `frame_w = to_document_rect.w * visible`，对 DeleteConceal 计算
/// `frame_w = from_document_rect.w * visible`。裁切宽度由 `visible`（unit 自己
/// 的 `current_visible_fraction`）决定，**不消费本帧 coordinated caret 的
/// 位置**。
///
/// 评论 5747719529："InsertReveal / DeleteConceal 的裁切边界应直接消费本帧
/// coordinated caret 的位置；caret 与文字使用同一个 frame_now 和同一个
/// from→to 几何轨迹。"当前裁切由 unit 自己的 visible fraction 驱动，光标再
/// 从 frame edge 反推 → 文字和光标各自一套进度 → 分叉 → 闪烁。
#[test]
fn repro_g_insert_reveal_delete_conceal_clip_independent_of_caret() {
    let src = read_src("src/sujian_editor_item/animated_slice.rs");
    let window = function_window(&src, "pub fn compute_frame", 1800);
    // 前提：compute_frame 确实由 visible 驱动裁切
    let insert_reveal_clip = window.contains("let frame_w = self.to_document_rect.w * visible;");
    let delete_conceal_clip = window.contains("let frame_w = self.from_document_rect.w * visible;");
    println!(
        "[BUGFIX_REPRO_TRACE] G compute_frame: insert_reveal_clip={} delete_conceal_clip={}",
        insert_reveal_clip, delete_conceal_clip
    );
    // Issue #722 修复后回归守卫：unit visible fraction 驱动裁切已删除时直接通过。
    if !(insert_reveal_clip && delete_conceal_clip) {
        return;
    }
    // 复现断言：裁切边界应消费 caret geometry，而非 unit 自己的 visible。
    let has_caret_driven = has_caret_driven_clip_guard(&window);
    println!(
        "[BUGFIX_REPRO_TRACE] G compute_frame has_caret_driven_clip: {}",
        has_caret_driven
    );
    assert!(
        has_caret_driven,
        "Issue #722 评论 5747719529 复现 G: animated_slice.rs::compute_frame 对 InsertReveal\
         计算 frame_w = to_document_rect.w * visible，对 DeleteConceal 计算 frame_w = \
         from_document_rect.w * visible。裁切宽度由 unit 自己的 current_visible_fraction\
         决定，不消费本帧 coordinated caret 的位置。评论 5747719529 明确要求：\"InsertReveal\
         / DeleteConceal 的裁切边界应直接消费本帧 coordinated caret 的位置；caret 与文字\
         使用同一个 frame_now 和同一个 from→to 几何轨迹\"。当前文字自己维护一套 reveal/conceal\
         进度，光标再从 frame edge 反推 → 文字和光标各自一套进度 → 分叉 → 快速输入/删除时闪烁。"
    );
}

// =========================================================================
// 复现 H：文字 unit 维护独立 timeline / visible fraction，会与 caret 分叉
// =========================================================================

/// 复现 H：文字 `PreparedVisualUnit` 拥有自己的 `started_at` / `duration_ms`，
/// 从自己的时间线计算 per-unit progress 和 `current_visible_fraction`。
/// 评论 5747719529："文字不能再维护一套会和 caret 分叉的'自己什么时候完全
/// 出现/完全消失'的位置/可见度进度。真正决定当前 reveal/conceal 截止位置的
/// 是这一帧的 caret geometry。"
///
/// 当前 `animation_coordinator.rs` 注释（第 2583 行）明确承认 unit"拥有自己的
/// `started_at` / `duration_ms`，从自己的时间线计算 per-unit progress"。
/// `animated_slice.rs` 注释（第 280 行）也承认"单元自己的时间线 +
/// `[start_fraction, target_fraction]` 视觉窗口"。这正是评论 3 禁止的独立
/// timeline。
#[test]
fn repro_h_text_unit_maintains_independent_timeline_that_forks_from_caret() {
    let coord = read_src("src/sujian_editor_item/animation/coordinator.rs");
    let slice = read_src("src/sujian_editor_item/animated_slice.rs");

    // 前提：文字 unit 确实维护独立 timeline
    let unit_has_own_timeline = coord.contains("拥有自己的 `started_at` / `duration_ms`")
        || coord.contains("per-unit progress");
    let slice_has_own_timeline =
        slice.contains("单元自己的时间线") || slice.contains("自己的时间线");
    let has_current_visible_fraction =
        slice.contains("current_visible_fraction") || coord.contains("current_visible_fraction");
    println!(
        "[BUGFIX_REPRO_TRACE] H independent_timeline: unit_has_own={} slice_has_own={} has_visible_fraction={}",
        unit_has_own_timeline, slice_has_own_timeline, has_current_visible_fraction
    );
    // Issue #722 修复后回归守卫：独立 timeline 注释已删除时直接通过。
    if !(unit_has_own_timeline || slice_has_own_timeline) {
        return;
    }

    // 复现断言：文字 unit 的 reveal/conceal 截止位置应由本帧 caret geometry 决定，
    // 不应维护独立 visible fraction timeline。检查是否已有"caret geometry 决定
    // reveal/conceal 截止"的机制。
    let caret_geometry_markers = [
        "caret_geometry_determines_clip",
        "reveal_until_caret",
        "conceal_until_caret",
        "clip_by_caret",
        "caret_bounds_clip",
        "caret_driven_reveal",
        "caret_driven_conceal",
        "no_independent_unit_timeline",
        "clip_from_coordinated_caret",
    ];
    let has_caret_geometry_clip = caret_geometry_markers
        .iter()
        .any(|m| coord.contains(m) || slice.contains(m));
    println!(
        "[BUGFIX_REPRO_TRACE] H has_caret_geometry_clip: {}",
        has_caret_geometry_clip
    );
    assert!(
        has_caret_geometry_clip,
        "Issue #722 评论 5747719529 复现 H: 文字 PreparedVisualUnit 拥有自己的 started_at /\
         duration_ms，从自己的时间线计算 per-unit progress 和 current_visible_fraction。\
         评论 5747719529 明确禁止：\"文字不能再维护一套会和 caret 分叉的'自己什么时候\
         完全出现/完全消失'的位置/可见度进度。真正决定当前 reveal/conceal 截止位置的\
         是这一帧的 caret geometry\"。当前文字 unit 自己跑独立 timeline，光标再从文字\
         frame edge 反推（复现 C/D/E），两套进度在快速连续输入/删除时 rebase 不一致 →\
         闪烁。修复：删除文字 unit 的独立 timeline，reveal/conceal 截止位置直接由本帧\
         coordinated caret geometry 决定，caret 与文字共用同一个 frame_now 和同一个\
         from→to 几何轨迹。"
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

// =========================================================================
// 综合断言：光标不是吞字/吐字的视觉边界（评论 5747719529 核心语义违反）
// =========================================================================

/// 综合复现：评论 5747719529 的核心语义是"光标本身就是吞字/吐字的视觉边界"。
/// 当前实现违反该语义：光标位置由文字 glyph 切片反推（rightmost_x.max() /
/// conceal_edge.min()），文字 unit 维护独立 timeline，裁切边界不消费 caret
/// geometry。这导致 Issue 正文描述的全部症状：
/// - 滚动后光标动画失效（复现 A/B/I）
/// - 快速输入/删除闪烁（复现 C/D/E/G/H）
/// - 软换行光标落点错误（复现 F）
#[test]
fn caret_is_not_visual_boundary_of_insert_reveal_delete_conceal() {
    let coord_render_plan = read_src("src/sujian_editor_item/animation/render_plan_builder.rs");
    let coord_cursor_motion = read_src("src/sujian_editor_item/animation/cursor_motion.rs");
    let coord_types = read_src("src/sujian_editor_item/animation/coordinator.rs");
    let slice = read_src("src/sujian_editor_item/animated_slice.rs");
    let layout = read_src("src/editor/layout/canonical_snapshot.rs");
    let cursor_ctrl = read_src("src/sujian_editor_item/cursor_controller.rs");
    let rendering = read_src("src/sujian_editor_item/rendering.rs");

    // 汇总所有违规模式的存在性
    let violation_a_last_scroll_y = cursor_ctrl.contains("pub last_scroll_y: f64,")
        && rendering.contains("self.cursor_ctrl.last_scroll_y");
    let violation_b_scroll_changed_in_hard_snap = {
        let w = window_after(&coord_render_plan, "let hard_snap =", 200);
        w.contains("scroll_changed")
    };
    let violation_c_rightmost_x_max = coord_cursor_motion.contains("prev.max(edge_x)");
    let violation_d_conceal_edge_min = coord_cursor_motion.contains("prev.min(edge)");
    let violation_f_inclusive_find = layout
        .contains(".find(|cl| cl.qchar_start <= cursor_qchar && cursor_qchar <= cl.qchar_end)");
    let violation_g_unit_visible_clip =
        slice.contains("let frame_w = self.to_document_rect.w * visible;");
    let has_caret_geometry_clip = coord_types.contains("caret_geometry_determines_clip")
        || coord_types.contains("clip_from_coordinated_caret");
    let violation_h_unit_own_timeline = (coord_types.contains("per-unit progress")
        || slice.contains("单元自己的时间线")
        || coord_types.contains("拥有自己的 `started_at` / `duration_ms`"))
        && !has_caret_geometry_clip;

    println!(
        "[BUGFIX_REPRO_TRACE] SUMMARY violations: A={} B={} C={} D={} F={} G={} H={}",
        violation_a_last_scroll_y,
        violation_b_scroll_changed_in_hard_snap,
        violation_c_rightmost_x_max,
        violation_d_conceal_edge_min,
        violation_f_inclusive_find,
        violation_g_unit_visible_clip,
        violation_h_unit_own_timeline
    );

    let any_violation = violation_a_last_scroll_y
        || violation_b_scroll_changed_in_hard_snap
        || violation_c_rightmost_x_max
        || violation_d_conceal_edge_min
        || violation_f_inclusive_find
        || violation_g_unit_visible_clip
        || violation_h_unit_own_timeline;

    assert!(
        !any_violation,
        "Issue #722 评论 5747719529 综合复现: 光标不是吞字/吐字的视觉边界。\
         当前实现存在以下违规模式（评论 5747719529 核心语义违反）：\n\
         A. last_scroll_y 字段无写回 + 仍传给 build_cursor_plan = {}\n\
         B. hard_snap 仍含 scroll_changed = {}\n\
         C. compute_coordinated_cursor_position 用 rightmost_x.max() 反推光标 = {}\n\
         D. compute_coordinated_cursor_position 用 conceal_edge.min() 反推光标 = {}\n\
         F. cursor_x_from_canonical 用包含区间 find 取 canonical line = {}\n\
         G. InsertReveal/DeleteConceal 裁切由 unit visible fraction 驱动 = {}\n\
         H. 文字 unit 维护独立 timeline = {}\n\
         评论 5747719529 要求：光标本身就是吞字/吐字的视觉边界；InsertReveal/DeleteConceal\
         的裁切边界应直接消费本帧 coordinated caret 的位置；caret 与文字使用同一个\
         frame_now 和同一个 from→to 几何轨迹；不要再用 rightmost_x.max()、\
         conceal_edge.min() 或独立 glyph progress 去反推出光标。",
        violation_a_last_scroll_y,
        violation_b_scroll_changed_in_hard_snap,
        violation_c_rightmost_x_max,
        violation_d_conceal_edge_min,
        violation_f_inclusive_find,
        violation_g_unit_visible_clip,
        violation_h_unit_own_timeline
    );
}
