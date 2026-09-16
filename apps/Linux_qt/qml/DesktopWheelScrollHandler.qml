// =============================================================================
// DesktopWheelScrollHandler.qml — 桌面滚轮事件直译器
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：把 wheel 事件按输入设备语义直接翻译成 contentY 位移，不做惯性、
//       不跑 Timer、不维护 velocity、不做 decay。
//
// 为什么需要这个组件：
//   Qt 6.10 的 QQuickFlickable::wheelEvent() 自带滚轮平滑/加速逻辑
//   （wheelDeceleration、OutExpo timeline、wheel acceleration/flick 路径）。
//   即使平台要求"不加速"，Qt 仍会把一次滚轮位移摊开执行，无法保证
//   "鼠标滚多少走多少，停手就停"。本组件用 WheelHandler 拦截 wheel 事件
//   并直接修改 contentY，阻止 Flickable 自带的 wheel acceleration。
//
// 语义规则（Issue #695 评论 5693346400）：
//   1. WheelHandler 只负责接 wheel event，不维护 velocity、不跑 Timer、不做 decay。
//   2. event.pixelDelta.y != 0 时，直接 contentY -= pixelDelta.y。触摸板、系统
//      momentum、高精度设备发多少像素事件就走多少像素；系统停止发事件就停止，
//      不额外补惯性。
//   3. 没有 pixelDelta 时，用 angleDelta.y / 120.0 按比例滚。不假定每次一定是
//      ±120，高精度滚轮的小 delta 也保留。
//   4. 每个 120 单位对应的距离 = Application.styleHints.wheelScrollLines ×
//      lineSpacingPx。Qt 已把平台的"每格滚几行"暴露出来，不写死行数，也不按
//      鼠标品牌写特判。
//   5. 只处理纵向量；写作区没有横向滚动，纯横向 wheel event 不 accept，让事件
//      继续传播。
//   6. acceptedDevices 不押宝 Mouse/TouchPad 分类。Linux 上部分触摸板/Wacom 会以
//      普通 wheel event 出现；按事件的 pixelDelta/angleDelta 语义处理即可。用默认
//      AllDevices 覆盖所有会产生 wheel event 的 pointing device，但不处理触屏 drag
//      本身（WheelHandler 只接 wheel event，不接 drag）。
//
// Qt 官方依据：
//   - QWheelEvent 明确要求有 pixelDelta 时直接按像素使用
//   - 细分滚轮的 angleDelta 可能小于 120，应累计或按比例处理
//   - Application.styleHints.wheelScrollLines 是平台默认每格滚动行数
//   - Qt 6.10 QQuickFlickable::wheelEvent() 源码内部有 wheelDeceleration 和
//     wheel acceleration 路径，需要用 WheelHandler 拦截并 accept 事件来阻止
// =============================================================================

import QtQuick

Item {
    id: root

    // 目标 Flickable，直接修改其 contentY
    // 通常是 ScrollView.contentItem（内部 Flickable）
    property var targetFlickable: null

    // 每个 120 单位（一格）对应的滚动距离 = wheelScrollLines × lineSpacingPx
    // 编辑区传 字体大小 × 行距倍数；设置页传一个合理行高估计值
    property real lineSpacingPx: 24

    // 暴露给外部，用于 editorIsScrolling 等滚动期间动画暂停逻辑
    // WheelHandler.active 在 wheel 事件期间为 true，事件结束即 false
    readonly property bool active: wheelHandler.active

    // 非可视：不绘制内容，不拦截鼠标点击/拖拽（Item 默认不处理鼠标事件），
    // 只通过 WheelHandler 接 wheel event
    WheelHandler {
        id: wheelHandler
        // target: null — 不让 WheelHandler 操作任何对象的 rotation/property，
        // 只用 onWheel 信号做事件直译
        target: null
        // acceptedDevices 用默认 AllDevices：按事件语义处理，不押宝设备分类
        // 触屏 drag 不受影响 — WheelHandler 只接 wheel event，不接 drag

        onWheel: function(event) {
            if (!root.targetFlickable) {
                event.accepted = false
                return
            }

            var pdy = event.pixelDelta.y
            var ady = event.angleDelta.y

            // 只处理纵向量；纯横向 wheel event 不 accept，让事件继续传播
            if (pdy === 0 && ady === 0) {
                event.accepted = false
                return
            }

            var flick = root.targetFlickable
            var newY = flick.contentY

            if (pdy !== 0) {
                // pixelDelta 直接按像素使用：触摸板、系统 momentum、高精度设备
                // 发多少像素事件就走多少像素；系统停止发事件就停止
                newY -= pdy
            } else {
                // 没有 pixelDelta 时按 angleDelta / 120.0 比例滚
                // 不假定每次一定是 ±120，高精度滚轮的小 delta 也保留
                // Application.styleHints.wheelScrollLines 是平台默认每格行数
                var linesPerNotch = Application.styleHints.wheelScrollLines || 1
                var pixelsPerNotch = linesPerNotch * root.lineSpacingPx
                newY -= (ady / 120.0) * pixelsPerNotch
            }

            // clamp 到有效滚动范围
            var maxY = Math.max(0, (flick.contentHeight || 0) - (flick.height || 0))
            flick.contentY = Math.max(0, Math.min(maxY, newY))

            // accept 事件，阻止 Qt 6.10 QQuickFlickable::wheelEvent() 自带的
            // wheel acceleration/flick 路径
            event.accepted = true
        }
    }
}
