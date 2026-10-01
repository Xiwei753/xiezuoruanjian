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
//   - 根对象是稳定 Item：x/y/width/height 与命中框恒定，对应 Canvas 的 nodeData 坐标；
//     wobble 只偏移内部视觉 Rectangle（visualNode），不影响命中测试。
//     TapHandler/DragHandler/PointHandler 全部挂在稳定 root Item 上。
//   - 使用 DesignTokens 统一样式
//   - Issue #814 评论 5935346839：节点不自己直接落盘诊断日志，交互边界日志
//     统一由 Canvas 的 logInteraction 写，避免 Node、Canvas 两层把同一次点击各记一份。
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
    readonly property color _border: dt.border
    readonly property color _surfaceContainer: dt.surfaceContainer
    readonly property color _shadowLight: dt.shadowLight
    readonly property color _textPrimary: dt.textPrimary
    readonly property color _textMuted: dt.textMuted
    readonly property int _radiusXs: dt.radiusXs
    readonly property int _radiusSm: dt.radiusSm

    property string title: "Node"
    property bool isSelected: false

    // 由 Canvas 控制：是否正处于拖动中（拖动时停止 idle wobble）
    property bool isBeingDragged: false

    // Issue #793 评论 5885482530: wobble 改纯视觉偏移，不影响命中框。
    // 根 Item 的 x/y/width/height 恒定，handler 命中基于稳定几何位置；
    // 视觉偏移只作用在内部 visualNode Rectangle 的 transform 上。
    // 用 index 错开 phase，避免所有节点同步晃
    property int wobbleIndex: 0
    // 动画驱动中间值，选中/按下/拖动时 visualOffset 归零
    property real _wobbleAnimX: 0
    property real _wobbleAnimY: 0

    // Issue #793 评论 5885482530: 声明时即为最终 binding（不再二次绑定）。
    // 选中、按下（nodeMouseTap/nodeTouchTap）、拖动时归零；idle 时跟随 wobble 动画。
    // Issue #801 评论 5894035036: 鼠标和触屏 TapHandler 拆开，pressed 取并集。
    property real visualOffsetX:
        (isSelected || isBeingDragged || nodeMouseTap.pressed || nodeTouchTap.pressed) ? 0 : _wobbleAnimX
    property real visualOffsetY:
        (isSelected || isBeingDragged || nodeMouseTap.pressed || nodeTouchTap.pressed) ? 0 : _wobbleAnimY

    // ---------------------------------------------------------------------------
    // 对外信号：节点只上抛事件，由 Canvas 决定后续行为
    // Issue #801 评论 5894035036: 长按按设备拆分——
    //   mouseLongPressed: 鼠标长按 → Canvas 进 connect（#373 鼠标规则：长按后拖=拉线）
    //   touchLongPressed: 触屏长按 → Canvas 进 contextPending（不移动弹菜单，移动转 connect）
    // ---------------------------------------------------------------------------
    signal singleClicked()
    signal doubleClicked()
    signal mouseLongPressed()
    signal touchLongPressed()
    signal contextMenuRequested(real sceneX, real sceneY)
    signal moveDelta(real dx, real dy)
    // 左键 press→release 追踪：由 PointHandler（passive grab）统一上抛，
    // 即使 DragHandler 取得 exclusive grab 也不丢观察链，保证长按后
    // 不拖动直接松开也能结束交互（Issue #788 评论 5868205321）。
    signal leftReleased()

    // Issue #801 评论 5894981235: 鼠标交互上抛信号，通知 Canvas 切回鼠标模式
    // （隐藏触屏 +/- 按钮）。触屏 TapHandler 不发此信号。
    signal mouseInteracted()

    // ---------------------------------------------------------------------------
    // 内部视觉卡片：只有它承载 transform 偏移，根 Item 几何保持稳定
    // ---------------------------------------------------------------------------
    Rectangle {
        id: visualNode
        anchors.fill: parent

        radius: root._radiusSm
        color: root._surfaceContainer
        border.color: root.isSelected ? root._accent : root._border
        border.width: root.isSelected ? 2 : 1

        // Issue #793 评论 5885482530: 纯视觉偏移，不影响根 Item 的 x/y 命中测试
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
            radius: visualNode.radius + 1
            visible: !root.isSelected
        }

        // Issue #801: 删除顶部类型标签条，标题成为卡片主体视觉信息
        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 8
            spacing: 0

            AppText {
                dt: root.dt
                Layout.fillWidth: true
                Layout.fillHeight: true
                text: root.title
                color: root._textPrimary
                font.pointSize: root.dt.fontSmPt
                wrapMode: Text.Wrap
                elide: Text.ElideRight
                horizontalAlignment: Text.AlignHCenter
                verticalAlignment: Text.AlignVCenter
            }
        }
    }

    // Issue #793 评论 5884923277: wobble 降速
    //   X: ±0.6px，半周期 7000~9500ms（7000 + (index % 7) * 400）
    //   Y: ±0.4px，半周期 8500~11500ms（8500 + (index % 5) * 300）
    // Issue #793 评论 5885482530: 选中/按下/拖动时动画暂停，idle 时才慢慢漂
    // Issue #801 评论 5894035036: pressed 取鼠标/触屏并集。
    SequentialAnimation on _wobbleAnimX {
        loops: Animation.Infinite
        running: !isSelected && !isBeingDragged && !nodeMouseTap.pressed && !nodeTouchTap.pressed
        NumberAnimation { to: 0.6; duration: 7000 + (wobbleIndex % 7) * 400; easing.type: Easing.InOutSine }
        NumberAnimation { to: -0.6; duration: 7000 + (wobbleIndex % 7) * 400; easing.type: Easing.InOutSine }
    }
    SequentialAnimation on _wobbleAnimY {
        loops: Animation.Infinite
        running: !isSelected && !isBeingDragged && !nodeMouseTap.pressed && !nodeTouchTap.pressed
        NumberAnimation { to: 0.4; duration: 8500 + (wobbleIndex % 5) * 300; easing.type: Easing.InOutSine }
        NumberAnimation { to: -0.4; duration: 8500 + (wobbleIndex % 5) * 300; easing.type: Easing.InOutSine }
    }

    // ---------------------------------------------------------------------------
    // 交互：用 TapHandler 上抛点击类信号，节点不自行决定行为
    // Issue #793 评论 5884923277: 加 exclusiveSignals 真正分清单击/双击，
    // 默认 NotExclusive 时双击会同时触发单击。
    // Issue #793 评论 5885482530: handler 全部挂在稳定 root Item 上，
    // 不放进 visualNode，命中框恒定。
    // Issue #801 评论 5894035036: 按 acceptedDevices 拆桌面指针/触屏——
    //   桌面指针长按 → mouseLongPressed（Canvas 进 connect）
    //   触屏长按 → touchLongPressed（Canvas 进 contextPending）
    //
    // Issue #812: 桌面语义一律 Mouse | TouchPad，不能只写 Mouse。acceptedDevices
    // 是硬过滤，设备类型不匹配时 Handler 根本不参与该事件；Wayland 的桌面
    // pointer 路径不能可靠还原成 Mouse，只写 Mouse 会让实体鼠标的单击选中、
    // 右键菜单、直接拖动全部失效。触屏语义保持 TouchScreen 独立。
    // ---------------------------------------------------------------------------
    TapHandler {
        id: nodeMouseTap
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
        acceptedButtons: Qt.LeftButton
        exclusiveSignals: TapHandler.SingleTap | TapHandler.DoubleTap

        // Issue #801 评论 5894981235: 鼠标按下即通知 Canvas 切回鼠标模式。
        onPressedChanged: { if (pressed) root.mouseInteracted() }

        onSingleTapped: root.singleClicked()
        onDoubleTapped: root.doubleClicked()
        onLongPressed: root.mouseLongPressed()
    }

    TapHandler {
        id: nodeTouchTap
        acceptedDevices: PointerDevice.TouchScreen
        acceptedButtons: Qt.LeftButton
        exclusiveSignals: TapHandler.SingleTap | TapHandler.DoubleTap

        onSingleTapped: root.singleClicked()
        onDoubleTapped: root.doubleClicked()
        onLongPressed: root.touchLongPressed()
    }

    TapHandler {
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
        acceptedButtons: Qt.RightButton
        // Issue #801 评论 5894981235: 右键也是鼠标交互。
        onPressedChanged: { if (pressed) root.mouseInteracted() }
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
    // Issue #801 评论 5894035036: 只有桌面指针直接拖 → move；触屏不在节点上 grab 拖动，
    // 让事件穿透到背景 pan（触屏 connect 移动由背景层 bgTouchDrag 处理）。
    // ---------------------------------------------------------------------------
    DragHandler {
        id: nodeDragHandler
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
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
    // Issue #801 评论 5894035036: PointHandler 保留鼠标+触屏 release 追踪（passive grab）。
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
}
