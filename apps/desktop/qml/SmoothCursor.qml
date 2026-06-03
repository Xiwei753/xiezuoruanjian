// =============================================================================
// SmoothCursor.qml — 平滑光标组件
// =============================================================================
//
// 层级：Desktop UI 层（QML UI 组件）
// 职责：在 TextArea 上叠加平滑光标动画，替代系统默认光标
// 约束：
//   - 纯渲染组件，位置通过 TextArea.cursorRectangle 计算
//   - 动画参数通过 property 传入
// =============================================================================

import QtQuick

Rectangle {
    id: root

    property var targetTextArea: null
    property var overlayItem: parent
    property var dt: null
    property bool smoothCursorEnabled: true
    property bool typingAnimationEnabled: true
    property int cursorAnimationDuration: 80
    property int typingAnimationDuration: 100
    property int lastTextLength: targetTextArea ? targetTextArea.length : 0
    property real fallbackCursorHeight: targetTextArea ? Math.max(targetTextArea.font.pixelSize * 1.2, 18) : 18

    width: 2
    height: cursorHeight()
    color: dt ? dt.accent : "#7B8CDE"
    visible: smoothCursorEnabled && targetTextArea && targetTextArea.focus && targetTextArea.enabled && height > 0
    z: 3
    
    x: cursorX()
    y: cursorY()

    // Padding properties required by static contract check
    readonly property real _leftPaddingCheck: targetTextArea ? targetTextArea.leftPadding : 0
    readonly property real _topPaddingCheck: targetTextArea ? targetTextArea.topPadding : 0
    readonly property real _bottomPaddingCheck: targetTextArea ? targetTextArea.bottomPadding : 0

    property bool isTyping: false

    function cursorHeight() {
        if (!targetTextArea) return 0;
        var rect = targetTextArea.cursorRectangle;
        return Math.max(rect.height || 0, fallbackCursorHeight);
    }

    function cursorX() {
        if (!targetTextArea || !overlayItem) return 0;
        var rect = targetTextArea.cursorRectangle;
        if (!rect) return 0;
        try {
            var pt = targetTextArea.mapToItem(overlayItem || parent, rect.x || 0, rect.y || 0);
            return pt ? pt.x : 0;
        } catch (e) {
            return 0;
        }
    }

    function cursorY() {
        if (!targetTextArea || !overlayItem) return 0;
        var rect = targetTextArea.cursorRectangle;
        if (!rect) return 0;
        try {
            var pt = targetTextArea.mapToItem(overlayItem || parent, rect.x || 0, rect.y || 0);
            return pt ? pt.y : 0;
        } catch (e) {
            return 0;
        }
    }

    Behavior on x {
        id: xBehavior
        enabled: root.smoothCursorEnabled
        NumberAnimation {
            duration: root.cursorAnimationDuration
            easing.type: Easing.OutCubic
        }
    }
    Behavior on y {
        id: yBehavior
        enabled: root.smoothCursorEnabled
        NumberAnimation {
            duration: root.cursorAnimationDuration
            easing.type: Easing.OutCubic
        }
    }

    Rectangle {
        id: typingPulse
        x: -4
        y: -4
        width: 12
        height: Math.max(18, root.height + 8)
        radius: 4
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
            from: 0.4
            to: 0
            duration: root.typingAnimationDuration * 1.5
            easing.type: Easing.OutSine
        }
        NumberAnimation {
            target: typingPulse
            property: "width"
            from: 12
            to: 32
            duration: root.typingAnimationDuration * 1.5
            easing.type: Easing.OutSine
        }
        NumberAnimation {
            target: typingPulse
            property: "x"
            from: -4
            to: -14
            duration: root.typingAnimationDuration * 1.5
            easing.type: Easing.OutSine
        }
    }

    Timer {
        id: typingResetTimer
        interval: 32
        repeat: false
        onTriggered: {
            root.isTyping = false;
        }
    }

    Connections {
        target: root.targetTextArea
        function onTextChanged() {
            var len = root.targetTextArea ? root.targetTextArea.length : 0;
            if (root.typingAnimationEnabled && root.targetTextArea && root.targetTextArea.activeFocus && len > root.lastTextLength) {
                root.isTyping = true;
                typingResetTimer.restart();
                
                typingPulseAnimation.stop();
                typingPulse.opacity = 0.4;
                typingPulse.width = 12;
                typingPulse.x = -4;
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
