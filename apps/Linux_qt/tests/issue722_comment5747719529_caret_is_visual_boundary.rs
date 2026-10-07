//! Issue #722 评论 5747719529 复现测试 — 光标就是吞字/吐字的视觉边界。
//!
//! 评论核心语义（Issue #826 评论 38 沿用并实现）：
//! - 吐字：光标往前走到哪里，文字就显示到哪里；已经被光标"带出来"的部分就是
//!   已经吐出来，不能后面再自己补一个淡入进度。
//! - 吞字：光标往回走到哪里，文字就消失到哪里；已经被光标扫过去的部分就是
//!   已经吞掉，不能还留着等另一个 unit timeline 再结束。
//! - 快速连续输入/删除时，新事务必须从当前这条视觉边界继续。上一帧光标已经
//!   扫过的部分保持最终状态，尚未扫过的部分继续跟着新的光标边界走。
//! - 文字不能再维护一套会和 caret 分叉的"自己什么时候完全出现/完全消失"的
//!   位置/可见度进度。真正决定当前 reveal/conceal 截止位置的是这一帧的
//!   caret 位置投影到 Frontier path 上的距离。
//!
//! Issue #826 评论 38：旧 cursor_motion / animated_slice 架构已删除，上面语义
//! 改由 `CoordinatedCaretMotion` + `project_onto_layer` + sample 携带的
//! `CoordinatedBoundary` 实现。指向旧文件的复现测试按评论 36「清理旧守卫」
//! 先例重写到新架构上；仍然有效的复现 A/B/F/I 原样保留。
//!
//! 行为级证明在 lib 内 `runtime_tests.rs` 的 `coordinated_*` 测试里。

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

// =========================================================================
// Issue #826 评论 38：光标就是吞吐边界 —— 新架构下的肯定式守卫
// =========================================================================
//
// 旧复现 C/D/E/G/H 指向已删除的 cursor_motion.rs / animated_slice.rs，
// 按评论 36「清理旧守卫」先例重写为肯定式断言（ below 6 条）。
// 行为级证明在 lib 内 runtime_tests.rs 的 coordinated_* 测试。

/// 吐字遮罩与吞字 overlay 的边界必须来自本帧 caret 投影，而不是文字自己的
/// visible fraction / 独立 timeline。
#[test]
fn caret_is_visual_boundary_of_frontier_masks() {
    let src = read_src("src/sujian_editor_item/animation/edit_frontier.rs");
    let mask_window = function_window(&src, "fn hidden_new_text_rects", 1500);
    assert!(
        mask_window.contains("reveal_distance(region, sample)"),
        "Issue #722/I#826-38: 吐字遮罩边界必须吃协同投影距离（caret 投影），\
         不能再由文字自己的 visible_fraction 决定。"
    );
    let overlay_window = function_window(&src, "fn old_overlay_glyphs", 1500);
    assert!(
        overlay_window.contains("conceal_distance(region, sample)"),
        "Issue #722/I#826-38: 吞字 overlay 边界必须吃协同投影距离。"
    );
    let sample_window = function_window(&src, "pub(crate) struct EditFrontierSample", 1200);
    assert!(
        sample_window.contains("coordinated: Option<CoordinatedBoundary>"),
        "Issue #722/I#826-38: 前沿 sample 必须携带本帧 caret 投影边界，\
         遮罩与 overlay 同帧看到同一个边界。"
    );
}

/// 投影只能从 caret 位置算出边界，绝不能反过来从 glyph 切片反推光标
/// （评论 3 明确禁止的反方向）。
#[test]
fn no_glyph_inference_in_coordinated_projection() {
    let src = read_src("src/sujian_editor_item/animation/coordinated_caret.rs");
    let window = function_window(&src, "pub(crate) fn project_onto_layer", 2500);
    assert!(
        window.contains("x: f64,") && window.contains("y: f64,"),
        "Issue #722/I#826-38: 投影输入必须是本帧 caret 的 (x, y)。"
    );
    for forbidden in ["rightmost", "conceal_edge", "rightmost_x", "clip_from_glyph"] {
        assert!(
            !src.contains(forbidden),
            "Issue #722/I#826-38: 协同投影里不得出现 {} —— 那是从文字反推光标，方向反了。",
            forbidden
        );
    }
}

/// 协同 caret 每帧只采样一次：motion 采样入口唯一，且渲染帧里先采协同、
/// 协同接管的帧不再推进独立光标 timeline。
#[test]
fn coordinated_caret_single_sample_per_frame() {
    let coord = read_src("src/sujian_editor_item/animation/coordinator.rs");
    assert!(
        coord.contains("pub(crate) fn sample_coordinated_caret"),
        "Issue #722/I#826-38: 协同 caret 必须有统一采样入口 sample_coordinated_caret。"
    );
    let paint = read_src("src/sujian_editor_item/qquickitem_impl.rs");
    let window = function_window(&paint, "fn update_paint_node", 6000);
    assert!(
        window.contains("tick_coordinated_caret_with_time(frame_now)"),
        "Issue #722/I#826-38: Scene Graph 帧必须采样协同 caret。"
    );
    assert!(
        window.contains("if !coordinated_owned_this_frame"),
        "Issue #722/I#826-38: 协同接管的帧不得再推进独立光标 timeline \
         （同一帧 visual 只写一次，否则光标与边界分叉）。"
    );
}

