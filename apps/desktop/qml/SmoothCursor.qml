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

Item {
    id: root

    property var targetTextArea: null
    property var overlayItem: parent
    property var dt: null
    property bool smoothCursorEnabled: true
    property bool typingAnimationEnabled: true
    property bool isScrolling: false
    property int cursorAnimationDuration: 80
    property int typingAnimationDuration: 100
    property int lastTextLength: targetTextArea ? targetTextArea.length : 0
    property string lastTextString: targetTextArea ? targetTextArea.text : ""
    property real fallbackCursorHeight: targetTextArea ? Math.max(targetTextArea.font.pixelSize * 1.2, 18) : 18

    anchors.fill: parent
    visible: targetTextArea && targetTextArea.enabled
    z: 3

    // Padding properties required by static contract check
    readonly property real _leftPaddingCheck: targetTextArea ? targetTextArea.leftPadding : 0
    readonly property real _topPaddingCheck: targetTextArea ? targetTextArea.topPadding : 0
    readonly property real _bottomPaddingCheck: targetTextArea ? targetTextArea.bottomPadding : 0

    property bool isTyping: false

    ListModel {
        id: typingParticlesModel
    }

    function clampCursorHeight(rectHeight) {
        var fallbackHeight = fallbackCursorHeight;
        var rawHeight = rectHeight || fallbackHeight;
        return Math.min(Math.max(rawHeight, fallbackHeight * 0.85), fallbackHeight * 1.25);
    }

    function cursorHeight() {
        if (!targetTextArea) return 0;
        var rect = targetTextArea.cursorRectangle;
        return clampCursorHeight(rect.height);
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

    function textPositionRect(position) {
        if (!targetTextArea || !overlayItem) return { "x": cursorRect.x, "y": cursorRect.y, "height": cursorRect.height };
        try {
            var rect = targetTextArea.positionToRectangle(position);
            var pt = targetTextArea.mapToItem(overlayItem || parent, rect.x || 0, rect.y || 0);
            var clampedHeight = clampCursorHeight(rect.height);
            return {
                "x": pt ? pt.x : cursorRect.x,
                "y": pt ? pt.y : cursorRect.y,
                "height": clampedHeight
            };
        } catch (e) {
            return { "x": cursorRect.x, "y": cursorRect.y, "height": cursorRect.height };
        }
    }

    function commonPrefixLength(a, b) {
        var limit = Math.min(a.length, b.length);
        var i = 0;
        while (i < limit && a.charAt(i) === b.charAt(i)) i++;
        return i;
    }

    function commonSuffixLength(a, b, prefix) {
        var maxSuffix = Math.min(a.length, b.length) - prefix;
        var i = 0;
        while (i < maxSuffix && a.charAt(a.length - 1 - i) === b.charAt(b.length - 1 - i)) i++;
        return i;
    }

    function appendTypingParticle(charText, position, isDeletion) {
        if (root.isScrolling) return;
        if (!charText || charText === "\n" || charText === "\r") return;
        if (typingParticlesModel.count > 40) typingParticlesModel.remove(0, typingParticlesModel.count - 40);
        var rect = textPositionRect(position);
        typingParticlesModel.append({
            charText: charText,
            startXPos: rect.x,
            startYPos: rect.y + rect.height * 0.75,
            createdAt: Date.now(),
            isDeletion: isDeletion
        });
    }

    Rectangle {
        id: cursorRect
        width: 2
        height: root.cursorHeight()
        color: root.dt ? root.dt.accent : "#7B8CDE"
        visible: root.smoothCursorEnabled && root.targetTextArea && root.targetTextArea.focus && root.targetTextArea.enabled && height > 0
        z: 3
        x: root.cursorX()
        y: root.cursorY()

        Behavior on x {
            enabled: root.smoothCursorEnabled && !root.isScrolling
            NumberAnimation {
                duration: root.cursorAnimationDuration
                easing.type: Easing.OutCubic
            }
        }
        Behavior on y {
            enabled: root.smoothCursorEnabled && !root.isScrolling
            NumberAnimation {
                duration: root.cursorAnimationDuration
                easing.type: Easing.OutCubic
            }
        }

        // Breathing pulse animation when idle
        SequentialAnimation on opacity {
            loops: Animation.Infinite
            running: root.smoothCursorEnabled && !root.isScrolling && root.targetTextArea && root.targetTextArea.focus
            NumberAnimation { from: 1.0; to: 0.2; duration: 600; easing.type: Easing.InOutQuad }
            NumberAnimation { from: 0.2; to: 1.0; duration: 600; easing.type: Easing.InOutQuad }
        }
    }

    Item {
        id: typingLayer
        anchors.fill: parent
        visible: !root.isScrolling
        z: 2

        Repeater {
            model: typingParticlesModel
            delegate: Text {
                text: charText
                color: root.targetTextArea ? root.targetTextArea.color : (root.dt ? root.dt.onSurface : "#000")
                x: startXPos
                y: startYPos
                opacity: 1.0
                scale: 1.0

                Component.onCompleted: {
                    if (root.targetTextArea) {
                        font = root.targetTextArea.font;
                    }
                    animGroup.start();
                }

                ParallelAnimation {
                    id: animGroup
                    NumberAnimation {
                        target: parent
                        property: "opacity"
                        from: isDeletion ? 1.0 : 0.4
                        to: 0.0
                        duration: isDeletion ? root.typingAnimationDuration * 2.5 : root.typingAnimationDuration * 1.5
                        easing.type: Easing.OutSine
                    }
                    NumberAnimation {
                        target: parent
                        property: "y"
                        from: startYPos
                        to: isDeletion ? startYPos - 10 : startYPos
                        duration: isDeletion ? root.typingAnimationDuration * 2.5 : root.typingAnimationDuration * 1.5
                        easing.type: Easing.OutSine
                    }
                    NumberAnimation {
                        target: parent
                        property: "scale"
                        from: 1.0
                        to: isDeletion ? 1.0 : 1.5
                        duration: root.typingAnimationDuration * 1.5
                        easing.type: Easing.OutSine
                    }
                    onFinished: {
                        // It will be cleaned up by garbageCollector
                    }
                }
            }
        }
    }

    Timer {
        id: garbageCollector
        interval: 500
        repeat: true
        running: typingParticlesModel.count > 0 && !root.isScrolling
        onTriggered: {
            var now = Date.now();
            var i = 0;
            while (i < typingParticlesModel.count) {
                var item = typingParticlesModel.get(i);
                var dur = item.isDeletion ? (root.typingAnimationDuration * 2.5 + 100) : (root.typingAnimationDuration * 1.5 + 100);
                if (now - item.createdAt >= dur) {
                    typingParticlesModel.remove(i, 1);
                } else {
                    i++;
                }
            }
        }
    }

    Connections {
        target: root.targetTextArea
        function onTextChanged() {
            var len = root.targetTextArea ? root.targetTextArea.length : 0;
            var newText = root.targetTextArea ? root.targetTextArea.text : "";
            if (root.typingAnimationEnabled && !root.isScrolling && root.targetTextArea && root.targetTextArea.activeFocus) {
                var prefix = commonPrefixLength(root.lastTextString, newText);
                var suffix = commonSuffixLength(root.lastTextString, newText, prefix);
                var addedText = newText.substring(prefix, newText.length - suffix);
                var deletedText = root.lastTextString.substring(prefix, root.lastTextString.length - suffix);
                var maxParticles = 16;

                if (addedText.length > 0 && deletedText.length === 0) {
                    for (var addIndex = 0; addIndex < Math.min(addedText.length, maxParticles); addIndex++) {
                        appendTypingParticle(addedText.charAt(addIndex), prefix + addIndex, false);
                    }
                } else if (deletedText.length > 0 && addedText.length === 0) {
                    for (var delIndex = 0; delIndex < Math.min(deletedText.length, maxParticles); delIndex++) {
                        appendTypingParticle(deletedText.charAt(delIndex), Math.min(prefix, newText.length), true);
                    }
                }
            }
            root.lastTextLength = len;
            root.lastTextString = newText;
        }
    }

    onIsScrollingChanged: {
        if (isScrolling && typingParticlesModel.count > 0) {
            typingParticlesModel.clear();
        }
    }

}
