//! Issue #815 评论 6042062633 结构守卫 — 吞字/吐字真正由光标驱动的协同动画。
//!
//! WHITE_BOX 验证策略：确定性断言"协同动画退化成两条互不相干的时间线"的缺陷模式
//! 已在代码中消除，而不是靠肉眼回归。
//!
//! 一句话定义（评审收口）：
//! `协同模式 = 一条 caret 运动轨迹 + 文字以 caret 当前帧为吞吐边界 + Reflow 可独立`
//! — 不是 `两条互不相干的时间线，只在起点位置看起来碰巧挨着`。
//!
//! 逐条覆盖评审的九个修改点：
//! 1. `qml/WritingWorkspace.qml` 恢复鼠标左键长按选词；
//! 2. `transaction/timeline.rs` 单元计时分成 `Timed` / `CaretTrack` 两种驱动；
//! 3. `animation/cursor_motion.rs` 光标 track 是唯一采样入口，文字与光标共用；
//! 4. `animation/transaction_builder.rs` 先建 track 再包 unit，协同吞吐字不建独立进度；
//! 5. `animated_slice.rs` 逐帧裁切由当前 caret 帧算出；
//! 6. `animation/render_plan_builder.rs` 每帧每事务只采样一次，完成条件两类分开；
//! 7. `animation/rebase.rs` 不再静默跳过，报正式事件；
//! 8. `editing.rs` / `transaction.rs` / `pipeline.rs` 闭环所有"编辑发生了但没动画"的原因；
//! 9. `animation/composition.rs` IME commit 与普通 Insert 同一规则。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/source_guard.rs"]
mod source_guard;

use source_guard::{function_window, read_src};

/// 评审收口定义。代码注释里必须留着这句，避免以后又被改回"两条独立时间线"。
const CLOSING_DEFINITION: &str =
    "一条 caret 运动轨迹 + 文字以 caret 当前帧为吞吐边界 + Reflow 可独立";

// =========================================================================
// 修改点 1：恢复桌面鼠标左键长按选词
// =========================================================================

#[test]
fn issue815_modify1_qml_restores_mouse_left_long_press() {
    let src = read_src("qml/WritingWorkspace.qml");

    // Issue #819 评论 5956495850 第 6/7 节：旧 touchLongPressHandler 已删除，
    // 改用 TapHandler + Timer 调 activate_pointer_long_press。
    // 本测试验证新实现仍保留 Issue #815 评论 6042062633 修改 1 的语义：
    // 左键长按选词恢复、所有设备接受、右键菜单独立 handler。

    // 旧 touchLongPressHandler 必须已删除。
    assert!(
        !src.contains("id: touchLongPressHandler"),
        "Issue #819 评论 5956495850 第 6 节: 旧 touchLongPressHandler 必须删除，\
         改用 TapHandler + Timer 调 activate_pointer_long_press。"
    );

    // 新 leftButtonLongPressHandler 必须存在。
    let handler_pos = src
        .find("id: leftButtonLongPressHandler")
        .expect("Issue #819 评论 5956495850 第 6 节: WritingWorkspace.qml 必须有新的左键长按 handler");
    let window_start = src[..handler_pos].rfind("TapHandler {").unwrap_or(0);
    // 取到下一个 TapHandler 之前，即本 handler 的完整范围（按字符边界回退）。
    let window_end = src[handler_pos..]
        .find("TapHandler {")
        .map(|i| handler_pos + i)
        .unwrap_or(src.len());
    let window = src[window_start..window_end].to_string();

    assert!(
        window.contains("acceptedButtons: Qt.LeftButton"),
        "Issue #815 评论 6042062633 修改 1: 长按 handler 仍必须只吃左键，右键菜单走独立 handler。"
    );
    for device in [
        "PointerDevice.Mouse",
        "PointerDevice.TouchPad",
        "PointerDevice.TouchScreen",
        "PointerDevice.Stylus",
    ] {
        assert!(
            window.contains(device),
            "Issue #815 评论 6042062633 修改 1: 长按 handler 必须接受 {}，\
             Issue #714 把鼠标排除在外是错的（左键长按与右键菜单是两个不同输入）。",
            device
        );
    }

    // Issue #819 评论 5956495850 第 6/7 节：新链路用 Timer 调
    // activate_pointer_long_press（Rust 状态机 activate_long_press + long_press_at）。
    assert!(
        window.contains("activate_pointer_long_press"),
        "Issue #819 评论 5956495850 第 6 节: 新长按 handler 必须通过 Timer 调\
         activate_pointer_long_press。"
    );
    assert!(
        window.contains("Timer"),
        "Issue #819 评论 5956495850 第 6 节: 新长按 handler 必须用 Timer 触发长按。"
    );

    // end_selection_gesture_qml 仍必须在 release/cancel 时调用。
    assert!(
        window.contains("end_selection_gesture_qml"),
        "Issue #815 评论 6042062633 修改 1: 长按释放/取消时仍必须调\
         end_selection_gesture_qml 结束选择手势。"
    );

    // 右键菜单继续由独立的 Qt.RightButton TapHandler 处理。
    assert!(
        src.contains("acceptedButtons: Qt.RightButton"),
        "Issue #815 评论 6042062633 修改 1: 右键菜单必须继续走独立的 Qt.RightButton handler。"
    );

    // 删掉 Issue #714 的错误论断注释。
    assert!(
        !src.contains("桌面鼠标的长按等同于右键菜单"),
        "Issue #815 评论 6042062633 修改 1: 必须删除「桌面鼠标长按 == 右键菜单」的注释/逻辑，\
         它正是当初把鼠标排除掉的错误理由。"
    );
}

// =========================================================================
// 修改点 2：视觉单元计时分成 Timed / CaretTrack 两种驱动
// =========================================================================

#[test]
fn issue815_modify2_visual_unit_timing_has_two_drivers() {
    let src = read_src("src/sujian_editor_item/animation/transaction/timeline.rs");

    let enum_window = function_window(&src, "pub(crate) enum VisualUnitTiming", 1200);
    assert!(
        enum_window.contains("Timed {"),
        "Issue #815 评论 6042062633 修改 2: VisualUnitTiming 必须保留 Timed 驱动\
         （ReflowMove/ReflowCrossFade + 非协同吞吐字）。"
    );
    assert!(
        enum_window.contains("CaretTrack { retired: bool }"),
        "Issue #815 评论 6042062633 修改 2: VisualUnitTiming 必须有 CaretTrack 驱动\
         （协同 InsertReveal/DeleteConceal）。"
    );

    // 协同吞吐字不能自己再拥有一条 ease_out_quad + text_duration_ms 时间线。
    let ctor_window = function_window(&src, "pub(crate) fn default_for_kind_with_coordinated", 900);
    assert!(
        ctor_window.contains("coordinated")
            && ctor_window.contains("AnimatedSliceKind::InsertReveal | AnimatedSliceKind::DeleteConceal"),
        "Issue #815 评论 6042062633 修改 2: 只有协同模式的 InsertReveal/DeleteConceal 才切 CaretTrack，\
         Reflow 必须继续独立 Timed。"
    );
    assert!(
        !ctor_window.contains("ease_out_quad") && !ctor_window.contains("started_at"),
        "Issue #815 评论 6042062633 修改 2: 协同吞吐字的驱动构造里不得再出现自己的 easing/时间线。"
    );

    // retire 语义：失去 caret ownership 后 CaretTrack 单元收口到终态。
    assert!(
        src.contains("pub(crate) fn retire_caret_motion"),
        "Issue #815 评论 6042062633 修改 3: CaretTrack 单元必须有 retire 收口语义，\
         否则事务会等一条已经不推进的 track。"
    );
    assert!(
        src.contains("pub(crate) fn is_caret_track"),
        "Issue #815 评论 6042062633 修改 2/6: 必须能区分 CaretTrack 单元（含已 retire 的）。"
    );
}

// =========================================================================
// 修改点 3：cursor track 是协同运动的唯一采样入口
// =========================================================================

#[test]
fn issue815_modify3_cursor_track_is_the_single_sampling_entry() {
    let src = read_src("src/sujian_editor_item/animation/cursor_motion.rs");

    assert!(
        src.contains("pub(crate) fn sample_caret_track_frame"),
        "Issue #815 评论 6042062633 修改 3: 光标 track 必须有统一采样入口 sample_caret_track_frame，\
         同一个 frame_now 一次给出 caret rect / x / y / visual_line_id / progress。"
    );

    let sample_window = function_window(&src, "pub(crate) fn sample_caret_track_frame", 1500);
    for marker in [
        "sampled_rect_at_progress",
        "sampled_ingest_at_progress",
        "progress(",
    ] {
        assert!(
            sample_window.contains(marker),
            "Issue #815 评论 6042062633 修改 3: 采样入口必须同时给出 rect / 行 id / progress（{}），\
             否则文字层还要自己再算一次时间。",
            marker
        );
    }

    // 光标层不许再自己重新采样一次 track。
    let pos_window = function_window(
        &src,
        "pub(crate) fn compute_coordinated_cursor_position",
        2500,
    );
    assert!(
        pos_window.contains("motion: &crate::sujian_editor_item::render_plan::CoordinatedMotionFrame"),
        "Issue #815 评论 6042062633 修改 3/6: 光标层必须消费文字层已经采好的 CoordinatedMotionFrame，\
         而不是自己再按 frame_now 采样一次 track。"
    );
    assert!(
        !pos_window.contains("sampled_rect_at_progress"),
        "Issue #815 评论 6042062633 修改 3/6: 光标层不得重新采样 track，\
         否则同一帧的文字边界和光标位置可能来自两个不同的时间点。"
    );

    // rebase 交棒必须先采样旧 track 当前帧，不退回逻辑旧 caret。
    let rebase_src = read_src("src/sujian_editor_item/animation/rebase.rs");
    assert!(
        rebase_src.contains("sample_caret_track_frame"),
        "Issue #815 评论 6042062633 修改 3: 快速连续输入/删除交棒时必须先采样旧 cursor track 的当前帧。"
    );
}

// =========================================================================
// 修改点 5：逐帧裁切由当前 caret 帧算出
// =========================================================================

