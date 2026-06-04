// =============================================================================
// SmoothCursor.qml — 平滑光标组件
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

    property bool hasSelection: targetTextArea ? (targetTextArea.selectedText.length > 0) : false

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

    function updateCursorRect() {
        if (!targetTextArea || isScrolling || scrollDebounceTimer.running) return;
        cursorRect.x = root.cursorX();
        cursorRect.y = root.cursorY();
        cursorRect.height = root.cursorHeight();
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

    Timer {
        id: scrollDebounceTimer
        interval: 200
        repeat: false
        onTriggered: {
            cursorRect.xBehaviorEnabled = false;
            cursorRect.yBehaviorEnabled = false;
            updateCursorRect();
            Qt.callLater(function() {
                cursorRect.xBehaviorEnabled = true;
                cursorRect.yBehaviorEnabled = true;
            });
        }
    }

    onIsScrollingChanged: {
        if (isScrolling) {
            scrollDebounceTimer.stop();
            if (typingParticlesModel.count > 0) {
                typingParticlesModel.clear();
            }
        } else {
            scrollDebounceTimer.restart();
        }
    }

    Connections {
        target: root.targetTextArea
        function onCursorRectangleChanged() {
            updateCursorRect();
        }
        function onTextChanged() {
            var len = root.targetTextArea ? root.targetTextArea.length : 0;
            var newText = root.targetTextArea ? root.targetTextArea.text : "";
            var isComposing = root.targetTextArea && root.targetTextArea.inputMethodComposing;
            
            if (root.typingAnimationEnabled && !root.isScrolling && !isComposing && root.targetTextArea && root.targetTextArea.activeFocus) {
                var prefix = commonPrefixLength(root.lastTextString, newText);
                var suffix = commonSuffixLength(root.lastTextString, newText, prefix);
                var addedText = newText.substring(prefix, newText.length - suffix);
                var deletedText = root.lastTextString.substring(prefix, root.lastTextString.length - suffix);

                // 只对短输入生成粒子，避开大段粘贴、章节加载、一键排版
                if (addedText.length > 0 && addedText.length <= 3 && deletedText.length === 0) {
                    for (var addIndex = 0; addIndex < addedText.length; addIndex++) {
                        appendTypingParticle(addedText.charAt(addIndex), prefix + addIndex, false);
                    }
                } else if (deletedText.length > 0 && deletedText.length <= 3 && addedText.length === 0) {
                    for (var delIndex = 0; delIndex < deletedText.length; delIndex++) {
                        appendTypingParticle(deletedText.charAt(delIndex), Math.min(prefix, newText.length), true);
                    }
                }
            }
            root.lastTextLength = len;
            root.lastTextString = newText;
        }
    }

    Rectangle {
        id: cursorRect
        width: 2
        height: root.fallbackCursorHeight
        color: root.dt ? root.dt.editorText : "#E2E2E5"
        visible: root.smoothCursorEnabled && root.targetTextArea && root.targetTextArea.focus && root.targetTextArea.enabled && height > 0 && !root.isScrolling && !scrollDebounceTimer.running && !root.hasSelection
        z: 3
        
        property bool xBehaviorEnabled: true
        property bool yBehaviorEnabled: true

        Behavior on x {
            enabled: root.smoothCursorEnabled && cursorRect.xBehaviorEnabled
            NumberAnimation {
                duration: root.cursorAnimationDuration
                easing.type: Easing.OutCubic
            }
        }
        Behavior on y {
            enabled: root.smoothCursorEnabled && cursorRect.yBehaviorEnabled
            NumberAnimation {
                duration: root.cursorAnimationDuration
                easing.type: Easing.OutCubic
            }
        }

        SequentialAnimation on opacity {
            loops: Animation.Infinite
            running: cursorRect.visible
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
                color: root.targetTextArea ? root.targetTextArea.color : (root.dt ? root.dt.editorText : "#E2E2E5")
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
}
