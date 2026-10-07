//! Issue #815 中仍有效的 QML 长按、输入事件和动画设置守卫。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/source_guard.rs"]
mod source_guard;

use source_guard::{function_window, read_src};

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