#[test]
fn issue815_modify5_ingest_clip_is_driven_by_current_caret_frame() {
    let src = read_src("src/sujian_editor_item/animated_slice.rs");

    assert!(
        src.contains("pub(crate) fn compute_frame_by_caret_ingest"),
        "Issue #815 评论 6042062633 修改 5: 必须新增「按当前 caret 帧算裁切」的入口，\
         不能把协同吞吐换算成独立 0..1 visible fraction。"
    );

    let window = function_window(&src, "pub(crate) fn compute_frame_by_caret_ingest", 8200);
    assert!(
        window.contains("caret_x: f64"),
        "Issue #815 评论 6042062633 修改 5: 本帧边界就是本帧的 caret.x，必须作为参数传进来。"
    );
    // Issue #815 评论 5946701331 问题1: 另一端改名为 path_other_end_x，
    // Delete 键时是静止 caret，Backspace/Insert 时是构造期锚点。
    assert!(
        window.contains("path_other_end_x.min(boundary_x)")
            && window.contains("path_other_end_x.max(boundary_x)"),
        "Issue #815 评论 5946701331 问题1: 同行吞吐路径的两端必须与本帧 boundary_x 归一化，\
         不能只比较一个方向。"
    );
    assert!(
        !window.contains("ease_out_quad") && !window.contains("* visible"),
        "Issue #815 评论 6042062633 修改 5: 边界不得写成 anchor + extent * text_progress，\
         那正是「看起来像被剪开而不是吞吐」的根因。"
    );

    // 跨软换行/跨段：必须靠**同侧**行序 + 方向判定，而不是裸的 < / >。
    assert!(
        !window.contains("slice_line > caret_line") && !window.contains("slice_line < caret_line"),
        "Issue #815 评论 5946701331 问题2: 裸的行号大小比较把「向前走」写死了，\
         Backspace 跨行 2→1 时方向完全相反，必须删掉。"
    );
    let phase_window = function_window(&src, "fn ingest_line_phase", 1800);
    assert!(
        phase_window.contains("let forward = to_ord > from_ord"),
        "Issue #815 评论 5946701331 问题2: 跨行相位必须独立成入口并按符号判定方向，\
         Backspace（to < from）方向相反，不能只按 slice_line > caret_line 判。"
    );
    assert!(
        phase_window.contains("let forward = to_ord > from_ord"),
        "Issue #815 评论 5946701331 问题2: 跨行相位必须按同一侧行序的符号判定方向——\
         Backspace（to < from）方向与 Insert 相反。"
    );
    for phase in ["Passed", "OnCurrentLine", "NotReached"] {
        assert!(
            phase_window.contains(phase),
            "Issue #815 评论 5946701331 问题2: 缺少 {:?} 相位判定。",
            phase
        );
    }
}

// =========================================================================
// 复核评论 5946701331 问题1：Delete 键必须有独立吞字边界驱动
// =========================================================================

/// 复核评论 5946701331 问题1。
///
/// Delete 键 old/new caret 本来就是同一位置，cursor track 从头到尾不移动。
/// 若吞字边界无条件等于 caret.x，`anchor == caret_x` 会让首帧裁切宽度就是 0，
/// DeleteConceal 直接消失。所以必须有显式的 boundary driver 区分两种删除。
#[test]
fn issue815_review1_delete_forward_has_its_own_ingest_boundary() {
    let slice_src = read_src("src/sujian_editor_item/animated_slice.rs");
    assert!(
        slice_src.contains("pub(crate) enum IngestBoundaryDriver")
            && slice_src.contains("CaretPosition")
            && slice_src.contains("DeleteForwardBoundary"),
        "Issue #815 评论 5946701331 问题1: 必须给协同吞吐增加明确的 boundary driver，\
         CaretTrack 不能只表达「必须等于真实 caret.x」。"
    );
    let driver_window = function_window(&slice_src, "pub(crate) enum IngestBoundaryDriver", 1200);
    assert!(
        !driver_window.contains("Timeline") && !driver_window.contains("duration_ms"),
        "Issue #815 评论 5946701331 问题1: DeleteForwardBoundary 是这笔删除事务的吞字边界轨迹，\
         不是旧的独立文字 timeline，不得带 duration_ms。"
    );

    let ingest_window = function_window(
        &slice_src,
        "pub(crate) fn compute_frame_by_caret_ingest",
        8200,
    );
    assert!(
        ingest_window.contains("IngestBoundaryDriver::DeleteForwardBoundary => caret_x"),
        "Issue #815 评论 5946701331 问题1: Delete 键的另一端必须是静止 caret 本身，\
         边界才可能从被删区间右端朝它收拢。"
    );

    // slices.rs 必须按 conceal_to_left_edge 区分 Backspace / Delete 键。
    let slices_src = read_src("src/sujian_editor_item/animation/transaction_builder/slices.rs");
    let slices_delete_window = function_window(
        &slices_src,
        "pub(crate) fn build_delete_conceal_slices",
        8800,
    );
    assert!(
        slices_delete_window.contains("if conceal_to_left_edge")
            && slices_delete_window.contains("IngestBoundaryDriver::CaretPosition")
            && slices_delete_window.contains("IngestBoundaryDriver::DeleteForwardBoundary"),
        "Issue #815 评论 5946701331 问题1: build_delete_conceal_slices 必须按 \
         conceal_to_left_edge 区分 Backspace（CaretPosition）与 Delete 键\
         （DeleteForwardBoundary），不能都强行解释成「边界等于真实 caret」。"
    );
}

// =========================================================================
// 复核评论 5946701331 问题2：跨行必须同侧行序 + 方向感知
// =========================================================================

/// 复核评论 5946701331 问题2。
///
/// `VisualLine.id` 每次 canonical 排版都从 0 重新编号，InsertReveal 的 slice 来自
/// new snapshot、DeleteConceal 来自 old snapshot，不能把两份 revision 的行号直接
/// 做大小比较。必须在**同一份 snapshot 内**建立起点行/终点行行序。
#[test]
fn issue815_review2_cross_line_uses_same_side_line_ordinals() {
    let slices_src = read_src("src/sujian_editor_item/animation/transaction_builder/slices.rs");
    // Insert 侧：起点/终点行都从 new snapshot 查。
    let insert_window = function_window(
        &slices_src,
        "pub(crate) fn build_insert_reveal_slices",
        10400,
    );
    assert!(
        insert_window.contains("line_ordinal_for_byte(new_snapshot"),
        "Issue #815 评论 5946701331 问题2: Insert 跨行吞吐必须在 new snapshot 内建立\
         起点行（inserted_range.start）与终点行，不拿 old 行号比较。"
    );
    // Delete 侧：起点/终点行都从 old snapshot 查。
    let delete_window = function_window(
        &slices_src,
        "pub(crate) fn build_delete_conceal_slices",
        8800,
    );
    assert!(
        delete_window.contains("line_ordinal_for_visual_line_id(old_snapshot")
            && delete_window.contains("line_ordinal_for_byte(old_snapshot"),
        "Issue #815 评论 5946701331 问题2 + 5947230558 问题2: Delete 跨行吞吐必须在\
         old snapshot 内建立起点行（old caret line）与终点行（deleted_range 所在行），\
         不拿 new 行号比较。起点行由 old 侧 visual_line_id 精确查，不再靠 y 容差猜。"
    );
    assert!(
        !delete_window.contains("line_ordinal_for_byte(new_snapshot")
            && !insert_window.contains("line_ordinal_for_byte(old_snapshot"),
        "Issue #815 评论 5946701331 问题2: 两侧行序必须同源，禁止跨 snapshot 比大小。"
    );

    // cursor_motion 不得把两个不同 revision 的 from/to line id 交给 slice 比大小。
    let cursor_src = read_src("src/sujian_editor_item/animation/cursor_motion.rs");
    assert!(
        !cursor_src.contains("from_visual_line_id <"),
        "Issue #815 评论 5946701331 问题2: ingest 采样不得把跨 revision 的 \
         from/to line id 做大小比较。"
    );
}

// =========================================================================
// 复核评论 5946701331 问题3：收口后不再猜终态，clip 逐 unit 判断
// =========================================================================

/// 复核评论 5946701331 问题3。
///
/// DeleteConceal 的 `compute_frame(visible)` 语义是 visible=1 旧字完整可见、
/// visible=0 旧字全吞。收口后用统一的 `compute_frame(1.0)` 会把旧字重新画成完整宽度。
#[test]
fn issue815_review3_retired_ingest_emits_no_glyph_and_clip_is_per_unit() {
    let src = read_src("src/sujian_editor_item/animation/render_plan_builder.rs");
    let text_window = function_window(&src, "fn build_text_animation_plan_with_sample", 9000);

    let glyph_start = text_window
        .find("for unit in &tx.units")
        .unwrap_or_else(|| {
            panic!("找不到文字 glyph 循环起点");
        });
    let glyph_window = &text_window[glyph_start..];
    assert!(
        !glyph_window.contains("unit.slice.compute_frame(1.0)"),
        "Issue #815 评论 5946701331 问题3: CaretTrack unit 收口后不得用 compute_frame(1.0) \
         猜终态——那会让 DeleteConceal 的旧字整段重现。终态就是不生成 glyph。"
    );
    assert!(
        glyph_window.contains("caret_sample.map(")
            && glyph_window.contains("let Some(frame) = frame else"),
        "Issue #815 评论 5946701331 问题3: 本帧没有 owner caret sample 时，\
         CaretTrack unit 必须得到 None 并直接跳过该 glyph，交还 canonical。"
    );

    // clip 收集在 build_render_plan_full 里（不在 build_text_animation_plan_with_sample）。
    let plan_window = function_window(&src, "pub(crate) fn build_render_plan_full", 9000);
    // 注意：旧写法仍然以注释形式保留在代码里（说明为什么删掉），
    // 所以这里断言「删除原因写在注释里」+「逐 unit 判断已落地」，而不是断言子串消失。
    assert!(
        plan_window.contains("之前这里是"),
        "Issue #815 评论 5946701331 问题3: 必须写下 clip 收集为何从整笔事务跳过改成逐 unit。"
    );
    assert!(
        plan_window.contains("unit.timing.is_caret_track() && !owns_caret"),
        "Issue #815 评论 5946701331 问题3: clip 收集必须逐 unit 判断——\
         CaretTrack 只在本事务 owns caret 时收它自己的 static hidden rect，\
         Timed Reflow 不依赖 caret ownership，按自己的生命周期继续收。"
    );
}

// =========================================================================
// 复核评论 5947443780 问题1：IME commit 特殊 crossfade 必须带完整 ingest 元数据
// =========================================================================

/// 复核评论 5947443780 问题1。
///
/// `build_composition_commit_crossfade_slices()` 之前只写 `is_caret_line`，5 个
/// ingest 元数据字段全是构造器默认值，`ingest_line_phase()` 永远退化成
/// `OnCurrentLine` —— 跨软换行时每一行都拿同一个 caret.x 当"当前行"边界。
#[test]
fn issue815_review5_composition_crossfade_carries_ingest_metadata() {
    let src = read_src("src/sujian_editor_item/animation/transaction_builder/slices.rs");
    let window = function_window(
        &src,
        "pub(crate) fn build_composition_commit_crossfade_slices",
        13000,
    );

    for endpoint in [
        "old_ingest_from_line_ord",
        "old_ingest_to_line_ord",
        "new_ingest_from_line_ord",
        "new_ingest_to_line_ord",
    ] {
        assert!(
            window.contains(endpoint),
            "Issue #815 评论 5947443780 问题1: composition crossfade 必须在各自 snapshot 内\
             建立吞吐路径两端（{}），否则 ingest_line_phase 永远退化成 OnCurrentLine。",
            endpoint
        );
    }
    for field in [
        "ingest_line_ord",
        "ingest_from_line_ord",
        "ingest_to_line_ord",
        "ingest_boundary_driver",
    ] {
        assert!(
            window.contains(field),
            "Issue #815 评论 5947443780 问题1: composition 切片必须写入 {}，\
             否则协同下被当成 CaretTrack 却没有同侧行序可判跨行。",
            field
        );
    }
    assert!(
        window.contains("old_line_removed_right_edge"),
        "Issue #815 评论 5947443780 问题1: 前删方向的 composition 吞字必须写入 \
         ingest_boundary_from_x（旧快照当前行被移除区间的右端），不能留 None。"
    );

    // 旧的跨 snapshot 锚点选择必须彻底消失。
    for banned in [
        "old_cursor_visual_line_id.is_none_or(|cid| old_line.visual_line_id == cid)",
        "new_cursor_visual_line_id.is_none_or(|cid| new_line.visual_line_id == cid)",
    ] {
        assert!(
            !window.contains(banned),
            "Issue #815 评论 5947443780 问题1: 不得再用另一侧 line id 选锚点行（{}）。\
             吐字锚点 = 吞吐起点行，吞字锚点 = 吞吐终点行，都在本侧 snapshot 内算出。",
            banned
        );
    }
    // 锚点行必须绑到 ingest 端点行序，而不是 caret 所在行。
    assert!(
        window.contains("old_ingest_to_line_ord == old_line_ord"),
        "Issue #815 评论 5947443780 问题1: 吞字锚点行必须是吞吐终点行序。"
    );
    assert!(
        window.contains("new_ingest_from_line_ord == new_line_ord"),
        "Issue #815 评论 5947443780 问题1: 吐字锚点行必须是吞吐起点行序。"
    );
}

