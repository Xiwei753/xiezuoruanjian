// =============================================================================
// StarMapNode.qml — 星图节点组件
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：单个星图节点的可视化渲染、选中态展示、节点内联标题编辑、
//       上抛点击类交互信号
// 约束：
//   - 纯 UI 组件，数据通过 property 传入
//   - 节点只负责展示与上抛信号，不决定"移动还是拉线"——决定权交回共享的
//     StarMapInteractionController
//   - 节点自身不修改 x/y；只有归属层 StarMapSceneContent 在 move 模式下通过
//     绑定驱动位置
//   - 根对象是稳定 Item：x/y/width/height 与命中框恒定，对应本层局部坐标；
//     wobble 只偏移内部视觉 Rectangle（visualNode），不影响命中测试。
//     TapHandler/DragHandler/PointHandler 全部挂在稳定 root Item 上。
//   - 使用 DesignTokens 统一样式
//   - Issue #814 评论 5935346839：节点不自己直接落盘诊断日志，交互边界日志
//     统一由归属层 StarMapSceneContent 的 logInteraction 写，避免 Node、Content
//     两层把同一次点击各记一份。
//
// Issue #822：节点框本身就是编辑器。
//   标题不再用只读 AppText，也不再双击 → editNodeRequested → 外部 Popup。
//   同一个位置放 TextInput：平时 readOnly 只读显示，双击进入编辑并把光标放进
//   节点框内，Enter / editingFinished 提交给归属层 GraphController，Esc 还原。
//   编辑中关掉节点自身的所有手势 Handler，让文本选区、光标和输入法正常工作。
//   https://doc.qt.io/qt-6/qml-qtquick-textinput.html
// =============================================================================

import QtQuick

