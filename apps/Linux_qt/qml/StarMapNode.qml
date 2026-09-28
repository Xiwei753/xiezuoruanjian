// =============================================================================
// StarMapNode.qml — 星图节点组件
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：单个星图节点的可视化渲染、选中态展示、上抛点击类交互信号
// 约束：
//   - 纯 UI 组件，数据通过 property 传入
//   - 节点只负责展示与上抛信号，不决定"移动还是拉线"——决定权交回 Canvas
//   - 节点自身不修改 x/y；只有 Canvas 在 move 模式下通过绑定驱动位置
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root

    required property var dt

    readonly property color _primary: dt.primary
    readonly property color _onPrimary: dt.onPrimary
    readonly property color _accent: dt.accent
    readonly property color _border: dt.border
    readonly property color _surfaceContainer: dt.surfaceContainer
    readonly property color _shadowLight: dt.shadowLight
    readonly property color _textPrimary: dt.textPrimary
    readonly property color _textMuted: dt.textMuted
    readonly property int _radiusXs: dt.radiusXs
    readonly property int _radiusSm: dt.radiusSm

    property string title: "Node"
    property string kind: "Note"
    property bool isSelected: false

    // 由 Canvas 控制：是否正处于拖动中（拖动时停止 idle wobble）
    property bool isBeingDragged: false

    // ---------------------------------------------------------------------------
    // 对外信号：节点只上抛事件，由 Canvas 决定后续行为
    // ---------------------------------------------------------------------------
    signal singleClicked()
    signal doubleClicked()
    signal longPressed()
    signal contextMenuRequested(real sceneX, real sceneY)
    signal moveDelta(real dx, real dy)
    // 左键 press→release 追踪：由 PointHandler（passive grab）统一上抛，
    // 即使 DragHandler 取得 exclusive grab 也不丢观察链，保证长按后
    // 不拖动直接松开也能结束交互（Issue #788 评论 5868205321）。
    signal leftReleased()

    radius: _radiusSm
    color: _surfaceContainer
    border.color: isSelected ? _accent : _border
    border.width: isSelected ? 2 : 1

    // Shadow effect approximation
    Rectangle {
        anchors.fill: parent
        anchors.margins: -1
        z: -1
        color: "transparent"
        border.color: _shadowLight
        radius: root.radius + 1
        visible: !isSelected
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 8
        spacing: 4

        Rectangle {
            Layout.fillWidth: true
            height: 16
            color: getKindColor(root.kind)
            radius: _radiusXs

            AppText {
                dt: root.dt
                anchors.centerIn: parent
                text: root.kind
                color: _onPrimary
                font.pointSize: dt.fontXsPt
                font.bold: true
            }
        }

        AppText {
            dt: root.dt
            Layout.fillWidth: true
            Layout.fillHeight: true
            text: root.title
            color: _textPrimary
            font.pointSize: dt.fontSmPt
            wrapMode: Text.Wrap
            elide: Text.ElideRight
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
        }
    }

    // ---------------------------------------------------------------------------
    // 交互：用 TapHandler 上抛点击类信号，节点不自行决定行为
    // ---------------------------------------------------------------------------
    TapHandler {
        id: nodeLeftTap
        acceptedButtons: Qt.LeftButton
        onSingleTapped: root.singleClicked()
        onDoubleTapped: root.doubleClicked()
        onLongPressed: root.longPressed()
    }

    TapHandler {
        acceptedButtons: Qt.RightButton
        onSingleTapped: function(eventPoint) {
            root.contextMenuRequested(eventPoint.scenePosition.x, eventPoint.scenePosition.y)
        }
    }

    // ---------------------------------------------------------------------------
    // 拖动跟踪：DragHandler 只上抛原始移动增量，不修改 x/y、不决定行为
    // Canvas 根据 pointerMode 决定 moveDelta 的含义（connect 预览线 / move 移动节点）
    // DragHandler 只负责拖动增量；交互结束由 PointHandler 的 leftReleased 统一上抛。
    // DragHandler.active 仅在超过 dragThreshold 后才为 true，长按后不拖动直接松开时
    // onActiveChanged(false) 不会触发，故不能依赖它来结束 connect/move 状态。
    // （Issue #788 评论 5868205321）
    // ---------------------------------------------------------------------------
    DragHandler {
        id: nodeDragHandler
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

    // ---------------------------------------------------------------------------
    // 左键 press→release 观察：PointHandler 用 passive grab，一旦拿到该点
    // 会一直追踪到 release，即使 DragHandler 后来取得 exclusive grab 也不丢。
    // 这样无论"长按后拖动"还是"长按后直接松手"，都走同一个 leftReleased
    // 出口，由 Canvas 统一结束 connect/move 状态。
    // https://doc.qt.io/qt-6.8/qml-qtquick-pointhandler.html
    // ---------------------------------------------------------------------------
    PointHandler {
        id: leftPointTracker
        acceptedButtons: Qt.LeftButton

        onActiveChanged: {
            if (!active) {
                root.leftReleased()
            }
        }
    }

    function getKindColor(k) {
        switch(k) {
            case "Chapter": return dt.starMapNodeChapter
            case "Character": return dt.starMapNodeCharacter
            case "Location": return dt.starMapNodeLocation
            case "Event": return dt.starMapNodeEvent
            case "Concept": return dt.starMapNodeConcept
            default: return _textMuted
        }
    }

    function getKindLabel(k) {
        switch(k) {
            case "Note": return qsTr("笔记")
            case "Chapter": return qsTr("章节")
            case "Character": return qsTr("角色")
            case "Location": return qsTr("地点")
            case "Event": return qsTr("事件")
            case "Concept": return qsTr("概念")
            case "Project": return qsTr("作品")
            case "Volume": return qsTr("卷")
            case "Custom": return qsTr("自定义")
            default: return k
        }
    }
}