// =========================================================================
// 复核评论 5947443780 问题3：旧的「两条独立动画」定义必须清干净
// =========================================================================

/// 复核评论 5947443780 问题3。
///
/// 顶部注释还写着「吞吐字始终用 Timed timing」「文字 progress 不消费 caret
/// frame」，与本轮实现（协同 InsertReveal/DeleteConceal 是 CaretTrack，
/// 一次采样同时驱动文字与光标，Reflow 才是独立 Timed）矛盾。
#[test]
fn issue815_review5_stale_two_independent_timeline_wording_is_gone() {
    let src = read_src("src/sujian_editor_item/animation/transaction_builder.rs");

    assert!(
        src.contains("Issue #815 评论 5947443780 问题3"),
        "Issue #815 评论 5947443780 问题3: 必须在 builder 顶部写下新的协同定义。"
    );
    assert!(
        src.contains("一条 caret 运动轨迹")
            && src.contains("以该轨迹当前帧为吞吐边界")
            && src.contains("Reflow 可独立"),
        "Issue #815 评论 5947443780 问题3: 必须写成「一条 caret 运动轨迹 + \
         文字以该轨迹当前帧为吞吐边界 + Reflow 可独立」。"
    );

    // create_transaction_from_prepared_handoff 里那段旧定义也必须换掉。
    let handoff = function_window(&src, "fn create_transaction_from_prepared_handoff", 12000);
    assert!(
        !handoff.contains("文字 progress 只来自文字 timeline"),
        "Issue #815 评论 5947443780 问题3: create_transaction_from_prepared_handoff 里\
         「文字 progress 只来自文字 timeline / 两条独立动画」的定义已被 #815 推翻，必须删掉。"
    );
    assert!(
        !handoff.contains("同时拥有文字和光标两条轨迹"),
        "Issue #815 评论 5947443780 问题3: 「同时拥有文字和光标两条轨迹」是 #815 之前的说法，\
         必须换成「吞吐字是 CaretTrack，边界来自 cursor track 当前帧」。"
    );
    // 仍然成立的部分要留下：协同必须要求有效 caret motion。
    assert!(
        handoff.contains("有效 caret motion"),
        "Issue #815 评论 5947443780 问题3: 要保留「协同要求有效 caret motion」这句——\
         没有 track 就没有吞吐边界来源，协同事务必须拒绝而不是退化成文字自己播。"
    );

    // edit_spec.rs 里的字段文档同样不能留旧定义。
    let spec_src = read_src("src/sujian_editor_item/animation/transaction_builder/edit_spec.rs");
    assert!(
        !spec_src.contains("三种语义彻底分开"),
        "Issue #815 评论 5947443780 问题3: edit_spec.rs 的 coordinated_animation_enabled \
         字段文档还写着「三种语义彻底分开」，是 #815 之前的定义，必须删掉。"
    );
}

// =========================================================================
// 复核评论 5947728704 问题1：跨行相位必须吃本帧真实 caret.y，不能再吃 raw progress
// =========================================================================

/// 复核评论 5947728704 问题1。
///
/// `sampled_rect_at_progress()` 走 `ease_out_cubic`，屏幕 `caret.x/y` 已经是
/// easing 后的几何。若跨行相位再用线性 `from_ord + span * progress` 推算边界行，
/// 同一个 `SampledCaretFrame` 里就会藏着两条不同的运动。
#[test]
fn issue815_review6_cross_line_phase_consumes_real_caret_y() {
    let src = read_src("src/sujian_editor_item/animated_slice.rs");

    assert!(
        src.contains("pub ingest_line_top: Option<f64>")
            && src.contains("pub ingest_line_bottom: Option<f64>"),
        "Issue #815 评论 5947728704 问题1: AnimatedSlice 必须带本 slice 自己那一侧 \
         canonical 的行 top/bottom，跨行相位要拿它和本帧 caret.y 比。"
    );

    let phase_window = function_window(&src, "fn ingest_line_phase", 2600);
    assert!(
        phase_window.contains("fn ingest_line_phase(&self, caret_y: f64)"),
        "Issue #815 评论 5947728704 问题1: 跨行相位入口必须改成吃本帧真实 caret.y。"
    );
    for stale in ["boundary_ord", "span as f64", "let delta"] {
        assert!(
            !phase_window.contains(stale),
            "Issue #815 评论 5947728704 问题1: 跨行相位不得再用 `{}` 从 raw progress \
             线性推边界行——那会让同一份 SampledCaretFrame 里藏着两条运动。",
            stale
        );
    }
    assert!(
        phase_window.contains("ingest_line_top") && phase_window.contains("ingest_line_bottom"),
        "Issue #815 评论 5947728704 问题1: 相位判定必须真的用上本侧行几何。"
    );

    // 0.5 行序容差会让相邻两行在同一帧同时成为当前行，等于拿上一行的 caret.x 裁下一行。
    assert!(
        !phase_window.contains("0.5"),
        "Issue #815 评论 5947728704 问题1: 不得再用 0.5 行序容差判定当前行——\
         from=0/to=1 时 progress==0.5 会让两行同时 OnCurrentLine。"
    );
}

/// 复核评论 5947728704 问题1：三个参数必须来自同一份 `SampledCaretFrame`。
#[test]
fn issue815_review6_ingest_frame_uses_one_sample_for_x_y_progress() {
    let src = read_src("src/sujian_editor_item/animation/render_plan_builder.rs");
    assert!(
        src.contains("caret.ingest_line_ord,"),
        "Issue #815 评论 5947728704 问题1: render plan 必须把同一份 SampledCaretFrame 的 \
         x（横向边界）/ y（当前行）/ progress（只给 DeleteForwardBoundary 用）一起传下去。"
    );

    let ingest_src = read_src("src/sujian_editor_item/animated_slice.rs");
    let ingest_window = function_window(
        &ingest_src,
        "pub(crate) fn compute_frame_by_caret_ingest",
        8200,
    );
    for param in [
        "caret_x: f64",
        "caret_y: f64",
        "sampled_ingest_line_ord: Option<usize>",
        "is_ingest_segment: bool",
        // Issue #815 评论 5953049681 问题1: 整条 track 的全局进度已从这个入口删除，
        // 吞吐边界只吃**段内局部**进度 `ingest_progress`。
        "ingest_progress: f64",
    ] {
        assert!(
            ingest_window.contains(param),
            "Issue #815 评论 5953049681 问题1: 入口签名必须同时收 x / y / 段内局部进度 \
             以及采样的 ingest_line_ord / is_ingest_segment / ingest_side，缺少 {}。",
            param
        );
    }
    assert!(
        !ingest_window.contains("caret_progress: f64"),
        "Issue #815 评论 5953049681 问题1: 全局 caret_progress 必须从这个入口删掉——\
         它会让某一行吃掉整笔事务的进度，边界只收三分之一就切到下一阶段。"
    );
    assert!(
        ingest_window.contains("self.ingest_phase_from_route_ord(")
            || ingest_window.contains("self.ingest_line_phase(caret_y)"),
        "Issue #815 评论 5949097065: 相位必须优先由采样得到的权威 ingest_line_ord 决定，\n         caret.y 只是拿不到行序时的降级；progress 不得再推导行序。"
    );
}

/// 复核评论 5947728704 问题1：三个 builder 都要写本侧行几何。
#[test]
fn issue815_review6_all_three_builders_write_same_side_line_geometry() {
    let src = read_src("src/sujian_editor_item/animation/transaction_builder/slices.rs");

    let insert_window = function_window(&src, "pub(crate) fn build_insert_reveal_slices", 10400);
    assert!(
        insert_window.contains("ingest_line_top = Some(new_line.visual_line_top)")
            && insert_window.contains("ingest_line_bottom = Some(new_line.visual_line_bottom)"),
        "Issue #815 评论 5947728704 问题1: InsertReveal 必须写 new snapshot 侧的行几何。"
    );

    let delete_window = function_window(&src, "pub(crate) fn build_delete_conceal_slices", 8800);
    assert!(
        delete_window.contains("ingest_line_top = Some(old_line.visual_line_top)")
            && delete_window.contains("ingest_line_bottom = Some(old_line.visual_line_bottom)"),
        "Issue #815 评论 5947728704 问题1: DeleteConceal 必须写 old snapshot 侧的行几何。"
    );

    let crossfade_window = function_window(
        &src,
        "pub(crate) fn build_composition_commit_crossfade_slices",
        13000,
    );
    assert_eq!(
        crossfade_window.matches("ingest_line_top = Some(").count(),
        2,
        "Issue #815 评论 5947728704 问题1: IME commit 交叉淡化必须两侧都写行几何——\
         Reveal 用 new、Conceal 用 old。"
    );
}

/// 复核评论 5947728704 问题2：`sampled_rect_at_progress()` 的注释仍是 #808 旧语义。
#[test]
fn issue815_review6_sampled_rect_doc_no_longer_claims_independent_text_easing() {
    let src = read_src("src/sujian_editor_item/animation/transaction/types.rs");
    for stale in [
        "文字 reveal/conceal 用文字自己的 easing",
        "不保留\"文字效果跟着光标边界\"",
    ] {
        assert!(
            !src.contains(stale),
            "Issue #815 评论 5947728704 问题2: sampled_rect_at_progress 的文档仍在说 {}, \
             这与 #815 的「文字与光标消费同一份采样」直接冲突。",
            stale
        );
    }
    assert!(
        src.contains("协同 InsertReveal/DeleteConceal") && src.contains("sample_caret_track_frame"),
        "Issue #815 评论 5947728704 问题2: 必须写明协同吞吐字与光标消费同一份 \
         sample_caret_track_frame 采样，只有非协同 Timed 文字才有自己的 easing。"
    );
}

// =========================================================================
// 复核评论 5950375533 问题1：多行 Insert route 必须真的插 RowHandoff
// =========================================================================

