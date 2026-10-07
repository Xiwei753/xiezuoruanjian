//! Issue #815 评论 6042062633 结构守卫 — 吞字/吐字真正由光标驱动的协同动画。
//!
//! WHITE_BOX 验证策略：确定性断言"协同动画退化成两条互不相干的时间线"的缺陷模式
//! 已在代码中消除，而不是靠肉眼回归。
//!
//! 一句话定义（评审收口，Issue #826 评论 38 沿用）：
//! `协同模式 = 一条 caret 运动轨迹 + 文字以 caret 当前帧为吞吐边界 + Reflow 可独立`
//! — 不是 `两条互不相干的时间线，只在起点位置看起来碰巧挨着`。
//!
//! Issue #826 评论 38：旧的 transaction / timeline / cursor_motion /
//! animated_slice / rebase 架构已删除，协同改由 `animation/coordinated_caret.rs`
//! 的单份 `CoordinatedCaretMotion` 实现（与前沿同 started_at / typing duration，
//! 边界由本帧 caret 投影到 Frontier path）。本文件的旧守卫已按新架构重写，
//! 只保留仍然有效的部分（QML 长按选词、正式跳过事件、协同/打字时长共享值）。
//!
//! 行为级证明（同一 progress 下 caret.x == 吞吐边界 x、跨行同帧、retarget 单
//! motion）在 lib 内 `runtime_tests.rs` 的 6 个 `coordinated_*` 测试里；
//! 这里只守"结构不被改回两条独立时间线"。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/source_guard.rs"]
mod source_guard;

use source_guard::{function_window, read_src};

/// 评审收口定义。代码注释里必须留着这句，避免以后又被改回"两条独立时间线"。
const CLOSING_DEFINITION: &str =
    "一条 caret 运动轨迹 + 文字以 caret 当前帧为吞吐边界 + Reflow 可独立";

// =========================================================================
// 修改点 1：恢复桌面鼠标左键长按选词（保留，仍有效）
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

    // Issue #819 评论 5967250411 问题 4：leftButtonLongPressHandler (TapHandler) 已删除。
    // 左键 pointer event 的唯一 owner 是 qquickitem_impl mouse_event。
    // QML 只保留一个不绑 pointer event 的 Timer，由 Rust property 控制启停。
    assert!(
        !src.contains("id: leftButtonLongPressHandler"),
        "Issue #819 评论 5967250411 问题 4: leftButtonLongPressHandler (TapHandler) \
         必须删除，左键 pointer event 单一 owner 是 qquickitem_impl mouse_event。"
    );

    // 新 leftButtonLongPressTimer 必须存在，且不绑 pointer event。
    assert!(
        src.contains("id: leftButtonLongPressTimer"),
        "Issue #819 评论 5967250411 问题 4: WritingWorkspace.qml 必须有 \
         leftButtonLongPressTimer（不绑 pointer event 的独立 Timer）。"
    );

    // Timer.running 绑定 Rust property long_press_timer_active。
    assert!(
        src.contains("long_press_timer_active"),
        "Issue #819 评论 5967250411 问题 4: Timer.running 必须绑定 \
         sujianEditor.long_press_timer_active，由 Rust mouse_event 控制启停。"
    );

    // Issue #819 评论 5967250411 问题 4：新链路用 Timer 调
    // activate_pointer_long_press（Rust 状态机 activate_long_press + long_press_at）。
    assert!(
        src.contains("activate_pointer_long_press"),
        "Issue #819 评论 5967250411 问题 4: QML Timer 必须调 \
         activate_pointer_long_press。"
    );

    // Issue #819 评论 5967250411 问题 4：QML 不再调 end_selection_gesture_qml。
    // release/cancel 的 selection gesture 结束只由 qquickitem_impl mouse_event 做一次。
    assert!(
        !src.contains("end_selection_gesture_qml"),
        "Issue #819 评论 5967250411 问题 4: QML 不再调 end_selection_gesture_qml，\
         release/cancel 的 selection gesture 结束只由 qquickitem_impl mouse_event 做一次。"
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
// Issue #826 评论 38：协同改由单份 CoordinatedCaretMotion 实现后的结构守卫
// =========================================================================
//
// 旧 transaction / timeline / cursor_motion / animated_slice / rebase 文件已删，
// 原来指向它们的守卫按评论 36「清理旧守卫」先例重写到新架构上。

#[test]
fn issue826_c38_closing_definition_preserved() {
    let src = read_src("src/sujian_editor_item/animation/coordinated_caret.rs");
    assert!(
        src.contains(CLOSING_DEFINITION),
        "Issue #826 评论 38: 协同收口定义必须留在协同 motion 模块注释里，\
         避免以后又被改回两条独立时间线。"
    );
}

#[test]
fn issue826_c38_single_motion_no_queue() {
    let src = read_src("src/sujian_editor_item/animation/coordinator.rs");
    let struct_window = function_window(
        &src,
        "pub(crate) struct LinuxEditorAnimationCoordinator",
        2500,
    );
    assert!(
        struct_window.contains("active_coordinated_caret: Option<CoordinatedCaretMotion>"),
        "Issue #826 评论 38: 协同 motion 必须只有一份 Option（数量恒 ≤ 1），\
         不存在 per-key queue。"
    );
    for forbidden in [
        "Vec<CoordinatedCaretMotion",
        "VecDeque<CoordinatedCaretMotion",
    ] {
        assert!(
            !src.contains(forbidden),
            "Issue #826 评论 38: coordinator 里不得出现 {}，协同不排历史队列。",
            forbidden
        );
    }
}

#[test]
fn issue826_c38_motion_shares_frontier_clock() {
    let src = read_src("src/sujian_editor_item/animation/coordinator.rs");
    let window = function_window(
        &src,
        "pub(crate) fn begin_or_retarget_coordinated_caret",
        4500,
    );
    assert!(
        window.contains("frontier.started_at") && window.contains("frontier.duration_ms"),
        "Issue #826 评论 38: motion 的 started_at / duration_ms 必须直接取当前前沿 \
         （与前沿同一时钟），不能自己另取 Instant::now()、不能用 smooth cursor duration。"
    );
    assert!(
        !window.contains("cursor_animation_duration_ms"),
        "Issue #826 评论 38: 协同 motion 时长永远是 typing duration（前沿的），\
         不得引用 smooth cursor duration。"
    );
    assert!(
        !window.contains("Instant::now()"),
        "Issue #826 评论 38: motion 不得自己另取时间起点（启动时刻分叉正是评论 38 \
         第 4 点指出的问题）。"
    );
}

#[test]
fn issue826_c38_retarget_samples_old_motion_first() {
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
        "Issue #826 评论 38/39: 交棒必须先采样旧 motion 当前帧的轨迹距离 \
         （屏幕真相），不退回调用方可能滞后一帧的 visual。"
    );
}

