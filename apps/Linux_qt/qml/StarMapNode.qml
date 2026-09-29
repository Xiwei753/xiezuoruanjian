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

    // Issue #793 评论 5884923277: portal 节点展示标记
    property bool isPortal: false

    // Issue #793 评论 5884923277: wobble 改纯视觉偏移，不影响命中框。
    // 根 Item 的 x/y/width/height 不变，handler 命中基于几何位置；
    // 视觉偏移由根 Rectangle 的 transform 提供。
    property real visualOffsetX: 0
    property real visualOffsetY: 0
    // 用 index 错开 phase，避免所有节点同步晃
    property int wobbleIndex: 0
    // 动画驱动中间值，选中/拖动时 visualOffset 归零
    property real _wobbleAnimX: 0
    property real _wobbleAnimY: 0

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

    // Issue #793 评论 5884923277: 纯视觉偏移，不影响 x/y 命中测试
    transform: Translate {
        x: visualOffsetX
        y: visualOffsetY
    }
    // 选中或拖动时偏移归零；idle 时跟随 wobble 动画
    visualOffsetX: (isSelected || isBeingDragged) ? 0 : _wobbleAnimX
    visualOffsetY: (isSelected || isBeingDragged) ? 0 : _wobbleAnimY

    // Issue #793 评论 5884923277: wobble 降速
    //   X: ±0.6px，半周期 7000~9500ms（7000 + (index % 7) * 400）
    //   Y: ±0.4px，半周期 8500~11500ms（8500 + (index % 5) * 300）
    // 选中/按下/拖动时动画暂停，idle 时才慢慢漂
    SequentialAnimation on _wobbleAnimX {
        loops: Animation.Infinite
        running: !isSelected && !isBeingDragged
        NumberAnimation { to: 0.6; duration: 7000 + (wobbleIndex % 7) * 400; easing.type: Easing.InOutSine }
        NumberAnimation { to: -0.6; duration: 7000 + (wobbleIndex % 7) * 400; easing.type: Easing.InOutSine }
    }
    SequentialAnimation on _wobbleAnimY {
        loops: Animation.Infinite
        running: !isSelected && !isBeingDragged
        NumberAnimation { to: 0.4; duration: 8500 + (wobbleIndex % 5) * 300; easing.type: Easing.InOutSine }
        NumberAnimation { to: -0.4; duration: 8500 + (wobbleIndex % 5) * 300; easing.type: Easing.InOutSine }
    }

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
                // Issue #793 评论 5884923277: portal 节点顶部标签显示"子星图"，
                // 普通节点仍显示自己的 kind
                text: isPortal ? qsTr("子星图") : root.kind
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
    // Issue #793 评论 5884923277: 加 exclusiveSignals 真正分清单击/双击，
    // 默认 NotExclusive 时双击会同时触发单击。
    // ---------------------------------------------------------------------------
    TapHandler {
        id: nodeLeftTap
        acceptedButtons: Qt.LeftButton
        exclusiveSignals: TapHandler.SingleTap | TapHandler.DoubleTap

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