/// 复核评论 5950375533 问题1。
///
/// 原实现扫完第一行后，让第二行的 `IngestLine` 从第一行右端连到本行右端/新 caret，
/// 把一条行间斜线标成 `IngestLine(本行)`——正是第 6 轮禁止的行为。
#[test]
fn issue815_review7_insert_route_inserts_row_handoff_between_rows() {
    let src = read_src("src/sujian_editor_item/animation/transaction_builder/ingest_route.rs");
    let window = function_window(&src, "pub(crate) fn build_insert_route", 4200);
    assert!(
        window.contains("CaretTrackSegmentKind::RowHandoff"),
        "Issue #815 评论 5950375533 问题1: build_insert_route 必须显式 push RowHandoff，\
         否则行间斜线 x 会冒充本行吞吐边界。"
    );
    assert!(
        window.contains("to: next.caret_rect_at(next.left)"),
        "Issue #815 评论 5950375533 问题1: RowHandoff 必须落到下一行**左端**，\
         下一条 IngestLine 才能从本行左端起步。"
    );
    assert!(
        window.contains("cursor = row.caret_rect_at(row.left);"),
        "Issue #815 评论 5950375533 问题1: 每条 IngestLine 的起点必须重置为本行左端，\
         绝不继承上一行右端。"
    );
}

// =========================================================================
// 复核评论 5950375533 问题2：前删必须是静止吞吐段，不是 LayoutHandoff
// =========================================================================

/// 复核评论 5950375533 问题2。
///
/// `LayoutHandoff` 语义是 `ingest_line_ord = None` ⇒ 文字层永远进不到边界收拢逻辑，
/// 旧字整段动画期间保持完整。前删必须给 `is_ingest_segment=true` 的静止吞吐段。
#[test]
fn issue815_review7_forward_delete_is_a_static_ingest_segment() {
    let src = read_src("src/sujian_editor_item/animation/transaction_builder/ingest_route.rs");
    // Issue #815 评论 5954004872 问题1: 事务级早退已删除；前删行的静止语义现在
    // 由 `row_ingest_start` 表达——`DeleteForwardBoundary` 行从 `row.left` 起手，
    // 因此它的 IngestLine 是 start == end 的静止段。
    assert!(
        src.contains("IngestBoundaryDriver::DeleteForwardBoundary => row.caret_rect_at(row.left)"),
        "Issue #815 评论 5954004872 问题1: DeleteForwardBoundary 行的吞吐起点必须是 \
         row.left（静止吞字），CaretPosition 行才是 row.right。"
    );
    let delete = function_window(&src, "pub(crate) fn build_delete_route", 7000);
    assert!(
        delete.contains("from: row_ingest_start(row)")
            && delete.contains("to: row_ingest_end(row)"),
        "Issue #815 评论 5954004872 问题1: 每行 IngestLine 必须按行级 driver 取起止点。"
    );
    assert!(
        delete.contains("kind: CaretTrackSegmentKind::IngestLine"),
        "前删行仍然要产出吞吐段，否则 DeleteForwardBoundary 收不到 ingest_progress。"
    );
}

// =========================================================================
// 复核评论 5950375533 问题2：前删必须是静止吞吐段，不是 LayoutHandoff
// =========================================================================

// =========================================================================
// 复核评论 5950375533 问题3：route 的屏幕起点必须来自 handoff
// =========================================================================

/// 复核评论 5950375533 问题3。
///
/// 旧实现固定用 `spec.old_cursor_rect` 当屏幕起点，handoff 只写进顶层 `track.from`，
/// 而 `segments` 非空时渲染读的是 `segments[0].from` —— 快速连续输入会跳回逻辑
/// old caret。
#[test]
fn issue815_review7_route_screen_origin_prefers_caret_handoff() {
    let src = read_src("src/sujian_editor_item/animation/transaction_builder/ingest_route.rs");
    assert!(
        src.contains("spec\n        .caret_handoff")
            || src.contains(".caret_handoff\n        .as_ref()"),
        "Issue #815 评论 5950375533 问题3: route 的屏幕起点必须优先取 caret_handoff。"
    );
    assert!(
        src.contains("map(|handoff| &handoff.sampled)"),
        "Issue #815 评论 5950375533 问题3: 必须用 handoff.sampled（上一帧真正画出来的位置）。"
    );
    assert!(
        src.contains(".or(spec.old_cursor_rect.as_ref())"),
        "Issue #815 评论 5950375533 问题3: 拿不到 handoff 才退回逻辑 old_cursor_rect。"
    );
    let insert = function_window(&src, "pub(crate) fn build_insert_route", 2200);
    let delete = function_window(&src, "pub(crate) fn build_delete_route", 2400);
    assert!(
        insert.contains("screen_caret: &CursorRect")
            && delete.contains("screen_caret: &CursorRect"),
        "Issue #815 评论 5950375533 问题3: Insert / Delete 两条路都必须接同一个屏幕起点参数。"
    );
    assert!(
        !insert.contains("old_cursor_rect") && !delete.contains("old_cursor_rect"),
        "Issue #815 评论 5950375533 问题3: route builder 内部不得再直接读逻辑 \
         old_cursor_rect，屏幕起点只能由调用方传进来。"
    );
}

// =========================================================================
// 复核评论 5950375533 问题4：0 长度 segment 不得白占一半时长
// =========================================================================

/// 复核评论 5950375533 问题4。
///
/// 所有 segment 均分总时长，一个 0 长度 segment 会让最普通的同行输入
/// 前一半时间一个字都不吐。
#[test]
fn issue815_review7_zero_length_segments_are_not_emitted() {
    let src = read_src("src/sujian_editor_item/animation/transaction_builder/ingest_route.rs");
    assert!(
        src.contains("fn same_rect("),
        "Issue #815 评论 5950375533 问题4: 需要一个几何比较来识别 0 长度 segment。"
    );
    let insert = function_window(&src, "pub(crate) fn build_insert_route", 4200);
    assert!(
        insert.contains("if !same_rect(&cursor, &ingest_start)"),
        "Issue #815 评论 5950375533 问题4: 屏幕 caret 与吞吐起点相同时不得生成 LayoutHandoff。"
    );
    let delete = function_window(&src, "pub(crate) fn build_delete_route", 7000);
    assert!(
        delete.contains("if !same_rect(&swallow_end, tail_target)"),
        "Issue #815 评论 5950375533 问题4: old 侧吞字终点与 new caret 相同时不得生成末尾 \
         RowHandoff。"
    );
}

// =========================================================================
// 复核评论 5950677031 问题1：普通退格分支必须消费 screen_caret
// =========================================================================

/// 复核评论 5950677031 问题1。
///
/// 旧实现签名虽然已经接了 `screen_caret`，但普通退格分支直接进
/// `for ... in rows.iter().enumerate().rev()`，第一条 `IngestLine` 的起点固定写成
/// `row.caret_rect_at(row.right)`，参数完全没被消费 —— 于是 `build_ingest_route()`
/// 选好的 `caret_handoff.sampled` 传进来又被丢掉。
#[test]
fn issue815_review8_backspace_route_consumes_screen_caret() {
    let src = read_src("src/sujian_editor_item/animation/transaction_builder/ingest_route.rs");
    let window = function_window(&src, "pub(crate) fn build_delete_route", 7000);
    assert!(
        window.contains("let start_row = rows.last().copied()"),
        "Issue #815 评论 5950677031 问题1: 退格 old-side 吞吐起点必须取**最后一条吞字行**。"
    );
    assert!(
        window.contains("if !same_rect(screen_caret, &ingest_start)"),
        "Issue #815 评论 5950677031 问题1: 屏幕 caret 与吞吐起点不同时先补一段几何换位；\
         相同则不白占时长。"
    );
    assert!(
        window.contains("kind: CaretTrackSegmentKind::LayoutHandoff")
            && window.contains("from: *screen_caret"),
        "Issue #815 评论 5950677031 问题1: 换位段必须从上一帧**真实**屏幕 caret 起步。"
    );
    assert!(
        window.contains("to: ingest_start") && window.contains("ingest_line_ord: None"),
        "Issue #815 评论 5950677031 问题1: 换位段是纯几何，ingest_line_ord 必须为 None \
         （is_ingest_segment = false），不冒充吞吐。"
    );
}

// =========================================================================
// 复核评论 5950677031 问题2：退格 route 必须逐段连续
// =========================================================================

/// 复核评论 5950677031 问题2。
///
/// 行间 `RowHandoff` 原来落到上一行 **left**，而紧接着的 `IngestLine(next_up)` 从
/// 上一行 **right** 起步 —— 两段在边界处不连续，采样切过去那一帧会瞬移。
#[test]
fn issue815_review8_backspace_row_handoff_lands_on_previous_row_right() {
    let src = read_src("src/sujian_editor_item/animation/transaction_builder/ingest_route.rs");
    let window = function_window(&src, "pub(crate) fn build_delete_route", 7000);
    assert!(
        window.contains("to: row_ingest_start(next_up)"),
        "Issue #815 评论 5950677031 问题2 / Issue #815 评论 5954004872 问题1: \
         退格行间 RowHandoff 必须落到下一段的**实际起点** row_ingest_start(next_up)，\
         这样前删行（从 left 起手）也不会瞬移。"
    );
    assert!(
        !window.contains("to: next_up.caret_rect_at(next_up.left)"),
        "Issue #815 评论 5950677031 问题2: 落到上一行 left 会让 route 在段边界断开。"
    );
    // 行为测试必须断言逐段连续，而不是逐个看 x。
    let tests = read_src("src/sujian_editor_item/animated_slice/ingest_tests.rs");
    assert!(
        tests.contains("fn assert_route_is_continuous("),
        "Issue #815 评论 5950677031 问题2: 需要一个断言 segments[i].to == segments[i+1].from \
         的辅助函数——逐段连续比逐个看 x 靠谱，否则段拼断了也会被判成通过。"
    );
    assert!(
        tests.contains("assert_route_is_continuous(&segments")
            && tests.contains("assert_route_is_continuous(&with_tail"),
        "Issue #815 评论 5950677031 问题2: 退格 route 的每一条路径都要过连续性断言。"
    );
    // 生产路径必须有退格的 handoff 覆盖（现有那个用的是 insert spec）。
    let builder_tests = read_src("src/sujian_editor_item/animation/transaction_builder/tests.rs");
    assert!(
        builder_tests.contains(
            "fn issue815_review8_production_backspace_rebase_starts_from_handoff_sampled_caret"
        ),
        "Issue #815 评论 5950677031 问题1: 必须补**退格**的生产路径 handoff 测试，\
         现有的 review7 测试用的是 insert spec，只覆盖 Insert。"
    );
    assert!(
        builder_tests.contains("TextVisualOperationKind::Delete"),
        "Issue #815 评论 5950677031 问题1: 退格生产测试必须真的走 Delete 事务。"
    );
}

// =========================================================================
// 复核评论 5950887715：IME Mixed 路径必须有 snapshot side，不再退回直线
// =========================================================================

/// 复核评论 5950887715。
///
/// root cause 不是「old/new 行号不能比较所以没法建 route」，而是 segment 只有
/// `ingest_line_ord` 却没有说明它属于哪一侧。加上 side 之后行序只在同一 side 内比较，
/// Mixed 就能建出「先吞旧 preedit、再吐新 candidate」的分段路线。
#[test]
fn issue815_review9_segments_carry_snapshot_side() {
    let types = read_src("src/sujian_editor_item/animation/transaction/types.rs");
    assert!(
        types.contains("pub(crate) enum IngestSnapshotSide"),
        "Issue #815 评论 5950887715: 必须有 IngestSnapshotSide（含 Old / New 两个变体）。"
    );
    assert!(
        types.contains("pub ingest_side: Option<IngestSnapshotSide>"),
        "Issue #815 评论 5950887715: CaretTrackSegment 必须带 ingest_side。"
    );
    let plan = read_src("src/sujian_editor_item/render_plan.rs");
    assert!(
        plan.contains("pub ingest_side: Option<IngestSnapshotSide>"),
        "Issue #815 评论 5950887715: SampledCaretFrame 必须把 side 带给文字层。"
    );
}

