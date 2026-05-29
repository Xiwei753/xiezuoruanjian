// =============================================================================
// SmoothCursor.qml — 平滑光标组件
// =============================================================================
//
// 层级：Linux UI 层（QML UI 组件）
// 职责：在 TextArea 上叠加平滑光标动画，替代系统默认光标
// 约束：
//   - 纯渲染组件，位置通过 TextArea.cursorRectangle 计算
//   - 动画参数通过 property 传入
// =============================================================================

import QtQuick

Rectangle {
    id: root

    property var targetTextArea: null
    property var dt: null
    property bool smoothCursorEnabled: true
    property bool typingAnimationEnabled: true
    property int cursorAnimationDuration: 80
    property int typingAnimationDuration: 100
    property int lastTextLength: targetTextArea ? targetTextArea.length : 0

    width: 2
    height: targetTextArea ? targetTextArea.cursorRectangle.height : 0
    color: dt ? dt.accent : "#7B8CDE"
    visible: smoothCursorEnabled && targetTextArea && targetTextArea.focus && targetTextArea.enabled && targetTextArea.cursorRectangle.height > 0
    z: 3
    
    x: targetTextArea ? targetTextArea.cursorRectangle.x : 0
    y: targetTextArea ? targetTextArea.cursorRectangle.y : 0

    Behavior on x {
        NumberAnimation {
            duration: root.cursorAnimationDuration
            easing.type: Easing.OutCubic
        }
    }
    Behavior on y {
        NumberAnimation {
            duration: root.cursorAnimationDuration
            easing.type: Easing.OutCubic
        }
    }

    Rectangle {
        id: typingPulse
        x: -8
        y: -4
        width: 28
        height: Math.max(18, root.height + 8)
        radius: height / 2
        color: root.dt ? root.dt.accent : "#7B8CDE"
        opacity: 0
        visible: opacity > 0
        z: -1
    }

    ParallelAnimation {
        id: typingPulseAnimation
        NumberAnimation {
            target: typingPulse
            property: "opacity"
            from: 0.22
            to: 0
            duration: root.typingAnimationDuration
            easing.type: Easing.OutCubic
        }
        NumberAnimation {
            target: typingPulse
            property: "y"
            from: -7
            to: -4
            duration: root.typingAnimationDuration
            easing.type: Easing.OutCubic
        }
    }

    Connections {
        target: root.targetTextArea
        function onTextChanged() {
            var len = root.targetTextArea ? root.targetTextArea.length : 0;
            if (root.typingAnimationEnabled && root.targetTextArea && root.targetTextArea.activeFocus && len > root.lastTextLength) {
                typingPulseAnimation.stop();
                typingPulse.opacity = 0.22;
                typingPulse.y = -7;
                typingPulseAnimation.start();
            }
            root.lastTextLength = len;
        }
    }

    // Breathing pulse animation when idle
    SequentialAnimation on opacity {
        loops: Animation.Infinite
        running: root.smoothCursorEnabled && targetTextArea && targetTextArea.focus
        NumberAnimation { from: 1.0; to: 0.2; duration: 600; easing.type: Easing.InOutQuad }
        NumberAnimation { from: 0.2; to: 1.0; duration: 600; easing.type: Easing.InOutQuad }
    }
}