Item {
    id: root

    required property var dt

    readonly property color _accent: dt.accent
    readonly property color _border: dt.border
    readonly property color _surfaceContainer: dt.surfaceContainer
    readonly property color _shadowLight: dt.shadowLight
    readonly property color _textPrimary: dt.textPrimary
    readonly property int _radiusXs: dt.radiusXs
    readonly property int _radiusSm: dt.radiusSm

    property string title: "Node"
    property bool isSelected: false

    // 由归属层控制：是否正处于拖动中（拖动时停止 idle wobble）
    property bool isBeingDragged: false

    // Issue #822：内联编辑状态。编辑中节点手势全部让位给文本输入。
    property bool editing: false

    // Issue #793 评论 5885482530: wobble 改纯视觉偏移，不影响命中框。
    // 用 index 错开 phase，避免所有节点同步晃
    property int wobbleIndex: 0
    // 动画驱动中间值，选中/按下/拖动时 visualOffset 归零
    property real _wobbleAnimX: 0
    property real _wobbleAnimY: 0

    // Issue #793 评论 5885482530: 声明时即为最终 binding（不再二次绑定）。
    property real visualOffsetX:
        (isSelected || isBeingDragged || nodeMouseTap.pressed || nodeTouchTap.pressed) ? 0 : _wobbleAnimX
    property real visualOffsetY:
        (isSelected || isBeingDragged || nodeMouseTap.pressed || nodeTouchTap.pressed) ? 0 : _wobbleAnimY

    // ---------------------------------------------------------------------------
    // 对外信号：节点只上抛事件，行为由共享状态机 / 归属层决定
    // ---------------------------------------------------------------------------
    signal singleClicked()
    signal doubleClicked()
    // 桌面鼠标按下（Qt scene 坐标）：归属层据此登记 pressPending
    signal itemPressed(real sceneX, real sceneY)
    // 触屏长按：归属层据此进 contextPending（不移动弹菜单，移动转 connect）
    signal touchLongPressed()
    // 原始 Qt scene 坐标位移增量：归属层换算成局部坐标后交给共享状态机仲裁
    signal moveDelta(real dx, real dy)
    // 左键 press→release 追踪：由 PointHandler（passive grab）统一上抛，
    // 即使 DragHandler 取得 exclusive grab 也不丢观察链。
    signal leftReleased()
    signal mouseInteracted()
    // 内联编辑提交：归属层回写本层 GraphController 的 { title }
    signal titleCommitted(string title)

    // Issue #822：进入内联编辑。右键菜单"编辑"也走这里，不再开外部 Popup。
    function beginEdit() {
        if (editing)
            return
        editing = true
        titleInput.text = root.title
        titleInput.forceActiveFocus()
        titleInput.selectAll()
    }

    function cancelEdit() {
        if (!editing)
            return
        editing = false
        titleInput.focus = false
        titleInput.text = root.title
    }

    function commitEdit() {
        if (!editing)
            return
        var next = titleInput.text.trim()
        var changed = next.length > 0 && next !== root.title
        editing = false
        titleInput.focus = false
        titleInput.text = root.title
        if (changed)
            root.titleCommitted(next)
    }

    // 注意：DragHandler.activeTranslation 是 Qt scene 坐标增量。
    // 这里只上抛原始增量，Qt scene → 本层 local 的换算由归属层
    // StarMapSceneContent 统一做一次，delegate 不再各自换算。

    // ---------------------------------------------------------------------------
    // 内部视觉卡片：只有它承载 transform 偏移，根 Item 几何保持稳定
    // ---------------------------------------------------------------------------
    Rectangle {
        id: visualNode
        anchors.fill: parent

        radius: root._radiusSm
        color: root._surfaceContainer
        border.color: root.isSelected || root.editing ? root._accent : root._border
        border.width: (root.isSelected || root.editing) ? 2 : 1

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

        // Issue #822：标题就是编辑器。平时 readOnly 只读显示，
        // 双击进入编辑把光标放进节点框内，Enter / editingFinished 提交，Esc 还原。
        // enabled 只在编辑时为 true：非编辑态完全不接管输入，
        // 节点的点击/拖动/长按手势才能照常命中。
        TextInput {
            id: titleInput
            anchors.fill: parent
            anchors.margins: 8
            readOnly: !root.editing
            enabled: root.editing
            color: root._textPrimary
            font.pointSize: root.dt.fontSmPt
            selectByMouse: root.editing
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
            clip: true

            // 非编辑态用 binding 跟随 title；进入编辑后解除 binding，
            // 让用户自由输入而不被 binding 覆盖。
            Binding {
                target: titleInput
                property: "text"
                value: root.title
                when: !root.editing
                restoreMode: Binding.RestoreBindingOrValue
            }

            padding: 0

            onAccepted: root.commitEdit()
            onEditingFinished: root.commitEdit()
            Keys.onEscapePressed: root.cancelEdit()
        }
    }

    // Issue #793 评论 5884923277: wobble 降速
    //   X: ±0.6px，半周期 7000~9500ms（7000 + (index % 7) * 400）
    //   Y: ±0.4px，半周期 8500~11500ms（8500 + (index % 5) * 300）
    // Issue #793 评论 5885482530: 选中/按下/拖动时动画暂停，idle 时才慢慢漂
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
    // 交互：TapHandler 上抛点击类信号，节点不自行决定行为
    // Issue #817 评论 5949494799: 保持 TapHandler 默认 NotExclusive。
    // Issue #822: 编辑中（editing）全部 Handler 让位给文本输入。
    // Issue #812: 桌面语义一律 Mouse | TouchPad，不能只写 Mouse。acceptedDevices
    // 是硬过滤，设备类型不匹配时 Handler 根本不参与该事件；Wayland 的桌面
    // pointer 路径不能可靠还原成 Mouse，只写 Mouse 会让实体鼠标的单击选中、
    // 拖动全部失效。触屏语义保持 TouchScreen 独立。
    // ---------------------------------------------------------------------------
    TapHandler {
        id: nodeMouseTap
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
        acceptedButtons: Qt.LeftButton
        enabled: !root.editing

        onPressedChanged: {
            if (pressed) {
                // Issue #801 评论 5894981235: 鼠标按下即通知归属层切回鼠标模式。
                root.mouseInteracted()
                root.itemPressed(nodeMouseTap.point.pressPosition.x,
                                  nodeMouseTap.point.pressPosition.y)
            }
        }

        onSingleTapped: root.singleClicked()
        onDoubleTapped: root.doubleClicked()
    }

    TapHandler {
        id: nodeTouchTap
        acceptedDevices: PointerDevice.TouchScreen
        acceptedButtons: Qt.LeftButton
        enabled: !root.editing

        onSingleTapped: root.singleClicked()
        onDoubleTapped: root.doubleClicked()
        onLongPressed: root.touchLongPressed()
    }

    // ---------------------------------------------------------------------------
    // 拖动跟踪：DragHandler 只上抛原始移动增量，不修改 x/y、不决定行为。
    // 共享的 StarMapInteractionController 按"先超拖动阈值 → move，
    // 先到长按时间 → connect"仲裁，状态只被提升一次。
    // ---------------------------------------------------------------------------
    DragHandler {
        id: nodeDragHandler
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
        target: null
        acceptedButtons: Qt.LeftButton
        enabled: !root.editing

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
            root.moveDelta(dx, dy)
        }
    }

    // ---------------------------------------------------------------------------
    // 左键 press→release 观察：PointHandler 用 passive grab，一旦拿到该点
    // 会一直追踪到 release，即使 DragHandler 后来取得 exclusive grab 也不丢。
    // 这样无论"长按后拖动"还是"长按后直接松手"，都走同一个 leftReleased
    // 出口，由归属层统一结束 connect/move 状态。
    // https://doc.qt.io/qt-6/qml-qtquick-pointhandler.html
    // ---------------------------------------------------------------------------
    PointHandler {
        id: leftPointTracker
        acceptedButtons: Qt.LeftButton
        enabled: !root.editing

        onActiveChanged: {
            if (!active) {
                root.leftReleased()
            }
        }
    }
}