/// Mixed 不得再返回空 route，且两侧必须分开收行。
#[test]
fn issue815_review9_mixed_route_is_built_not_skipped() {
    let route = read_src("src/sujian_editor_item/animation/transaction_builder/ingest_route.rs");
    assert!(
        route.contains("pub(crate) fn collect_delete_rows(")
            && route.contains("pub(crate) fn collect_insert_rows("),
        "Issue #815 评论 5950887715: Reveal / Conceal 必须分开收行，各自的 ordinal \
         只来自自己那侧 canonical。"
    );
    assert!(
        !route.contains("IngestRouteShape::Mixed => Vec::new()"),
        "Issue #815 评论 5950887715: Mixed 不得再直接返回空 route——那会让 IME \
         退回 old→new 一条斜线。"
    );
    assert!(
        route.contains("build_delete_route(&delete_rows, screen_caret, None)")
            && route.contains("build_insert_route(&insert_rows, &insert_start, new_caret)"),
        "Issue #815 评论 5950887715: Mixed 路线必须是「先吞旧 preedit（Old 侧，\
         不带末尾换位），再接 new 侧吐字」。"
    );

    // transaction_builder 里那道 composition_commit_crossfade.is_none() 的门必须删掉。
    let builder = read_src("src/sujian_editor_item/animation/transaction_builder.rs");
    assert!(
        !builder.contains("&& spec.composition_commit_crossfade.is_none()"),
        "Issue #815 评论 5950887715: 必须删掉 composition_commit_crossfade.is_none() \
         这道门，否则 IME commit 根本不会走正式 route。"
    );
}

/// 文字层必须先按 side 隔离，再谈行序。
#[test]
fn issue815_review9_slice_isolates_sides_before_line_ordinal() {
    let src = read_src("src/sujian_editor_item/animated_slice.rs");
    let window = function_window(&src, "let slice_side = match self.kind", 2600);
    assert!(
        window.contains("AnimatedSliceKind::DeleteConceal => IngestSnapshotSide::Old"),
        "Issue #815 评论 5950887715: DeleteConceal 属于 old snapshot 侧。"
    );
    assert!(
        window.contains("IngestSnapshotSide::New"),
        "Issue #815 评论 5950887715: InsertReveal 属于 new snapshot 侧。"
    );
    assert!(
        window.contains("side_rank(sampled_side) < side_rank(slice_side)")
            && window.contains("side_rank(sampled_side) > side_rank(slice_side)"),
        "Issue #815 评论 5950887715: 必须按 side 阶段序隔离两侧，绝不让 old / new \
         的 ordinal 互相比较。"
    );
}

// =========================================================================
// 复核评论 5953049681 问题1：前删边界吃本段 local progress
// =========================================================================

/// 复核评论 5953049681 问题1。
///
/// 段均分总时长，所以 Mixed 的第一段结束时全局 progress 只有 ~1/n。若前删边界吃
/// 全局 progress，边界只收了一小部分，紧接着 side 切 New 后 old slice 因 side
/// phase 直接变 `Passed` —— 剩下那一大半旧字在一帧内突然消失。
#[test]
fn issue815_review10_forward_boundary_uses_segment_local_progress() {
    let plan = read_src("src/sujian_editor_item/render_plan.rs");
    assert!(
        plan.contains("pub ingest_progress: f64"),
        "Issue #815 评论 5953049681 问题1: SampledCaretFrame 必须带本段 local 进度。"
    );
    let types = read_src("src/sujian_editor_item/animation/transaction/types.rs");
    let sampling = function_window(&types, "pub fn sampled_ingest_at_progress", 2200);
    assert!(
        sampling.contains("f64,"),
        "Issue #815 评论 5953049681 问题1: sampled_ingest_at_progress 的返回值末尾\
         必须是本段 local eased progress（由 sampled_segment_at_progress 一路传下，不重算）。"
    );
    let slice = read_src("src/sujian_editor_item/animated_slice.rs");
    let ingest = function_window(&slice, "pub(crate) fn compute_frame_by_caret_ingest", 8200);
    assert!(
        ingest.contains("ingest_progress: f64"),
        "Issue #815 评论 5953049681 问题1: 逐帧裁切入口必须接住 local 进度。"
    );
    assert!(
        ingest.contains("let progress = ingest_progress.clamp(0.0, 1.0);"),
        "Issue #815 评论 5953049681 问题1: DeleteForwardBoundary 必须吃 ingest_progress。"
    );
    let forward_branch = function_window(
        &slice,
        "IngestBoundaryDriver::DeleteForwardBoundary =>",
        900,
    );
    assert!(
        !forward_branch.contains("caret_progress.clamp"),
        "Issue #815 评论 5953049681 问题1: 前删分支不得再吃全局 caret_progress。"
    );
}

// =========================================================================
// 复核评论 5953049681 问题2：Mixed 拼接必须接上一阶段的真实末端
// =========================================================================

/// 复核评论 5953049681 问题2。
///
/// 前删的 old 侧是一条 `from == to == screen_caret` 的静止段，真实终点就是
/// `screen_caret`；若 Mixed 仍用猜出来的 `first_delete.left` 当下一段起点，
/// 只要 `screen_caret != first_delete.left` 相邻段就会瞬移。
#[test]
fn issue815_review10_mixed_route_joins_previous_stage_end() {
    let src = read_src("src/sujian_editor_item/animation/transaction_builder/ingest_route.rs");
    assert!(
        src.contains("segments.last().map(|seg| seg.to).unwrap_or(*screen_caret)"),
        "Issue #815 评论 5953049681 问题2: Mixed 拼接下一阶段必须接上一阶段的真实末端。"
    );
    let mixed = function_window(
        &src,
        "build_delete_route(&delete_rows, screen_caret, None)",
        1600,
    );
    assert!(
        !mixed.contains("let swallow_end = first_delete.caret_rect_at(first_delete.left)"),
        "Issue #815 评论 5953049681 问题2: 不得再重新猜 old 侧结束几何，\
         必须取上一阶段 route 的末端。"
    );
}

// =========================================================================
// 复核评论 5953049681 问题3：driver 必须按行决定
// =========================================================================

/// 复核评论 5953049681 问题3。
///
/// composition crossfade 的 `conceal_to_left_edge` 是按**每个 old cluster**
/// 相对 `new_cursor_rect.x` 单独算的，跨行时同一批 preedit slice 理论上可以同时
/// 出现 `CaretPosition` 与 `DeleteForwardBoundary`。事务级 `any()` 会把整条
/// old 侧 route 缩成一个静止段，把其它退格行一起吞掉。
#[test]
fn issue815_review10_driver_is_decided_per_row() {
    let src = read_src("src/sujian_editor_item/animation/transaction_builder/ingest_route.rs");
    assert!(
        src.contains("pub driver: IngestBoundaryDriver"),
        "Issue #815 评论 5953049681 问题3: driver 必须收进 IngestRow，按行决定。"
    );
    assert!(
        !src.contains("fn is_forward_delete"),
        "Issue #815 评论 5953049681 问题3: 事务级 is_forward_delete 的 any() 判定必须删掉。"
    );
    assert!(
        src.contains("driver_conflict"),
        "Issue #815 评论 5953049681 问题3: 同一行出现两种 driver 必须在构造阶段记\
         invariant diagnostic，不能静默拿一侧猜。"
    );
    let builder = read_src("src/sujian_editor_item/animation/transaction_builder.rs");
    assert!(
        !builder.contains("&& spec.composition_commit_crossfade.is_none()"),
        "Issue #815 评论 5953049681: composition commit 必须继续走正式 route。"
    );
}

// =========================================================================
// 复核评论 5954004872 问题1/2：Delete 按行 driver、Insert 末段不跨行
// =========================================================================

/// 复核评论 5954004872 问题1。
///
/// 行级 driver 下沉后，`build_delete_route` 仍按"全是 Backspace"拼：事务级早退、
/// 行间换位硬编码 `next_up.right`、吞完位置重算。现在统一走 `row_ingest_start/end`。
#[test]
fn issue815_review11_delete_route_is_driven_per_row() {
    let src = read_src("src/sujian_editor_item/animation/transaction_builder/ingest_route.rs");
    assert!(
        src.contains("fn row_ingest_start(row: &IngestRow) -> CursorRect")
            && src.contains("fn row_ingest_end(row: &IngestRow) -> CursorRect"),
        "Issue #815 评论 5954004872 问题1: 必须抽出 row_ingest_start / row_ingest_end \
         两个行级 helper，所有地方都调它们，不在三个地方各猜一次。"
    );
    assert!(
        !src.contains("if first_row.driver == IngestBoundaryDriver::DeleteForwardBoundary"),
        "Issue #815 评论 5954004872 问题1: 事务级早退必须删掉——它会把除首个 row \
         之外的所有 old-side 行一起吞掉。"
    );
    let delete = function_window(&src, "pub(crate) fn build_delete_route", 7000);
    assert!(
        delete.contains("to: row_ingest_start(next_up)"),
        "Issue #815 评论 5954004872 问题1: 行间 RowHandoff 的终点必须是下一段实际的 \
         起点 row_ingest_start(next_up)，不能硬编码 next_up.right——前删行的下一段 \
         是从 next_up.left 起手的，写死 right 会让相邻两段瞬移。"
    );
    assert!(
        delete.contains("let swallow_end = segments")
            && delete.contains(".unwrap_or(*screen_caret);"),
        "Issue #815 评论 5954004872 问题1: 末尾换位的起点必须取上一段真实的 to，\
         不能重算 first_row.left。"
    );
}

/// 复核评论 5954004872 问题2。
///
/// `build_insert_reveal_slices` 会跳过纯空格 / 换行，所以「最后一个可见字符所在行」
/// 不等于「最终 caret 所在行」。`if is_last { *new_caret }` 会把跨行的对角线标成
/// 吞吐段。
#[test]
fn issue815_review11_insert_last_ingest_line_stays_in_its_row() {
    let src = read_src("src/sujian_editor_item/animation/transaction_builder/ingest_route.rs");
    let insert = function_window(&src, "pub(crate) fn build_insert_route", 4600);
    assert!(
        !insert.contains("if is_last"),
        "Issue #815 评论 5954004872 问题2: 最后一条 IngestLine 不得直接连到 new_caret \
         ——末尾是换行时那是下一行的坐标。"
    );
    assert!(
        insert.contains("to: row.caret_rect_at(row.right)"),
        "Issue #815 评论 5954004872 问题2: 每条 IngestLine 只能在**本行内**运动，\
         终点固定取本行右端。"
    );
    assert!(
        insert.contains("if !same_rect(&row_end, new_caret)")
            && insert.contains("kind: CaretTrackSegmentKind::RowHandoff"),
        "Issue #815 评论 5954004872 问题2: 本行右端与最终 caret 不同时，必须追加一条 \
         RowHandoff 承担跨行；相同时才跳过，不白占时长。"
    );
}

