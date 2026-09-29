// =============================================================================
// StarMapEmbed.qml — 子星图 Embed 卡片组件
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：单个子星图 Embed 的可视化渲染、选中态展示、上抛点击类交互信号
// 约束：
//   - 纯 UI 组件，数据通过 property 传入
//   - Issue #801: 不再用文字标签标注"子星图"，改用 accentSoft 背景色调 +
//     右下角 ▸ 符号暗示可进入的嵌套空间；label 成为卡片主体视觉信息
//   - 单击只选中；双击进入 targetStarmapId；右键上抛菜单；拖动只改 Embed position
//   - 根对象是稳定 Item：x/y/width/height 与命中框恒定，对应 Canvas 的 embedData 坐标；
//     wobble 只偏移内部视觉 Rectangle（visualEmbed），不影响命中测试。
//   - 使用 TapHandler.SingleTap | DoubleTap 互斥方式处理单击/双击（和 StarMapNode.qml 一致）
//   - 使用 DesignTokens 统一样式
//
// Issue #796 评论 5886483653: 子星图改回正式 Embed 语义，不再伪装成 portal Node。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: root

    required property var dt

    readonly property color _primary: dt.primary
    readonly property color _onPrimary: dt.onPrimary
    readonly property color _accent: dt.accent
    readonly property color _accentSoft: dt.accentSoft
    readonly property color _border: dt.border
    readonly property color _surfaceContainer: dt.surfaceContainer
    readonly property color _shadowLight: dt.shadowLight
    readonly property color _textPrimary: dt.textPrimary
    readonly property color _textMuted: dt.textMuted
    readonly property int _radiusXs: dt.radiusXs
    readonly property int _radiusSm: dt.radiusSm

    // Embed 身份与数据
    property string instanceId: ""
    property string targetStarmapId: ""
    property string label: qsTr("子星图")
    property bool isSelected: false

    // 由 Canvas 控制：是否正处于拖动中（拖动时停止 idle wobble）
    property bool isBeingDragged: false

    // wobble 改纯视觉偏移，不影响命中框（和 StarMapNode.qml 一致）
    property int wobbleIndex: 0
    property real _wobbleAnimX: 0
    property real _wobbleAnimY: 0

    // Issue #801 评论 5894035036: 鼠标和触屏 TapHandler 拆开，pressed 取并集。
    property real visualOffsetX:
        (isSelected || isBeingDragged || embedMouseTap.pressed || embedTouchTap.pressed) ? 0 : _wobbleAnimX
    property real visualOffsetY:
        (isSelected || isBeingDragged || embedMouseTap.pressed || embedTouchTap.pressed) ? 0 : _wobbleAnimY

    // ---------------------------------------------------------------------------
    // 对外信号：Embed 只上抛事件，由 Canvas 决定后续行为
    // Issue #801 评论 5894035036: 长按按设备拆分（与 Node 对称）——
    //   mouseLongPressed: 鼠标长按 → Canvas 进 connect
    //   touchLongPressed: 触屏长按 → Canvas 进 contextPending
    // ---------------------------------------------------------------------------
    signal clicked(string instanceId)
    signal doubleClicked(string targetStarmapId)
    signal rightClicked(string instanceId)
    signal moveDelta(real dx, real dy)
    signal mouseLongPressed(string instanceId)
    signal touchLongPressed(string instanceId)
    signal contextMenuRequested(string instanceId, real sceneX, real sceneY)
    signal leftReleased()

    // Issue #801 评论 5894981235: 鼠标交互上抛信号，通知 Canvas 切回鼠标模式
    // （隐藏触屏 +/- 按钮）。触屏 TapHandler 不发此信号。
    signal mouseInteracted()

    // ---------------------------------------------------------------------------
    // 内部视觉卡片：只有它承载 transform 偏移，根 Item 几何保持稳定
    // ---------------------------------------------------------------------------
    Rectangle {
        id: visualEmbed
        anchors.fill: parent

        radius: root._radiusSm
        // Issue #801: 用 accentSoft 背景色调暗示可进入的嵌套空间（非文字方式）
        color: root.isSelected ? root._surfaceContainer : root._accentSoft
        // 选中态和 StarMapNode.qml 一样有明确边框
        border.color: root.isSelected ? root._accent : root._border
        border.width: root.isSelected ? 2 : 1

        // 纯视觉偏移，不影响根 Item 的 x/y 命中测试
        transform: Translate {
            x: root.visualOffsetX
            y: root.visualOffsetY
        }

        // Shadow effect approximation
        Rectangle {
            anchors.fill: parent
            anchors.margins: -1
            z: -1
            color: "transparent"
            border.color: root._shadowLight
            radius: visualEmbed.radius + 1
            visible: !root.isSelected
        }

        // Issue #801: 删除顶部"子星图"类型标签条，label 成为主体视觉信息
        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 8
            spacing: 0

            AppText {
                dt: root.dt
                Layout.fillWidth: true
                Layout.fillHeight: true
                text: root.label
                color: root._textPrimary
                font.pointSize: root.dt.fontSmPt
                wrapMode: Text.Wrap
                elide: Text.ElideRight
                horizontalAlignment: Text.AlignHCenter
                verticalAlignment: Text.AlignVCenter
            }
        }

        // Issue #801: 右下角 ▸ 符号暗示可进入的嵌套空间（非文字标签方式）
        AppText {
            dt: root.dt
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            anchors.rightMargin: 4
            anchors.bottomMargin: 2
            text: "▸"
            color: root._accent
            font.pointSize: root.dt.fontSmPt
            font.bold: true
        }
    }

    // wobble 降速，和 StarMapNode.qml 一致；选中/按下/拖动时动画暂停
    // Issue #801 评论 5894035036: pressed 取鼠标/触屏并集。
    SequentialAnimation on _wobbleAnimX {
        loops: Animation.Infinite
        running: !isSelected && !isBeingDragged && !embedMouseTap.pressed && !embedTouchTap.pressed
        NumberAnimation { to: 0.6; duration: 7000 + (wobbleIndex % 7) * 400; easing.type: Easing.InOutSine }
        NumberAnimation { to: -0.6; duration: 7000 + (wobbleIndex % 7) * 400; easing.type: Easing.InOutSine }
    }
    SequentialAnimation on _wobbleAnimY {
        loops: Animation.Infinite
        running: !isSelected && !isBeingDragged && !embedMouseTap.pressed && !embedTouchTap.pressed
        NumberAnimation { to: 0.4; duration: 8500 + (wobbleIndex % 5) * 300; easing.type: Easing.InOutSine }
        NumberAnimation { to: -0.4; duration: 8500 + (wobbleIndex % 5) * 300; easing.type: Easing.InOutSine }
    }

    // ---------------------------------------------------------------------------
    // 交互：TapHandler.SingleTap | DoubleTap 互斥（和 StarMapNode.qml 一致）
    // handler 全部挂在稳定 root Item 上，命中框恒定
    // Issue #801 评论 5894035036: 按 acceptedDevices 拆鼠标/触屏（与 Node 对称）。
    // ---------------------------------------------------------------------------
    TapHandler {
        id: embedMouseTap
        acceptedDevices: PointerDevice.Mouse
        acceptedButtons: Qt.LeftButton
        exclusiveSignals: TapHandler.SingleTap | TapHandler.DoubleTap

        // Issue #801 评论 5894981235: 鼠标按下即通知 Canvas 切回鼠标模式。
        onPressedChanged: { if (pressed) root.mouseInteracted() }

        onSingleTapped: root.clicked(root.instanceId)
        onDoubleTapped: root.doubleClicked(root.targetStarmapId)
        onLongPressed: root.mouseLongPressed(root.instanceId)
    }

    TapHandler {
        id: embedTouchTap
        acceptedDevices: PointerDevice.TouchScreen
        acceptedButtons: Qt.LeftButton
        exclusiveSignals: TapHandler.SingleTap | TapHandler.DoubleTap

        onSingleTapped: root.clicked(root.instanceId)
        onDoubleTapped: root.doubleClicked(root.targetStarmapId)
        onLongPressed: root.touchLongPressed(root.instanceId)
    }

    TapHandler {
        acceptedDevices: PointerDevice.Mouse
        acceptedButtons: Qt.RightButton
        // Issue #801 评论 5894981235: 右键也是鼠标交互。
        onPressedChanged: { if (pressed) root.mouseInteracted() }
        onSingleTapped: function(eventPoint) {
            root.rightClicked(root.instanceId)
            root.contextMenuRequested(root.instanceId, eventPoint.scenePosition.x, eventPoint.scenePosition.y)
        }
    }

    // ---------------------------------------------------------------------------
    // 拖动跟踪：DragHandler 只上抛原始移动增量，不修改 x/y、不决定行为
    // Canvas 根据 pointerMode 决定 moveDelta 的含义（connect 预览线 / move 移动 Embed）
    // DragHandler 只负责拖动增量；交互结束由 PointHandler 的 leftReleased 统一上抛。
    // （和 StarMapNode.qml 对称，Issue #796 评论 5888480054）
    // Issue #801 评论 5894035036: 只鼠标直接拖 → move；触屏不在 Embed 上 grab 拖动，
    // 让事件穿透到背景 pan（触屏 connect 移动由背景层 bgTouchDrag 处理）。
    // ---------------------------------------------------------------------------
    DragHandler {
        id: embedDragHandler
        acceptedDevices: PointerDevice.Mouse
        target: null
        acceptedButtons: Qt.LeftButton

        property real lastTx: 0
        property real lastTy: 0

        onActiveChanged: {
            if (active) {
                lastTx = 0
                lastTy = 0
            }
        }

        onActiveTranslationChanged: {
            var dx = activeTranslation.x - lastTx
            var dy = activeTranslation.y - lastTy
            lastTx = activeTranslation.x
            lastTy = activeTranslation.y
            // 转成世界坐标增量（除以父项 scale，container.scale === zoomLevel）
            var zoom = (root.parent && root.parent.scale) ? root.parent.scale : 1.0
            root.moveDelta(dx / zoom, dy / zoom)
        }
    }

    // 左键 press→release 观察：PointHandler 用 passive grab
    // Issue #801 评论 5894035036: 保留鼠标+触屏 release 追踪。
    PointHandler {
        id: leftPointTracker
        acceptedButtons: Qt.LeftButton

        onActiveChanged: {
            if (!active) {
                root.leftReleased()
            }
        }
    }
}
