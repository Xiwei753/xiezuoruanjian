// =============================================================================
// StarMapEmbed.qml — 子星图 Embed 卡片组件
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：单个子星图 Embed 的可视化渲染、选中态展示、上抛点击类交互信号
// 约束：
//   - 纯 UI 组件，数据通过 property 传入
//   - 不伪装成普通 Node：视觉参考 StarMapNode 但顶部标签明确写"子星图"
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

    property real visualOffsetX:
        (isSelected || isBeingDragged || embedLeftTap.pressed) ? 0 : _wobbleAnimX
    property real visualOffsetY:
        (isSelected || isBeingDragged || embedLeftTap.pressed) ? 0 : _wobbleAnimY

    // ---------------------------------------------------------------------------
    // 对外信号：Embed 只上抛事件，由 Canvas 决定后续行为
    // ---------------------------------------------------------------------------
    signal clicked(string instanceId)
    signal doubleClicked(string targetStarmapId)
    signal rightClicked(string instanceId)
    signal dragged(string instanceId, real newX, real newY)
    signal longPressed(string instanceId)
    signal contextMenuRequested(string instanceId, real sceneX, real sceneY)
    signal leftReleased()

    // ---------------------------------------------------------------------------
    // 内部视觉卡片：只有它承载 transform 偏移，根 Item 几何保持稳定
    // ---------------------------------------------------------------------------
    Rectangle {
        id: visualEmbed
        anchors.fill: parent

        radius: root._radiusSm
        color: root._surfaceContainer
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

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 8
            spacing: 4

            // 顶部标签：明确写"子星图"，不伪装成普通 Node
            Rectangle {
                Layout.fillWidth: true
                height: 16
                color: root._accent
                radius: root._radiusXs

                AppText {
                    dt: root.dt
                    anchors.centerIn: parent
                    text: qsTr("子星图")
                    color: root._onPrimary
                    font.pointSize: root.dt.fontXsPt
                    font.bold: true
                }
            }

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
    }

    // wobble 降速，和 StarMapNode.qml 一致；选中/按下/拖动时动画暂停
    SequentialAnimation on _wobbleAnimX {
        loops: Animation.Infinite
        running: !isSelected && !isBeingDragged && !embedLeftTap.pressed
        NumberAnimation { to: 0.6; duration: 7000 + (wobbleIndex % 7) * 400; easing.type: Easing.InOutSine }
        NumberAnimation { to: -0.6; duration: 7000 + (wobbleIndex % 7) * 400; easing.type: Easing.InOutSine }
    }
    SequentialAnimation on _wobbleAnimY {
        loops: Animation.Infinite
        running: !isSelected && !isBeingDragged && !embedLeftTap.pressed
        NumberAnimation { to: 0.4; duration: 8500 + (wobbleIndex % 5) * 300; easing.type: Easing.InOutSine }
        NumberAnimation { to: -0.4; duration: 8500 + (wobbleIndex % 5) * 300; easing.type: Easing.InOutSine }
    }

    // ---------------------------------------------------------------------------
    // 交互：TapHandler.SingleTap | DoubleTap 互斥（和 StarMapNode.qml 一致）
    // handler 全部挂在稳定 root Item 上，命中框恒定
    // ---------------------------------------------------------------------------
    TapHandler {
        id: embedLeftTap
        acceptedButtons: Qt.LeftButton
        exclusiveSignals: TapHandler.SingleTap | TapHandler.DoubleTap

        onSingleTapped: root.clicked(root.instanceId)
        onDoubleTapped: root.doubleClicked(root.targetStarmapId)
        onLongPressed: root.longPressed(root.instanceId)
    }

    TapHandler {
        acceptedButtons: Qt.RightButton
        onSingleTapped: function(eventPoint) {
            root.rightClicked(root.instanceId)
            root.contextMenuRequested(root.instanceId, eventPoint.scenePosition.x, eventPoint.scenePosition.y)
        }
    }

    // ---------------------------------------------------------------------------
    // 拖动跟踪：DragHandler 只上抛原始移动增量，不修改 x/y、不决定行为
    // Canvas 决定 dragged 的含义（move 模式下移动 Embed position）
    // ---------------------------------------------------------------------------
    DragHandler {
        id: embedDragHandler
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
            var wdx = dx / zoom
            var wdy = dy / zoom
            // 直接改根 Item 的 x/y，并上拖 dragged 信号
            root.x += wdx
            root.y += wdy
            root.dragged(root.instanceId, root.x, root.y)
        }
    }

    // 左键 press→release 观察：PointHandler 用 passive grab
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