/// 同一行内出现两种 driver 属于 invariant 违例，必须有正式事件，不只是 debug log。
#[test]
fn issue815_review11_driver_conflict_has_a_formal_event() {
    let src = read_src("src/sujian_editor_item/animation/transaction_builder/ingest_route.rs");
    let window = function_window(&src, "fn record_ingest_row_driver_conflict", 1800);
    assert!(
        window.contains("editor.anim.ingest_row_driver_conflict")
            && window.contains("record_event"),
        "Issue #815 评论 5954004872 问题1: 同行 driver 冲突必须记进正式 writer_diagnostics \
         事件，只打 debug log 不算 invariant 记录。"
    );
    assert!(
        window.contains("serde_json::json!(line_ord)"),
        "Issue #815 评论 5954004872 问题1: 事件要带冲突行的 line_ord。"
    );
}

// =========================================================================
// 复核评论 5954588641：tail segment 的「刚完成到哪一行」身份
// =========================================================================

/// 复核评论 5954588641。
///
/// 这两个 tail segment 都是几何连续的（`segments[i].to == segments[i+1].from`
/// 成立），所以连续性守卫抓不到它们——真正的问题是 tail 帧报告的
/// `ingest_line_ord` 写反了行，`ingest_phase_from_route_ord()` 于是把已经吐完 /
/// 吞完的行判成 `NotReached`，末尾出现闪回。
#[test]
fn issue815_review12_tail_segments_carry_the_just_finished_row() {
    let src = read_src("src/sujian_editor_item/animation/transaction_builder/ingest_route.rs");

    let insert = function_window(&src, "pub(crate) fn build_insert_route", 4600);
    assert!(
        insert.contains("let final_row = rows.last()"),
        "Issue #815 评论 5954588641 问题1: Insert 的 tail handoff 发生在所有行吐完之后，\
         「刚扫完的行」是 rows.last()，不是 rows.first()。"
    );
    assert!(
        !insert.contains("ingest_line_ord: Some(first.line_ord)"),
        "Issue #815 评论 5954588641 问题1: Insert tail 不得再标 first 行——那会让刚吐完的 \
         最后一行在末尾 handoff 阶段被判 NotReached 而重新隐藏（末尾闪回）。"
    );

    let delete = function_window(&src, "pub(crate) fn build_delete_route", 7000);
    assert!(
        delete.contains("let final_row = rows.first()"),
        "Issue #815 评论 5954588641 问题2: Delete 从大行序往小行序吞，start_row 是\
         **最先**吞的那一行；全部吞完后刚完成的是 rows.first()。"
    );
    assert!(
        !delete.contains("ingest_line_ord: Some(start_row.line_ord)"),
        "Issue #815 评论 5954588641 问题2: Delete tail 不得再标 start_row——那会让已吞掉的\
         低行序旧字在最终 handoff 阶段重新出现。"
    );
    // `swallow_end` 必须继续取上一段真实的 to，不退回重算几何。
    assert!(
        delete.contains("let swallow_end = segments")
            && delete.contains(".unwrap_or(*screen_caret);"),
        "Issue #815 评论 5954588641 问题2: 只改 tail 的 phase identity，吞字终点仍取\
         segments.last().to，不退回重算 first_row.left。"
    );
}

// =========================================================================
// 复核评论 5955090551：协同速度由「打字动画时长」决定，且设置里可调
// =========================================================================

/// 复核评论 5955090551（推翻上一条关闭结论）。
///
/// #815 把协同吞吐字改成 `VisualUnitTiming::CaretTrack`，它**没有自己的时长**，
/// 逐帧进度完全跟随 `cursor_visual_track`。而 `prepare_edit_motion()` 当时仍写死
/// `caret_duration_ms = cursor_animation_duration_ms`，于是真正决定协同速度的
/// 变成了「平滑光标时长」（80–120ms）——实机上表现为 204/212/205ms 甚至 28ms，
/// 「快得看不见」，看起来像动画丢了。
#[test]
fn issue815_review14_coordinated_ingest_uses_typing_duration() {
    let pipeline = read_src("src/sujian_editor_item/pipeline.rs");
    assert!(
        pipeline.contains("ctx.coordinated_animation_enabled && is_body_ingest_edit"),
        "Issue #815 评论 5955090551: 协同正文吞吐的 caret_duration_ms 必须取 \
         typing_animation_duration_ms，不能再无条件取 cursor_animation_duration_ms。"
    );
    assert!(
        pipeline.contains(
            "is_body_ingest_edit = result.operation_kind != EditorOperationKind::CursorOnly"
        ),
        "Issue #815 评论 5955090551: CursorOnly / 鼠标点击 / 纯光标移动仍是「平滑光标」 \
         领域，不能被协同时长接管。"
    );

    let composition = read_src("src/sujian_editor_item/animation/composition.rs");
    // 两处：composition update 与 composition commit 都要套用协同规则。
    assert_eq!(
        composition
            .matches("caret_duration_ms: u64::from(if coordinated_animation_enabled")
            .count(),
        2,
        "Issue #815 评论 5955090551: IME 的 composition update 与 composition commit \
         两处的 caret_duration_ms 都必须按协同规则选择时长。"
    );
    assert!(
        !composition.contains("caret_duration_ms: u64::from(self.cursor_animation_duration_ms)"),
        "Issue #815 评论 5955090551: composition.rs 不得再无条件把平滑光标时长当协同速度。"
    );
}

/// 设置里必须有一个真正驱动协同速度的可调项。
#[test]
fn issue815_review14_settings_expose_coordinated_duration() {
    let qml = read_src("qml/SettingsDialog.qml");
    assert!(
        qml.contains("qsTr(\"协同动画时长\")"),
        "Issue #815 评论 5955090551: 协同开启时必须单独给一个「协同动画时长」，\
         否则真正驱动协同吞吐的时长被藏起来，用户无处可调。"
    );
    assert!(
        qml.contains("setting_typing_animation_duration_ms = coordinatedAnimDuration.value")
            || qml.contains("coordinatedAnimDuration.value"),
        "Issue #815 评论 5955090551: 「协同动画时长」必须绑定 setting_typing_animation_duration_ms。"
    );
    assert!(
        qml.contains("visible: coordinatedAnim.checked"),
        "Issue #815 评论 5955090551: 「协同动画时长」只在协同开启时显示；\
         协同关闭时仍是独立的打字动画时长与平滑光标时长。"
    );
    assert!(
        !qml.contains("setting_smooth_cursor_duration_ms = coordinatedAnimDuration.value"),
        "Issue #815 评论 5955090551: 绝不能把协同速度绑到平滑光标时长——那正是本轮 \
         实机「快得看不见」的根因。"
    );
}

// =========================================================================
// 复核评论 5955676896：同一 setting 不得在两个滑块上各留一份状态
// =========================================================================

/// 复核评论 5955676896。
///
/// 「打字动画持续时间」和「协同动画时长」写的是同一个设置项
/// `setting_typing_animation_duration_ms`。若两个滑块各持一份 `value`，切协同开关后
/// 隐藏滑块的旧值会被 `onClosed` 写回去，把刚调好的时长覆盖掉。
#[test]
fn issue815_review15_typing_and_coordinated_duration_share_one_value() {
    let src = read_src("qml/SettingsDialog.qml");

    assert!(
        src.contains("property real textAnimationDurationValue: 100"),
        "Issue #815 评论 5955676896: 必须有唯一一份共享值 textAnimationDurationValue。"
    );
    assert!(
        src.contains("function setTextAnimationDuration(value)"),
        "Issue #815 评论 5955676896: 两个滑块必须走同一个写入口，\
         由它同时同步 backend 和另一个滑块。"
    );
    let setter = function_window(&src, "function setTextAnimationDuration(value)", 1400);
    for line in [
        "root.textAnimationDurationValue = value",
        "if (coordinatedAnimDuration.value !== value) coordinatedAnimDuration.value = value",
        "if (typingAnimDuration.value !== value) typingAnimDuration.value = value",
        "settingsBackendRef.setting_typing_animation_duration_ms = value",
    ] {
        assert!(
            setter.contains(line),
            "Issue #815 评论 5955676896: 共享写入口必须同时更新共享值、两个滑块和 backend，\
             缺少 `{}`",
            line
        );
    }

    // onClosed 只能写这一份共享值，不能再按协同开关二选一。
    let closed = function_window(&src, "onClosed: {", 2400);
    assert!(
        closed.contains(
            "settingsBackendRef.setting_typing_animation_duration_ms = root.textAnimationDurationValue"
        ),
        "Issue #815 评论 5955676896: onClosed 只写共享值，\
         不得 `coordinatedAnim.checked ? coordinatedAnimDuration.value : typingAnimDuration.value`。"
    );
    assert!(
        !closed.contains("coordinatedAnim.checked ? coordinatedAnimDuration.value"),
        "Issue #815 评论 5955676896: onClosed 按协同开关二选一正是回滚根因——\
         隐藏滑块可能还是旧值。"
    );

    // 两个滑块的写入口都必须收口到共享 setter，不能各自直接写 backend。
    // 整个文件里只允许共享 setter 内部出现这一次直写（setter 自己的那一行），
    // 两个滑块都不得再各自直写 backend。
    assert_eq!(
        src.matches("settingsBackendRef.setting_typing_animation_duration_ms = value")
            .count(),
        1,
        "Issue #815 评论 5955676896: 滑块不得再各自直写 backend，必须走共享 setter；\
         唯一允许的直写在共享 setter 内部。"
    );
    assert_eq!(
        src.matches("onMoved: function() { root.setTextAnimationDuration(value) }")
            .count(),
        2,
        "两个时长滑块的 onMoved 都必须收口到共享 setter。"
    );

    // 旧的「协同不共享 duration」注释与 #815 实际实现相反，必须改掉。
    assert!(
        !src.contains("协同只表示同事务/同首帧/同 rebase"),
        "Issue #815 评论 5955676896: setCoordinatedAnimation 上方的旧注释说协同不共享 \
         duration，已被 #815 推翻，必须删掉。"
    );
}

// =========================================================================
// 修改点 6：每帧每事务只采样一次，完成条件两类分开
// =========================================================================

