// =============================================================================
// EditorWheelScroller.qml — 编辑器滚轮惯性控制器
// =============================================================================

import QtQuick

Item {
    id: root

    property var scrollView: null
    property var textArea: null
    property var editorItem: null

    // Tuned in one place so WritingWorkspace stays a layout container.
    property real angleLinesPerStep: 3.0
    property real velocityGain: 42.0
    property real decayPerSecond: 0.045
    property real maxVelocityViewportMultiplier: 7.0
    property real minMaxVelocity: 2400
    property real stopVelocityFontMultiplier: 0.35
    property real minStopVelocity: 6

    readonly property bool active: wheelKineticTimer.running

    signal scrollActivity()

    enabled: scrollView && ((textArea && textArea.enabled) || (editorItem && editorItem.editor_enabled))

    property real wheelVelocityY: 0
    property real wheelLastTickMs: 0
    readonly property real wheelMaxVelocityY: Math.max(viewportHeight() * maxVelocityViewportMultiplier, minMaxVelocity)
    readonly property real wheelStopVelocityY: Math.max(wheelFontPixelSize() * stopVelocityFontMultiplier, minStopVelocity)

    function viewportHeight() {
        return scrollView ? scrollView.availableHeight : 0;
    }

    function contentItem() {
        return scrollView ? scrollView.contentItem : null;
    }

    function maxContentY() {
        var item = contentItem();
        if (!item) return 0;
        return Math.max(0, item.contentHeight - item.height);
    }

    function clampContentY(value) {
        return Math.max(0, Math.min(maxContentY(), value));
    }

    function wheelLineHeight() {
        if (editorItem) return Math.max(editorItem.font_pixel_size * 1.35, 18);
        if (!textArea) return 18;
        var rectHeight = textArea.cursorRectangle ? textArea.cursorRectangle.height : 0;
        return Math.max(rectHeight || 0, textArea.font.pixelSize * 1.35, 18);
    }

    function wheelFontPixelSize() {
        if (editorItem) return editorItem.font_pixel_size || 16;
        if (textArea) return textArea.font.pixelSize || 16;
        return 16;
    }

    function wheelDeltaPixels(event) {
        var pixelY = event.pixelDelta ? event.pixelDelta.y : 0;
        if (Math.abs(pixelY) > 0) return -pixelY;
        var angleY = event.angleDelta ? event.angleDelta.y : 0;
        if (Math.abs(angleY) > 0) return -(angleY / 120.0) * wheelLineHeight() * angleLinesPerStep;
        return 0;
    }

    function applyWheelImpulse(deltaY) {
        var item = contentItem();
        var maxY = maxContentY();
        if (!item || deltaY === 0 || maxY <= 0) return false;

        var currentY = item.contentY;
        if ((currentY <= 0 && deltaY < 0) || (currentY >= maxY && deltaY > 0)) {
            wheelVelocityY = 0;
            wheelKineticTimer.stop();
            return false;
        }

        if (wheelVelocityY * deltaY < 0) {
            wheelVelocityY = 0;
        }

        wheelVelocityY = Math.max(-wheelMaxVelocityY, Math.min(wheelMaxVelocityY, wheelVelocityY + deltaY * velocityGain));
        wheelLastTickMs = Date.now();
        if (!wheelKineticTimer.running) {
            wheelKineticTimer.start();
        }
        return true;
    }

    Timer {
        id: wheelKineticTimer
        interval: 16
        repeat: true
        onTriggered: {
            var item = root.contentItem();
            if (!item) {
                root.wheelVelocityY = 0;
                wheelKineticTimer.stop();
                return;
            }

            var now = Date.now();
            var dtSeconds = Math.max(0.001, Math.min((now - root.wheelLastTickMs) / 1000.0, 0.05));
            root.wheelLastTickMs = now;

            var oldY = item.contentY;
            var newY = root.clampContentY(oldY + root.wheelVelocityY * dtSeconds);
            item.contentY = newY;

            if (newY === oldY || newY <= 0 || newY >= root.maxContentY()) {
                root.wheelVelocityY = 0;
                wheelKineticTimer.stop();
                return;
            }

            root.wheelVelocityY *= Math.pow(root.decayPerSecond, dtSeconds);
            if (Math.abs(root.wheelVelocityY) < root.wheelStopVelocityY) {
                root.wheelVelocityY = 0;
                wheelKineticTimer.stop();
            }
        }
    }

    WheelHandler {
        id: editorWheelHandler
        target: null
        orientation: Qt.Vertical
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
        onWheel: function(event) {
            var deltaY = root.wheelDeltaPixels(event);
            if (deltaY === 0) {
                event.accepted = false;
                return;
            }

            event.accepted = root.applyWheelImpulse(deltaY);
            if (event.accepted) {
                root.scrollActivity();
            }
        }
    }
}