#[test]
fn issue826_c38_frontier_sample_carries_projection() {
    let src = read_src("src/sujian_editor_item/animation/coordinator.rs");
    let window = function_window(&src, "pub(crate) fn sample_edit_frontier", 2500);
    assert!(
        window.contains("motion.sample_at_distance(motion.distance_at_progress(progress))")
            && window.contains("frontier_distance")
            && window.contains("CoordinatedBoundary"),
        "Issue #826 评论 38/39/41: 每帧先由同一 progress 算 caret 在分段轨迹上的 \
         位置与所属文字段的前沿距离（Connector 段冻结），遮罩与 overlay 吃该距离。"
    );
    assert!(
        window.contains("frontier.reveal.advanced(progress)")
            || window.contains("frontier.conceal.advanced(progress)"),
        "Issue #826 评论 38: 投影未命中行时必须回退到 progress 时钟，不能留 None。"
    );
}

#[test]
fn issue826_c38_masks_and_overlays_consume_projection() {
    let src = read_src("src/sujian_editor_item/animation/edit_frontier.rs");
    let sample_window = function_window(&src, "pub(crate) struct EditFrontierSample", 1200);
    assert!(
        sample_window.contains("coordinated: Option<CoordinatedBoundary>"),
        "Issue #826 评论 38: 前沿 sample 必须携带协同投影边界。"
    );
    let mask_window = function_window(&src, "fn hidden_new_text_rects", 1500);
    assert!(
        mask_window.contains("reveal_distance(region, sample)"),
        "Issue #826 评论 38: 吐字遮罩必须吃投影距离，不能再用 distance_at(region, sample.progress)。"
    );
    assert!(
        !mask_window.contains("distance_at(region, sample.progress)"),
        "Issue #826 评论 38: 吐字遮罩里不得残留 progress 时钟（两条时钟会分叉）。"
    );
    let overlay_window = function_window(&src, "fn old_overlay_glyphs", 1500);
    assert!(
        overlay_window.contains("conceal_distance(region, sample)"),
        "Issue #826 评论 38: 吞字 overlay 必须吃投影距离。"
    );
}