#[test]
fn issue815_modify6_one_caret_sample_per_frame_and_two_class_completion() {
    let src = read_src("src/sujian_editor_item/animation/render_plan_builder.rs");

    let plan_window = function_window(&src, "pub(crate) fn build_render_plan_full", 9000);
    assert!(
        !plan_window.contains("self.sample_coordinated_motion_frame("),
        "Issue #815 评论 6042062633 修改 6: build_render_plan_full 不得再自己采样一次 caret，\
         必须复用 build_text_animation_plan_with_sample 返回的那一份采样。"
    );
    assert!(
        plan_window.contains("build_text_animation_plan_with_sample"),
        "Issue #815 评论 6042062633 修改 6: 文字层必须在同一个函数里完成唯一一次 caret 采样。"
    );
    assert!(
        plan_window.contains(
            "compute_coordinated_cursor_position(cursor_owner_epoch, &coordinated_motion_frame)"
        ),
        "Issue #815 评论 6042062633 修改 6: 光标层必须消费同一份 CoordinatedMotionFrame。"
    );

    let text_window = function_window(
        &src,
        "pub(crate) fn build_text_animation_plan_with_sample",
        9000,
    );
    assert!(
        text_window.contains("self.sample_coordinated_motion_frame("),
        "Issue #815 评论 6042062633 修改 6: 唯一一次 caret 采样在文字层入口，文字与光标共用它的返回值。"
    );

    // 吞吐字按 caret 帧算，只有 Timed unit 才走 current_visible_fraction。
    let glyph_window = text_window
        .find("for unit in &tx.units")
        .map(|i| text_window[i..].to_string())
        .unwrap_or_else(|| panic!("文字层必须有逐 unit 的 glyph 生成循环"));
    assert!(
        glyph_window.contains("is_caret_track()"),
        "Issue #815 评论 6042062633 修改 6: 逐 unit 渲染必须先按 CaretTrack / Timed 分流。"
    );
    assert!(
        glyph_window.contains("compute_frame_by_caret_ingest("),
        "Issue #815 评论 6042062633 修改 6: CaretTrack unit 必须调用按当前 caret 帧算裁切的入口。"
    );
    assert!(
        glyph_window.contains("current_visible_fraction"),
        "Issue #815 评论 6042062633 修改 6: 只有 Timed unit 保留独立 visible fraction 路径。"
    );

    // 完成条件：协同吞吐字随 cursor track 结束，不等一条自己的文字时间线。
    assert!(
        text_window.contains("!u.timing.is_caret_track()"),
        "Issue #815 评论 6042062633 修改 6: 事务完成判断必须把 CaretTrack unit 与 Timed unit 分开，\
         协同吞吐字随 cursor track 结束（它没有第二条进度可等）。"
    );
    assert!(
        text_window.contains("caret_track_done") && text_window.contains("caret_track_complete"),
        "Issue #815 评论 6042062633 修改 6: 事务完成仍必须等待本事务的 cursor track。"
    );

    // 失去 ownership 的事务必须收口 CaretTrack 单元。
    assert!(
        text_window.contains("retire_caret_driven_units()"),
        "Issue #815 评论 6042062633 修改 6: 拿不到本帧 caret 采样的事务必须把 CaretTrack 单元收口到终态。"
    );
}

// =========================================================================
// 修改点 4：builder 先建 track 再包 unit
// =========================================================================

#[test]
fn issue815_modify4_builder_marks_ingest_as_caret_track_after_track_exists() {
    let src = read_src("src/sujian_editor_item/animation/transaction_builder.rs");

    let track_pos = src
        .find("build_cursor_visual_track(")
        .expect("Issue #815 评论 6042062633 修改 4: builder 必须建 cursor_visual_track");
    let wrap_pos = src
        .find("PreparedVisualUnit::wrap_with_coordinated(")
        .expect("Issue #815 评论 6042062633 修改 4: InsertReveal/DeleteConceal 必须走 wrap_with_coordinated");
    let rebase_pos = src
        .find("match_rebase_frames(")
        .expect("Issue #815 评论 6042062633 修改 4: builder 仍要做 rebase 帧交棒");

    assert!(
        track_pos < wrap_pos,
        "Issue #815 评论 6042062633 修改 4: 必须先建 cursor_visual_track 再包 unit，\
         否则 wrap_with_coordinated 无从知道这笔事务到底有没有 caret 轨迹可依附。"
    );
    assert!(
        wrap_pos < rebase_pos,
        "Issue #815 评论 6042062633 修改 4: rebase 帧交棒仍在包 unit 之后。"
    );

    let window = function_window(
        &src,
        "let coordinated_ingest = spec.coordinated_animation_enabled",
        300,
    );
    assert!(
        window.contains("cursor_visual_track.is_some()"),
        "Issue #815 评论 6042062633 修改 4: 协同吞吐字的判定条件是「协同开启且这笔事务确实有 cursor track」，\
         不是只看设置开关。"
    );

    // 被删掉的根因注释/实现不得复活。
    for stale in [
        "InsertReveal/DeleteConceal 的文字 progress 只来自文字自己的 timeline",
        "coordinated 不再改 timing",
    ] {
        assert!(
            !src.contains(stale),
            "Issue #815 评论 6042062633 修改 4: 「{}」正是根因描述，必须删除。",
            stale
        );
    }

    // 协同却没有 track 时不允许入队，必须记正式事件。
    let skip_window = function_window(&src, "coordinated_without_cursor_track", 900);
    assert!(
        skip_window.contains("editor_animation_transaction_skipped_event"),
        "Issue #815 评论 6042062633 修改 7/8: 协同模式下没有 cursor track 时必须记正式跳过事件，\
         不允许退化成「文字自己播、光标不动」。"
    );
}

// =========================================================================
// 修改点 7/8/9：所有跳过点都有正式事件，输入路径闭环
// =========================================================================

#[test]
fn issue815_modify7_rebase_reports_skip_instead_of_silently_returning_none() {
    let src = read_src("src/sujian_editor_item/animation/rebase.rs");

    assert!(
        !src.contains("if coordinated_animation_enabled && !valid_caret_motion_track {\n            return None;"),
        "Issue #815 评论 6042062633 修改 7: 「协同但缺 caret 几何」不能再静默 return None，\
         这正是诊断包里只剩 Delete、没有 Insert 的原因。"
    );

    let window = function_window(&src, "pub(crate) fn prepare_rebase_handoff_for_edit", 9000);
    assert!(
        window.contains("editor_animation_transaction_skipped_event"),
        "Issue #815 评论 6042062633 修改 7: rebase handoff 的跳过点必须记正式事件。"
    );
    for cause in [
        "suppressed_by_context",
        "caret_geometry_missing",
        "missing_inserted_range",
    ] {
        assert!(
            window.contains(cause),
            "Issue #815 评论 6042062633 修改 7: 必须报出跳过原因 {}，不能吞掉原因。",
            cause
        );
    }
}

#[test]
fn issue815_modify8_input_path_close_the_loop_with_formal_events() {
    let skip_fields = function_window(
        &read_src("src/sujian_editor_item/mod.rs"),
        "pub(crate) struct AnimationSkipFields",
        2000,
    );
    for field in [
        "cause",
        "operation_kind",
        "typing_animation_enabled",
        "smooth_cursor_enabled",
        "coordinated_animation_enabled",
        "old_caret_present",
        "new_caret_present",
        "inserted_range",
        "unit_kinds",
        "cursor_track_present",
        "is_scrolling",
        "is_loading",
        "is_applying_format",
    ] {
        assert!(
            skip_fields.contains(field),
            "Issue #815 评论 6042062633 修改 8: 跳过事件必须带字段 {}，日志只暴露 cause 不足以定位问题。",
            field
        );
    }
    assert!(
        read_src("src/sujian_editor_item/mod.rs")
            .contains("event: \"editor.anim.transaction_skipped\".to_string()"),
        "Issue #815 评论 6042062633 修改 7/8: 正式事件名必须是 editor.anim.transaction_skipped。"
    );

    // 输入路径上的每个跳过点都要有正式事件。
    let cases: &[(&str, &[&str])] = &[
        (
            "src/sujian_editor_item/pipeline.rs",
            &[
                "suppressed_by_context",
                "stale_current_canonical",
                "canonical_invariant_failure",
            ],
        ),
        (
            "src/sujian_editor_item/transaction.rs",
            &["suppressed_by_scrolling"],
        ),
        (
            "src/sujian_editor_item/editing.rs",
            &[
                "composition_commit_old_snapshot_unavailable",
                "composition_commit_new_snapshot_invariant_failure",
            ],
        ),
    ];
    for (path, causes) in cases {
        let src = read_src(path);
        assert!(
            src.contains("editor_animation_transaction_skipped_event"),
            "Issue #815 评论 6042062633 修改 8: {} 必须用正式跳过事件解释「编辑发生了但没有动画」。",
            path
        );
        for cause in *causes {
            assert!(
                src.contains(cause),
                "Issue #815 评论 6042062633 修改 8: {} 必须报出 {}。",
                path,
                cause
            );
        }
    }
}

#[test]
fn issue815_modify9_ime_commit_uses_the_same_caret_driven_rule() {
    let src = read_src("src/sujian_editor_item/animation/composition.rs");

    assert!(
        !src.contains("prepared.cursor_visual_track.is_none()"),
        "Issue #815 评论 6042062633 修改 9: IME commit/update 不得再单独用「只有 debug log」的 gate 拦掉，\
         必须和普通 Insert 走同一条规则（builder 统一判定 + 正式事件）。"
    );
    for fn_marker in [
        "pub fn handle_composition_update",
        "pub fn handle_composition_commit_or_cancel",
    ] {
        let window = function_window(&src, fn_marker, 9000);
        assert!(
            window.contains("build_prepared_transaction(spec)?"),
            "Issue #815 评论 6042062633 修改 9: {} 必须直接消费 builder 的判定结果。",
            fn_marker
        );
    }
}

/// 评审收口定义必须留在代码注释里，防止语义再次漂移。
#[test]
fn issue815_closing_definition_is_preserved_in_comments() {
    let files = [
        "src/sujian_editor_item/animation/transaction/timeline.rs",
        "src/sujian_editor_item/animation/transaction/types.rs",
    ];
    for path in files {
        assert!(
            read_src(path).contains(CLOSING_DEFINITION),
            "Issue #815: {} 的注释里必须保留评审收口定义「{}」，\
             否则「两条互不相干的时间线」的错误语义会再漂回来。",
            path,
            CLOSING_DEFINITION
        );
    }
}

// =========================================================================
// 复核评论 5947230558 问题1：行身份只认当前这侧 canonical
// =========================================================================

