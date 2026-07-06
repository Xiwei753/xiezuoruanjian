// =============================================================================
// SmoothWheelScroller.qml — 通用滚轮惯性控制器
// =============================================================================
//
// 层级：Desktop UI 层（QML 通用组件）
// 职责：为任意 ScrollView 提供平滑滚轮惯性滚动
// 约束：
//   - 不依赖编辑器特定属性（textArea / editorItem）
//   - 通过 scrollView 属性绑定目标
//   - 可选 lineHeight / fontPixelSize 用于角度滚轮步进计算
// =============================================================================

import QtQuick

Item {
    id: root

    property var scrollView: null

    // Optional: line height for angle-delta wheel step calculation.
    // If not set, defaults to 20px.
    property real lineHeight: 20

    // Optional: font pixel size for stop-velocity calculation.
    // If not set, defaults to 16.
    property real fontPixelSize: 16

    property real angleLinesPerStep: 2.2
    property real velocityGain: 25.0
    property real pixelVelocityGain: 10.0
    property real decayPerSecond: 0.006
    property real maxVelocityViewportMultiplier: 4.0
    property real minMaxVelocity: 1400
    property real stopVelocityFontMultiplier: 0.35
    property real minStopVelocity: 6

    readonly property bool active: wheelKineticTimer.running

    signal scrollActivity()

    enabled: scrollView !== null

    property real wheelVelocityY: 0
    property real wheelLastTickMs: 0
    readonly property real wheelMaxVelocityY: Math.max(viewportHeight() * maxVelocityViewportMultiplier, minMaxVelocity)
    readonly property real wheelStopVelocityY: Math.max(fontPixelSize * stopVelocityFontMultiplier, minStopVelocity)

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

    function wheelDeltaPixels(event) {
        var pixelY = event.pixelDelta ? event.pixelDelta.y : 0;
        if (Math.abs(pixelY) > 0) return { deltaY: -pixelY, gain: pixelVelocityGain };
        var angleY = event.angleDelta ? event.angleDelta.y : 0;
        if (Math.abs(angleY) > 0) return { deltaY: -(angleY / 120.0) * lineHeight * angleLinesPerStep, gain: velocityGain };
        return { deltaY: 0, gain: velocityGain };
    }

    function applyWheelImpulse(deltaY, gain) {
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

        wheelVelocityY = Math.max(-wheelMaxVelocityY, Math.min(wheelMaxVelocityY, wheelVelocityY + deltaY * gain));
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
        id: smoothWheelHandler
        target: null
        orientation: Qt.Vertical
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
        onWheel: function(event) {
            var wheelDelta = root.wheelDeltaPixels(event);
            if (wheelDelta.deltaY === 0) {
                event.accepted = false;
                return;
            }

            event.accepted = root.applyWheelImpulse(wheelDelta.deltaY, wheelDelta.gain);
            if (event.accepted) {
                root.scrollActivity();
            }
        }
    }
}