#[test]
fn issue826_c38_projection_is_caret_driven_no_glyph_inference() {
    let src = read_src("src/sujian_editor_item/animation/coordinated_caret.rs");
    let window = function_window(&src, "pub(crate) fn project_onto_layer", 2500);
    assert!(
        window.contains("x: f64,") && window.contains("y: f64,"),
        "Issue #826 评论 38: 投影输入必须是本帧 caret 的 (x, y)。"
    );
    assert!(
        window.contains("segment.y") && window.contains("x_from") && window.contains("distance_start + prefix"),
        "Issue #826 评论 38: 投影必须按 caret.y 找同行段、按 x 比例算距离。"
    );
    for forbidden in ["rightmost", "conceal_edge", "infer"] {
        assert!(
            !src.contains(forbidden),
            "Issue #826 评论 38: 协同投影不得从 glyph 反推光标（{}），方向反了。",
            forbidden
        );
    }
}

#[test]
fn issue826_c38_frame_samples_coordinated_before_text_tick() {
    let src = read_src("src/sujian_editor_item/qquickitem_impl.rs");
    let window = function_window(&src, "fn update_paint_node", 6000);
    let coordinated_pos = window
        .find("tick_coordinated_caret_with_time(frame_now)")
        .expect("协同采样必须在 update_paint_node 里");
    let tick_pos = window
        .find("tick_text_animations_with_time(frame_now)")
        .expect("正文 tick 必须在 update_paint_node 里");
    assert!(
        coordinated_pos < tick_pos,
        "Issue #826 评论 38: 协同 caret 必须先采样（到终点精确落 target），\
         再 tick 正文层收前沿；反向会留一帧 0.99 亚像素残留。"
    );
    assert!(
        window.contains("if !coordinated_owned_this_frame"),
        "Issue #826 评论 38: 协同接管的帧不得再调独立 tick_animation \
         （同一帧 visual 只写一次）。"
    );
}

#[test]
fn issue826_c38_no_independent_tween_while_coordinated() {
    let rendering = read_src("src/sujian_editor_item/rendering.rs");
    let follow_window = function_window(
        &rendering,
        "pub(crate) fn apply_coordinated_cursor_follow",
        3500,
    );
    assert!(
        follow_window.contains("animation = None"),
        "Issue #826 评论 38: 协同跟随态不得建独立 Tween（animation 保持 None，\
         推进只走 motion 采样）。"
    );
    let update_window = function_window(
        &rendering,
        "pub(crate) fn update_cursor_visual_position",
        6000,
    );
    assert!(
        update_window.contains("begin_or_retarget_coordinated_caret")
            && update_window.contains("clear_coordinated_caret"),
        "Issue #826 评论 38: 光标更新必须分协同接管 / 独立回退两支，独立分支要清 motion。"
    );
}

#[test]
fn issue826_c38_motion_lifecycle_follows_frontier() {
    let src = read_src("src/sujian_editor_item/animation/coordinator.rs");
    let finish_window = function_window(
        &src,
        "pub(crate) fn finish_edit_frontier_to_canonical",
        1200,
    );
    assert!(
        finish_window.contains("active_coordinated_caret = None"),
        "Issue #826 评论 38: 前沿收成 canonical 时 motion 必须一起清（同生共死）。"
    );
    let resume_window = function_window(&src, "pub(crate) fn resume_all", 1500);
    assert!(
        resume_window.contains("motion.shift_started_at(delta)"),
        "Issue #826 评论 38: 滚动 resume 必须连 motion 一起平移 started_at，\
         否则光标与吞吐边界立刻分叉。"
    );
    // tick 不得清 motion（零可见 path 时前沿第一帧即 finished，motion 必须活到
    // typing 结束）：见 issue826_c39_motion_outlives_empty_frontier。
}

#[test]
fn issue826_c39_motion_outlives_empty_frontier() {
    let src = read_src("src/sujian_editor_item/animation/coordinator.rs");
    let tick_window = function_window(&src, "pub(crate) fn tick(", 2000);
    assert!(
        !tick_window.contains("active_coordinated_caret = None"),
        "Issue #826 评论 39 BLOCKER 1: tick 不得因前沿没了就清 motion——\
         零可见 path 的编辑（Enter / shaping 全接管）第一帧前沿即 finished，\
         motion 必须活到 typing duration 结束，否则协同光标只动一帧就停。"
    );
    assert!(
        tick_window.contains("active_coordinated_caret.is_some()"),
        "Issue #826 评论 39 BLOCKER 1: motion 自己就是一层 clock，\
         tick 续帧条件必须包含它。"
    );
}