/// 复核评论 5947230558 问题1。
///
/// `VisualLine.id` 每次 canonical 排版都从 0 按全文顺序重新编号。InsertReveal 的
/// slice 来自 new snapshot、DeleteConceal 来自 old snapshot，跨 revision 比大小
/// 会把 `caret_anchor_x` 放到错的行上。本测试锁住「行身份只认当前这侧」。
#[test]
fn issue815_review4_line_identity_never_crosses_snapshot_sides() {
    let slices_src = read_src("src/sujian_editor_item/animation/transaction_builder/slices.rs");

    // 问题1：不再把另一侧的 caret line id 传进来做锚点判定。
    assert!(
        !slices_src.contains("line_ordinal_for_line_top"),
        "Issue #815 评论 5947230558 问题2: 必须删除 line_ordinal_for_line_top。\
         相邻行满足 line1.bottom == line2.top，y 容差命中第一行，\
         .position() 会把 old caret 归到上一行，跨行 Backspace 的起点行整个错掉。"
    );
    assert!(
        slices_src.contains("fn line_ordinal_for_visual_line_id"),
        "Issue #815 评论 5947230558 问题2: 行序必须由本侧的 visual_line_id 精确查，\
         不再靠 y 容差猜。"
    );
    let ordinal_window = function_window(&slices_src, "fn line_ordinal_for_visual_line_id", 1200);
    assert!(
        ordinal_window.contains("line.visual_line_id == visual_line_id"),
        "Issue #815 评论 5947230558 问题2: 只能在**当前这侧** snapshot 内按 visual_line_id 精确匹配。"
    );
    assert!(
        !ordinal_window.contains("visual_line_top")
            && !ordinal_window.contains("visual_line_bottom"),
        "Issue #815 评论 5947230558 结语: 不再靠 y 容差猜行身份。"
    );

    // 问题1：Insert 锚点 = new 侧 ingest 起点行；Delete 锚点 = old 侧 ingest 终点行。
    let insert_window = function_window(
        &slices_src,
        "pub(crate) fn build_insert_reveal_slices",
        10400,
    );
    assert!(
        insert_window.contains("ingest_from_line_ord == Some(line_ord)"),
        "Issue #815 评论 5947230558 问题1: Insert 的 caret 锚点必须是 new snapshot 内\
         ingest 路径的**起点行**（吐字起点）。"
    );
    assert!(
        !insert_window.contains("caret_visual_line_id"),
        "Issue #815 评论 5947230558 问题1: build_insert_reveal_slices 不得再接收 \
         caret_visual_line_id——那是 old 侧的 id，拿来和 new 的行比就是跨 revision 比大小。"
    );

    let delete_window = function_window(
        &slices_src,
        "pub(crate) fn build_delete_conceal_slices",
        8800,
    );
    assert!(
        delete_window.contains("ingest_to_line_ord == Some(line_ord)"),
        "Issue #815 评论 5947230558 问题1: Delete 的 caret 锚点必须是 old snapshot 内\
         ingest 路径的**终点行**（吞字终点）。"
    );
    assert!(
        delete_window
            .contains("line_ordinal_for_visual_line_id(old_snapshot, old_cursor_visual_line_id)"),
        "Issue #815 评论 5947230558 问题2: Delete 的 ingest 起点行必须由 old 侧的 \
         old_cursor_visual_line_id 在 old snapshot 内精确查得。"
    );

    // Composition crossfade 是同一类 bug：一侧 id 同时比 old 行和 new 行。
    let crossfade_window = function_window(
        &slices_src,
        "pub(crate) fn build_composition_commit_crossfade_slices",
        9000,
    );
    assert!(
        crossfade_window.contains("old_cursor_visual_line_id"),
        "Issue #815 评论 5947230558 结语: Composition 路径的 old 侧行只能和 old 侧 \
         caret line id 比。"
    );
    assert!(
        crossfade_window.contains("new_cursor_visual_line_id"),
        "Issue #815 评论 5947230558 结语: Composition 路径的 new 侧行只能和 new 侧 \
         caret line id 比。"
    );
    assert!(
        !crossfade_window.contains("caret_visual_line_id.is_none_or"),
        "Issue #815 评论 5947230558 结语: 不允许再用一个 id 同时比 old 行和 new 行。"
    );
}

/// 复核评论 5947230558：builder 调用点也必须跟上。
#[test]
fn issue815_review4_builder_call_sites_pass_same_side_ids() {
    let builder_src = read_src("src/sujian_editor_item/animation/transaction_builder.rs");

    // 注意：Rust 字符串切片必须在 char boundary 上，不能直接 `pos + N`。
    let char_safe_window = |from: usize, bytes: usize| -> String {
        let mut end = (from + bytes).min(builder_src.len());
        while !builder_src.is_char_boundary(end) {
            end -= 1;
        }
        builder_src[from..end].to_string()
    };

    let insert_call = builder_src
        .find("build_insert_reveal_slices(")
        .expect("找不到 build_insert_reveal_slices 调用点");
    let insert_ctx = char_safe_window(insert_call, 1400);
    assert!(
        !insert_ctx.contains("spec.old_cursor_visual_line_id"),
        "Issue #815 评论 5947230558 问题1: build_insert_reveal_slices 不得再传 \
         spec.old_cursor_visual_line_id（old 侧 id 进 new 侧比较）。"
    );

    let delete_call = builder_src
        .find("build_delete_conceal_slices(")
        .expect("找不到 build_delete_conceal_slices 调用点");
    let delete_ctx = char_safe_window(delete_call, 1800);
    assert!(
        delete_ctx.contains("spec.old_cursor_visual_line_id"),
        "Issue #815 评论 5947230558 问题2: build_delete_conceal_slices 必须传 \
         spec.old_cursor_visual_line_id——old 侧的行身份只能来自 old 侧。"
    );
    assert!(
        !delete_ctx.contains("spec.new_cursor_visual_line_id"),
        "Issue #815 评论 5947230558 问题1: build_delete_conceal_slices 不得再传 \
         spec.new_cursor_visual_line_id（new 侧 id 进 old 侧比较）。"
    );
}

// =========================================================================
// 复核评论 5949097065：跨 snapshot 的空间坐标也必须同侧
// =========================================================================

/// 复核评论 5949097065 问题3：CursorTrack 不再是「from → to 一条直线」。
#[test]
fn issue815_review7_cursor_track_carries_formal_route_segments() {
    let src = read_src("src/sujian_editor_item/animation/transaction/types.rs");
    assert!(
        src.contains("pub(crate) enum CaretTrackSegmentKind"),
        "Issue #815 评论 5949097065 问题3: CursorTrack 必须有正式的路由段类型，\\
         跨 layout 编辑不能表达成 from.x -> to.x 一条直线。"
    );
    for kind in ["LayoutHandoff", "IngestLine", "RowHandoff"] {
        assert!(
            src.contains(kind),
            "Issue #815 评论 5949097065 问题3: 缺少路由段种类 {}。",
            kind
        );
    }
    assert!(
        src.contains("pub(crate) struct CaretTrackSegment"),
        "Issue #815 评论 5949097065 问题3: 缺少路由段数据结构。"
    );
    for field in [
        "pub kind: CaretTrackSegmentKind",
        "pub from: CursorRect",
        "pub to: CursorRect",
        "pub ingest_line_ord: Option<usize>",
    ] {
        assert!(
            src.contains(field),
            "Issue #815 评论 5949097065 问题3: CaretTrackSegment 缺少字段 {}。",
            field
        );
    }
    assert!(
        src.contains("pub segments: Vec<CaretTrackSegment>"),
        "Issue #815 评论 5949097065 问题3: PreparedCursorVisualTrack 必须持有路由段。"
    );
    assert!(
        src.contains("pub fn sampled_ingest_at_progress"),
        "Issue #815 评论 5949097065 问题3: 采样必须按路由累计进度给出当前行序，\\
         不能再用只懂 from/to 的 sampled_visual_line_id_at_progress。"
    );
    assert!(
        !src.contains("fn sampled_visual_line_id_at_progress("),
        "Issue #815 评论 5949097065 问题3: sampled_visual_line_id_at_progress 只懂 from/to，\\
         三行以上会把「已经过的中间行」全部误判成 to，必须删除。"
    );

    // render plan 必须把行序透传给文字层。
    let plan_src = read_src("src/sujian_editor_item/render_plan.rs");
    for field in [
        "pub ingest_line_ord: Option<usize>",
        "pub is_ingest_segment: bool",
    ] {
        assert!(
            plan_src.contains(field),
            "Issue #815 评论 5949097065 问题3: SampledCaretFrame 必须带 {}，\\
             文字层不该再自己从 y 猜「我在不在当前吞吐行」。",
            field
        );
    }
}

/// 复核评论 5949097065 问题1/问题2：吞吐几何必须来自 slice 自己那侧的 snapshot。
#[test]
fn issue815_review7_ingest_geometry_comes_from_the_slices_own_side() {
    let src = read_src("src/sujian_editor_item/animation/transaction_builder/slices.rs");
    assert!(
        src.contains("fn same_side_start_x("),
        "Issue #815 评论 5949097065 问题1/2: 吞吐锚点必须从本侧 snapshot 的真实 cluster \\
         rect 解出来，禁止按字节比例猜。"
    );
    let insert_window = function_window(&src, "pub(crate) fn build_insert_reveal_slices", 10400);
    assert!(
        insert_window
            .contains("same_side_start_x(new_snapshot, ingest_from_line_ord, range_start)"),
        "Issue #815 评论 5949097065 问题1: Insert 的吐字起点必须用 new snapshot 自己的 \\
         inserted_range.start 位置，不能塞 old_cursor_rect.x。"
    );
    let delete_window = function_window(&src, "pub(crate) fn build_delete_conceal_slices", 8800);
    assert!(
        delete_window.contains("same_side_start_x(old_snapshot, ingest_to_line_ord, range_start)"),
        "Issue #815 评论 5949097065 问题2: Delete 的吞字终点必须用 old snapshot 自己的 \\
         deleted_range.start 位置，不能拿 new_cursor_rect.x 当旧布局的锚点。"
    );

    // 事务构造顺序：切片 → assign_shared_line_masks → 生成路由 → 建 track。
    let builder_src = read_src("src/sujian_editor_item/animation/transaction_builder.rs");
    assert!(
        builder_src.contains("pub(crate) mod ingest_route;"),
        "Issue #815 评论 5949097065 问题3: 必须有独立的路由生成模块。"
    );
    let route_src =
        read_src("src/sujian_editor_item/animation/transaction_builder/ingest_route.rs");
    assert!(
        route_src.contains("pub(crate) fn build_ingest_route("),
        "Issue #815 评论 5949097065 问题3: 必须在建 track 之前由同侧切片生成 caret 路由。"
    );
    let route_pos = builder_src
        .find("let ingest_route_segments =")
        .expect("build_prepared_transaction 必须先算路由");
    let track_pos = builder_src
        .find("build_cursor_visual_track(")
        .expect("build_prepared_transaction 必须建 track");
    assert!(
        route_pos < track_pos,
        "Issue #815 评论 5949097065 问题3: 路由必须在 build_cursor_visual_track 之前生成，\\
         否则 track 又会退回 from -> to 直线。"
    );
}

/// 复核评论 5949097065 问题1：换位段不得冒充吞吐。
#[test]
fn issue815_review7_layout_handoff_does_not_fake_ingest() {
    let src = read_src("src/sujian_editor_item/animated_slice.rs");
    let window = function_window(&src, "pub(crate) fn compute_frame_by_caret_ingest", 8200);
    assert!(
        window.contains("IngestLinePhase::RouteBeforeStart"),
        "Issue #815 评论 5949097065 问题3: 拿不到权威行序（换位段）时，吞字/吐字切片必须\\
         保持初始状态，绝不能用换位的 x 去裁任何一行。"
    );
    assert!(
        window.contains("IngestLinePhase::FallbackFromCaretY"),
        "Issue #815 评论 5949097065 问题3: 退化路线（没有路由段）才允许降级用 caret.y。"
    );
    let phase_window = function_window(&src, "fn ingest_phase_from_route_ord", 2000);
    assert!(
        phase_window.contains("is_ingest_segment"),
        "Issue #815 评论 5949097065 问题3: 采样明确落在吞吐段上时该行才是当前行；\\
         落在换位段上时该行已收口，文字必须停在终态。"
    );
    // 只在「另一端怎么取」那一段里禁止 caret_anchor_x；整段窗口仍会因为
    // AnimatedSlice 的字段定义 / 构造函数 doc 出现该名字。
    let other_end_pos = window
        .find("let path_other_end_x")
        .expect("compute_frame_by_caret_ingest 必须计算吞吐路径的另一端");
    let other_end_end = window[other_end_pos..]
        .find("let phase =")
        .map(|i| other_end_pos + i)
        .unwrap_or(window.len());
    assert!(
        !window[other_end_pos..other_end_end].contains("caret_anchor_x"),
        "Issue #815 评论 5949097065 问题1: 吞吐路径的另一端必须来自行级 mask 与本帧 boundary，\
         不能再用跨 snapshot 写进来的 caret_anchor_x。"
    );
}
