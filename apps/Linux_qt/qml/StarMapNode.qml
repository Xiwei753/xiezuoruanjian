// =============================================================================
// StarMapNode.qml — 星图节点组件
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：单个星图节点的可视化渲染、选中态展示、节点内联标题编辑
// 约束：
//   - 纯 UI 组件，数据通过 property 传入
//   - 节点自身不接任何业务手势：单击/双击/移动/连线/右键全部由根层唯一的
//     StarMapInputRouter 解释，节点不再挂 TapHandler/DragHandler/PointHandler，
//     也就不会和递归命中抢同一次手势
//   - 节点自身不修改 x/y；只有归属层 StarMapSceneContent 在 move 模式下通过
//     绑定驱动位置
//   - 根对象是稳定 Item：x/y/width/height 与命中框恒定，对应本层局部坐标；
//     wobble 只偏移内部视觉 Rectangle（visualNode），不影响命中测试。
//   - 使用 DesignTokens 统一样式
//   - Issue #814 评论 5935346839：节点不自己直接落盘诊断日志，交互边界日志
//     统一由归属层 StarMapSceneContent 的 logInteraction 写，避免 Node、Content
//     两层把同一次点击各记一份。
//   - Issue #832：按下/拖动的视觉暂停改绑共享 StarMapInteractionController 的
//     isPressedTarget()，节点不再依赖自己的 Handler.pressed。
//
// Issue #822：节点框本身就是编辑器。
//   标题不再用只读 AppText，也不再双击 → editNodeRequested → 外部 Popup。
//   同一个位置放 TextInput：平时 readOnly 只读显示，双击进入编辑并把光标放进
//   节点框内，Enter / editingFinished 提交给归属层 GraphController，Esc 还原。
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

    // Issue #832：节点身份与共享手势状态机，只有视觉查询用（不解释输入）。
    property string nodeId: ""
    property string scenePathKey: ""
    property var interactionController: null

    property string title: "Node"
    property bool isSelected: false

    // Issue #822：内联编辑状态。编辑中节点让位给文本输入。
    property bool editing: false

    // Issue #793 评论 5885482530: wobble 改纯视觉偏移，不影响命中框。
    // 用 index 错开 phase，避免所有节点同步晃
    property int wobbleIndex: 0
    // 动画驱动中间值，选中/按下/拖动时 visualOffset 归零
    property real _wobbleAnimX: 0
    property real _wobbleAnimY: 0

    // 是否被当前手势按住/拖动（pressPending / contextPending / connect / move
    // 任一状态指向本节点）：视觉动画暂停的唯一判据。
    readonly property bool isPressed: interactionController
            ? interactionController.isPressedTarget(scenePathKey, "node", nodeId)
            : false

    // Issue #793 评论 5885482530: 声明时即为最终 binding（不再二次绑定）。
    property real visualOffsetX:
        (isSelected || isPressed) ? 0 : _wobbleAnimX
    property real visualOffsetY:
        (isSelected || isPressed) ? 0 : _wobbleAnimY

    // ---------------------------------------------------------------------------
    // 对外信号：只上报内联编辑提交，交互事件不再经过节点。
    // ---------------------------------------------------------------------------
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
        // 节点的点击/拖动/长按手势才能照常命中（这些手势现在统一由 Router 解释）。
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
    // Issue #832: 按下判据改绑共享 InteractionController（节点自己不再有 Handler）
    SequentialAnimation on _wobbleAnimX {
        loops: Animation.Infinite
        running: !isSelected && !isPressed
        NumberAnimation { to: 0.6; duration: 7000 + (wobbleIndex % 7) * 400; easing.type: Easing.InOutSine }
        NumberAnimation { to: -0.6; duration: 7000 + (wobbleIndex % 7) * 400; easing.type: Easing.InOutSine }
    }
    SequentialAnimation on _wobbleAnimY {
        loops: Animation.Infinite
        running: !isSelected && !isPressed
        NumberAnimation { to: 0.4; duration: 8500 + (wobbleIndex % 5) * 300; easing.type: Easing.InOutSine }
        NumberAnimation { to: -0.4; duration: 8500 + (wobbleIndex % 5) * 300; easing.type: Easing.InOutSine }
    }
}