#[test]
fn issue826_c39_frame_requests_continue_while_motion_alive() {
    let src = read_src("src/sujian_editor_item/qquickitem_impl.rs");
    assert!(
        src.contains("has_active_coordinated_caret()"),
        "Issue #826 评论 39 BLOCKER 1: 尾部续帧条件必须包含协同 motion，\
         否则 motion 活着却没有帧可跑，光标一样定住。"
    );
}

#[test]
fn issue826_c39_caret_walks_piecewise_path() {
    let caret_src = read_src("src/sujian_editor_item/animation/coordinated_caret.rs");
    assert!(
        caret_src.contains("pub(crate) fn position_at_distance"),
        "Issue #826 评论 39 BLOCKER 2: caret 必须沿分段轨迹按距离行走。"
    );
    assert!(
        !caret_src.contains("start_x + (self.target_x - self.start_x)"),
        "Issue #826 评论 39 BLOCKER 2: 不得对起点终点拉 x/y 斜线——\
         跨行斜线会穿过行间缝隙，那里不属于任何文字行。"
    );
    let coord_src = read_src("src/sujian_editor_item/animation/coordinator.rs");
    let window = function_window(&coord_src, "fn coordinated_path_from_frontier", 6000);
    assert!(
        window.contains("reveal.regions") && window.contains("conceal.regions"),
        "Issue #826 评论 39/41: caret 路径必须取自本笔前沿分段 \
         （吐字侧优先，纯吞字走吞字侧）。"
    );
    assert!(
        window.contains("CaretSegmentKind::Boundary") && window.contains("CaretSegmentKind::Connector"),
        "Issue #826 评论 41: 轨迹必须区分「文字边界段」与「纯光标 connector」，\
         文字段的视觉行几何不得被 start/target 钉成斜线。"
    );
    assert!(
        window.contains("y_from: caret_y") && window.contains("y_to: caret_y"),
        "Issue #826 评论 41: Boundary 段必须保持 y_from == y_to（同视觉行）。"
    );
}

#[test]
fn issue826_c39_projection_prefers_x_match() {
    let src = read_src("src/sujian_editor_item/animation/coordinated_caret.rs");
    let window = function_window(&src, "pub(crate) fn project_onto_layer", 3000);
    assert!(
        window.contains("nearest"),
        "Issue #826 评论 39 BLOCKER 3: 同行多 region 时必须先收齐候选、\
         优先选 x 真正命中的段，落空才取最近——绝不能 first-y-match 就返回。"
    );
}

#[test]
fn issue826_c39_blink_suppressed_while_motion_alive() {
    let src = read_src("src/sujian_editor_item/properties.rs");
    let window = function_window(&src, "pub(crate) fn current_cursor_blink_mode", 1500);
    assert!(
        window.contains("has_active_coordinated_caret()"),
        "Issue #826 评论 39 BLOCKER 1: 零可见 path 时前沿第一帧就没了，\
         blink 抑制必须跟着 motion 走，否则协同中途光标闪烁。"
    );
}

/// Issue #826 评论 42：跳行 Boundary 的 drawn caret y 必须用 Qt 真实 caret top，
/// 不能再用 `FrontierSegment.y`（= visual_line_top）或 `same_row ? start_y : ...` 猜。
#[test]
fn issue826_c42_boundary_uses_qt_caret_top_not_visual_line_top() {
    let layout = read_src("src/sujian_editor_item/layout_snapshot.rs");
    assert!(
        layout.contains("pub caret_top: f64") && layout.contains("pub caret_height: f64"),
        "Issue #826 评论 42: PreparedLineSnapshot 必须携带 Qt 真实 caret top/height。"
    );
    let builder = read_src("src/sujian_editor_item/line_snapshot_builder.rs");
    assert!(
        builder.contains("cursor_rect_for_line("),
        "Issue #826 评论 42: 构造 PreparedLineSnapshot 时必须用 cursor_rect_for_line \
         计算真实 caret top，不能拿 line.y + 常数。"
    );
    let coord = read_src("src/sujian_editor_item/animation/coordinator.rs");
    let window = function_window(&coord, "fn boundary_segments", 2500);
    assert!(
        window.contains("caret_top"),
        "Issue #826 评论 42: boundary_segments 的 drawn caret y 必须取该行 caret_top。"
    );
    assert!(
        !window.contains("same_row"),
        "Issue #826 评论 42: 不得再用 same_row ? start_y : segment.y 猜 caret y。"
    );
}
