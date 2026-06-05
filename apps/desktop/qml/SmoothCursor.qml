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
    property bool isScrolling: false
    property int cursorAnimationDuration: 80
    property real fallbackCursorHeight: targetTextArea ? Math.max(targetTextArea.font.pixelSize * 1.2, 18) : 18

    property bool hasSelection: targetTextArea ? (targetTextArea.selectedText.length > 0) : false
    property bool snapNextUpdate: true
    property real smoothUntilMs: 0

    anchors.fill: parent
    visible: targetTextArea && targetTextArea.enabled
    z: 3

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

    function allowSmoothCursorMotion() {
        root.smoothUntilMs = Date.now() + Math.max(120, root.cursorAnimationDuration + 80);
        root.snapNextUpdate = false;
    }

    function snapNextCursorUpdate() {
        root.snapNextUpdate = true;
        root.smoothUntilMs = 0;
    }

    function updateCursorRect(forceSnap) {
        if (!targetTextArea || isScrolling || scrollDebounceTimer.running) return;
        var newX = root.cursorX();
        var newY = root.cursorY();
        var newHeight = root.cursorHeight();
        var lineChanged = Math.abs(newY - cursorRect.y) > 2;
        var maySmooth = root.smoothCursorEnabled && Date.now() <= root.smoothUntilMs;
        var shouldSnap = forceSnap || root.snapNextUpdate || !maySmooth || lineChanged;

        if (shouldSnap) {
            cursorRect.xBehaviorEnabled = false;
            cursorRect.yBehaviorEnabled = false;
            cursorRect.x = newX;
            cursorRect.y = newY;
            cursorRect.height = newHeight;
            Qt.callLater(function() {
                cursorRect.xBehaviorEnabled = true;
                cursorRect.yBehaviorEnabled = true;
            });
        } else {
            cursorRect.x = newX;
            cursorRect.y = newY;
            cursorRect.height = newHeight;
        }

        root.snapNextUpdate = false;

        if (typeof blinkAnim !== "undefined") {
            blinkAnim.restart();
        }
    }

    Timer {
        id: scrollDebounceTimer
        interval: 200
        repeat: false
        onTriggered: {
            root.snapNextCursorUpdate();
            updateCursorRect(true);
        }
    }

    onIsScrollingChanged: {
        if (isScrolling) {
            scrollDebounceTimer.stop();
            root.snapNextCursorUpdate();
        } else {
            scrollDebounceTimer.restart();
        }
    }

    Connections {
        target: root.targetTextArea

        function onCursorRectangleChanged() {
            updateCursorRect(false);
        }

        function onTextChanged() {
            updateCursorRect(false);
        }

        function onActiveFocusChanged() {
            root.snapNextCursorUpdate();
            updateCursorRect(true);
        }

        function onSelectionChanged() {
            if (root.targetTextArea && root.targetTextArea.selectedText.length > 0) {
                root.snapNextCursorUpdate();
            }
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
            id: blinkAnim
            loops: Animation.Infinite
            running: cursorRect.visible
            NumberAnimation { from: 1.0; to: 0.2; duration: 600; easing.type: Easing.InOutQuad }
            NumberAnimation { from: 0.2; to: 1.0; duration: 600; easing.type: Easing.InOutQuad }
        }
    }
}