/// 快速连续输入/删除交棒时，新 motion 必须从旧 motion 当前帧继续，
/// 不能退回逻辑旧 caret（否则上一帧光标扫过的部分会跳回去）。
#[test]
fn coordinated_handoff_continues_from_sampled_frame() {
    let src = read_src("src/sujian_editor_item/animation/coordinator.rs");
    let window = function_window(
        &src,
        "pub(crate) fn begin_or_retarget_coordinated_caret",
        4500,
    );
    assert!(
        window.contains("position_at_distance(")
            && window.contains("distance_at_progress(")
            && window.contains("sample_progress(now)"),
        "Issue #722/I#826-38/39: 交棒必须先采样旧 motion 当前帧的轨迹距离，不退回逻辑旧 caret。"
    );
    assert!(
        window.contains("frontier.started_at") && window.contains("frontier.duration_ms"),
        "Issue #722/I#826-38: 交棒后 motion 时钟必须与当前前沿一致（同一 progress）。"
    );
}

/// 协同关时独立路径必须原样保留：前沿走 typing timeline，光标走 smooth tween。
/// 没有把所有模式硬绑在一起。
#[test]
fn coordinated_off_keeps_independent_paths() {
    let coord = read_src("src/sujian_editor_item/animation/coordinator.rs");
    assert!(
        coord.contains("pub(crate) fn build_cursor_plan"),
        "Issue #722/I#826-38: 非协同光标 Tween 的构造入口必须保留。"
    );
    let ctrl = read_src("src/sujian_editor_item/cursor_controller.rs");
    assert!(
        ctrl.contains("pub(crate) fn tick_animation"),
        "Issue #722/I#826-38: 独立光标 timeline 的每帧推进入口必须保留。"
    );
    let rendering = read_src("src/sujian_editor_item/rendering.rs");
    let window = function_window(
        &rendering,
        "pub(crate) fn update_cursor_visual_position",
        6000,
    );
    assert!(
        window.contains("begin_or_retarget_coordinated_caret")
            && window.contains(".build_cursor_plan("),
        "Issue #722/I#826-38: 光标更新必须同时保留协同接管与独立 Tween 两支 \
         （协同关时走独立分支）。"
    );
}

/// 两侧边界都由本帧 caret 位置投影得到，不再按来源侧直写 shared distance
///（glyph 矩形与 caret 矩形定位基准差几个像素，直写会在终点跳变）。
#[test]
fn coordinated_boundary_projected_from_caret_position() {
    let src = read_src("src/sujian_editor_item/animation/coordinator.rs");
    let window = function_window(&src, "pub(crate) fn sample_edit_frontier", 2500);
    assert!(
        window.contains("motion.sample_at_distance(motion.distance_at_progress(progress))")
            && window.contains("frontier_distance"),
        "Issue #722/I#826-39/41: 边界必须吃本帧 caret 所属文字段的前沿距离 \
         （Connector 段冻结），不能只共享 progress。"
    );
    assert!(
        window.contains("project_onto_layer(&frontier.reveal.regions")
            || window.contains("project_onto_layer(&frontier.conceal.regions"),
        "Issue #722/I#826-39: 非来源侧（Replace 两边）仍由 caret 位置投影。"
    );
}

/// 同行多 region 投影必须选 x 命中的段（多 patch / Replace / IME batch 一笔
/// 多 island 不是理论死角）。
#[test]
fn coordinated_projection_prefers_x_containing_segment() {
    let src = read_src("src/sujian_editor_item/animation/coordinated_caret.rs");
    let window = function_window(&src, "pub(crate) fn project_onto_layer", 3000);
    assert!(
        window.contains("nearest"),
        "Issue #722/I#826-39: 同行多 region 先收齐候选、优先 x 命中，\
         绝不能 first-y-match 就返回（region B 的 caret 会投到 region A 末端）。"
    );
}

/// 零可见 path 时 motion 比前沿活得长：tick 不得清 motion，续帧与 blink 都要
/// 跟着 motion 走，否则 Enter 后光标只动一帧就停。
#[test]
fn coordinated_motion_outlives_empty_frontier() {
    let coord = read_src("src/sujian_editor_item/animation/coordinator.rs");
    let tick_window = function_window(&coord, "pub(crate) fn tick(", 2000);
    assert!(
        !tick_window.contains("active_coordinated_caret = None"),
        "Issue #722/I#826-39: tick 不得因前沿没了就清 motion。"
    );
    let paint = read_src("src/sujian_editor_item/qquickitem_impl.rs");
    assert!(
        paint.contains("has_active_coordinated_caret()"),
        "Issue #722/I#826-39: 尾部续帧条件必须包含协同 motion。"
    );
}
