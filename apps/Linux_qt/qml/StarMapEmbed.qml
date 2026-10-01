// =============================================================================
// StarMapEmbed.qml — 子星图 Embed 卡片组件（事件分层 + 递归渲染）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：单个子星图 Embed 的可视化渲染、选中态展示、上抛点击类交互信号
//
// Issue #805 评论 5907045450 第 3 部分：事件分层
//   改成两层: Embed → chrome(title hit area + 4条 border hit area) +
//   contentViewport(child StarMapScene)。
//   - 标题文字命中：选择/移动/右键/长按都作用于 Embed。
//   - 四条边框命中：同上；边框可以有少量 hit slop。
//   - contentViewport：父 Embed 不挂 TapHandler/DragHandler/MouseArea，
//     事件直接给 child Scene。
//   - 删除 doubleClicked(targetStarmapId) 信号和右下角"▸ 进入"语义。
//   - 不用 findEmbedChromeAt() 把整个矩形都判成 Embed 命中。
//
// Issue #805 评论 5907045450 第 2 部分：递归渲染
//   contentViewport 内部用 Loader 创建下一层 StarMapScene
//   （childPath = parent.pathSegments + EnterEmbed(embed.instanceId)）。
//   子 Scene 自己有 viewport 和手势状态。
//
// 约束：
//   - 纯 UI 组件，数据通过 property 传入
//   - 根对象是稳定 Item：x/y/width/height 与命中框恒定
//   - 使用 DesignTokens 统一样式
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
    property string label: ""
    property bool isSelected: false

    // 由 Canvas 控制：是否正处于拖动中（拖动时停止 idle wobble）
    property bool isBeingDragged: false

    // wobble 改纯视觉偏移，不影响命中框
    property int wobbleIndex: 0
    property real _wobbleAnimX: 0
    property real _wobbleAnimY: 0

    // Issue #805 评论 5907045450 第 2 部分：递归渲染上下文。
    // rootStarmapId / parentPathSegments / starmapBackendRef 由 Canvas 传入，
    // Embed 的 contentViewport 用这些构造子 Scene 的 pathSegments。
    property string rootStarmapId: ""
    property var parentPathSegments: []
    property var starmapBackendRef: null
    // Issue #805 评论 5908703621 问题 1：父路径 key，由 Canvas 传入，
    // 用于构造 child Scene 的 pathKey（父路径 + "/embed_<instanceId>"）。
    property string parentPathKey: ""

    // 递归 Scene 不再无条件一次性展开整棵引用树。
    // Canvas 只把“当前视口内的 Embed”置为 true；第一次进入视口后锁存为已激活，
    // 这样滚动离开后不会销毁 child Scene，也不会丢掉该 Scene 自己的 pan/zoom/手势状态。
    // 层级仍不设固定上限，下一层继续按它自己的视口决定何时实例化。
    property bool childSceneInViewport: false
    property bool childSceneActivated: false
    onChildSceneInViewportChanged: {
        if (childSceneInViewport)
            childSceneActivated = true
    }
    Component.onCompleted: {
        if (childSceneInViewport)
            childSceneActivated = true
    }

    // Issue #805 评论 5907045450 第 3 部分：chrome 命中区域高度 + 边框 hit slop。
    readonly property int _chromeHeight: 24
    readonly property int _borderSlop: 6

    property real visualOffsetX:
        (isSelected || isBeingDragged || chromeMouseTap.pressed || chromeTouchTap.pressed) ? 0 : _wobbleAnimX
    property real visualOffsetY:
        (isSelected || isBeingDragged || chromeMouseTap.pressed || chromeTouchTap.pressed) ? 0 : _wobbleAnimY

    // ---------------------------------------------------------------------------
    // 对外信号：Embed 只上抛事件，由 Canvas 决定后续行为
    // Issue #805 评论 5907045450 第 3 部分：删除 doubleClicked 信号。
    // ---------------------------------------------------------------------------
    signal clicked(string instanceId)
    signal rightClicked(string instanceId)
    signal moveDelta(real dx, real dy)
    signal mouseLongPressed(string instanceId)
    signal touchLongPressed(string instanceId)
    signal contextMenuRequested(string instanceId, real sceneX, real sceneY)
    signal leftReleased()
    signal mouseInteracted()
    // Issue #805 评论 5908703621 问题 5：child Scene 的节点编辑请求向上冒泡。
    // 已带 owner 上下文（ownerStarmapId/ownerPathKey），Canvas 接收后转发给 Scene。
    // Issue #805 评论 5912394108：ownerScene(var) 携带真正拥有该节点的子 Scene 引用，
    // 原样冒泡保持指向不变，Workspace 据此直接回写到对应子 Scene 的 Controller。
    signal editNodeRequested(var ownerScene, string ownerStarmapId, string ownerPathKey, var node)

    // ---------------------------------------------------------------------------
    // 内部视觉卡片：只有它承载 transform 偏移，根 Item 几何保持稳定
    // ---------------------------------------------------------------------------
    Rectangle {
        id: visualEmbed
        anchors.fill: parent

        radius: root._radiusSm
        color: root.isSelected ? root._surfaceContainer : root._accentSoft
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

        // ── Issue #805 评论 5907045450 第 3 部分：chrome 区域 ──
        // 标题条（顶部 _chromeHeight 高度），挂 TapHandler/DragHandler。
        // 命中标题条：选择/移动/右键/长按都作用于 Embed。
        // Issue #805 评论 5908703621 问题 2：Handler 直接放进 titleBar 内部，
        // parent Item 就是 titleBar，命中范围限定在标题条。
        // 不靠 target: titleBar 做事件隔离。
        Rectangle {
            id: titleBar
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.top: parent.top
            height: root._chromeHeight
            color: "transparent"

            AppText {
                anchors.fill: parent
                anchors.leftMargin: 8
                anchors.rightMargin: 8
                text: root.label
                color: root._textPrimary
                font.pointSize: root.dt.fontSmPt
                wrapMode: Text.NoWrap
                elide: Text.ElideRight
                horizontalAlignment: Text.AlignHCenter
                verticalAlignment: Text.AlignVCenter
            }

            TapHandler {
                id: chromeMouseTap
                acceptedDevices: PointerDevice.Mouse
                acceptedButtons: Qt.LeftButton
                exclusiveSignals: TapHandler.SingleTap | TapHandler.DoubleTap
                onPressedChanged: { if (pressed) root.mouseInteracted() }
                onSingleTapped: root.clicked(root.instanceId)
                onLongPressed: root.mouseLongPressed(root.instanceId)
            }

            TapHandler {
                id: chromeTouchTap
                acceptedDevices: PointerDevice.TouchScreen
                acceptedButtons: Qt.LeftButton
                exclusiveSignals: TapHandler.SingleTap | TapHandler.DoubleTap
                onSingleTapped: root.clicked(root.instanceId)
                onLongPressed: root.touchLongPressed(root.instanceId)
            }

            TapHandler {
                acceptedDevices: PointerDevice.Mouse
                acceptedButtons: Qt.RightButton
                onPressedChanged: { if (pressed) root.mouseInteracted() }
                onSingleTapped: function(eventPoint) {
                    root.rightClicked(root.instanceId)
                    root.contextMenuRequested(root.instanceId, eventPoint.scenePosition.x, eventPoint.scenePosition.y)
                }
            }

            DragHandler {
                id: titleDragHandler
                acceptedDevices: PointerDevice.Mouse
                target: null
                acceptedButtons: Qt.LeftButton
                grabPermissions: PointerHandler.CanTakeOverFromHandlersOfDifferentType
                property real lastTx: 0
                property real lastTy: 0
                onActiveChanged: {
                    if (active) { lastTx = 0; lastTy = 0 }
                }
                onActiveTranslationChanged: {
                    var dx = activeTranslation.x - lastTx
                    var dy = activeTranslation.y - lastTy
                    lastTx = activeTranslation.x
                    lastTy = activeTranslation.y
                    var zoom = (root.parent && root.parent.scale) ? root.parent.scale : 1.0
                    root.moveDelta(dx / zoom, dy / zoom)
                }
            }

            PointHandler {
                id: titlePointTracker
                acceptedButtons: Qt.LeftButton
                onActiveChanged: {
                    if (!active) root.leftReleased()
                }
            }
        }

        // ── Issue #805 评论 5907045450 第 3 部分：四条边框命中区域 ──
        // 边框有少量 hit slop，命中时选择/移动作用于 Embed。
        // Issue #805 评论 5908703621 问题 2：每条 border 内部放一套与 titleBar
        // 对称的 handler（单击选择 + 长按拉线 + 右键菜单 + 拖动移动 + press→release）。
        // Handler 声明在 border 内部，命中范围就是那条 border。
        Rectangle {
            id: borderTop
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.top: parent.top
            height: root._borderSlop
            color: "transparent"

            TapHandler {
                acceptedDevices: PointerDevice.Mouse
                acceptedButtons: Qt.LeftButton
                onPressedChanged: { if (pressed) root.mouseInteracted() }
                onSingleTapped: root.clicked(root.instanceId)
                onLongPressed: root.mouseLongPressed(root.instanceId)
            }
            TapHandler {
                acceptedDevices: PointerDevice.TouchScreen
                acceptedButtons: Qt.LeftButton
                onSingleTapped: root.clicked(root.instanceId)
                onLongPressed: root.touchLongPressed(root.instanceId)
            }
            TapHandler {
                acceptedDevices: PointerDevice.Mouse
                acceptedButtons: Qt.RightButton
                onSingleTapped: function(eventPoint) {
                    root.rightClicked(root.instanceId)
                    root.contextMenuRequested(root.instanceId, eventPoint.scenePosition.x, eventPoint.scenePosition.y)
                }
            }
            DragHandler {
                acceptedDevices: PointerDevice.Mouse
                target: null
                acceptedButtons: Qt.LeftButton
                grabPermissions: PointerHandler.CanTakeOverFromHandlersOfDifferentType
                property real lastTx: 0
                property real lastTy: 0
                onActiveChanged: { if (active) { lastTx = 0; lastTy = 0 } }
                onActiveTranslationChanged: {
                    var dx = activeTranslation.x - lastTx
                    var dy = activeTranslation.y - lastTy
                    lastTx = activeTranslation.x
                    lastTy = activeTranslation.y
                    var zoom = (root.parent && root.parent.scale) ? root.parent.scale : 1.0
                    root.moveDelta(dx / zoom, dy / zoom)
                }
            }
            PointHandler {
                acceptedButtons: Qt.LeftButton
                onActiveChanged: { if (!active) root.leftReleased() }
            }
        }
        Rectangle {
            id: borderBottom
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            height: root._borderSlop
            color: "transparent"

            TapHandler {
                acceptedDevices: PointerDevice.Mouse
                acceptedButtons: Qt.LeftButton
                onPressedChanged: { if (pressed) root.mouseInteracted() }
                onSingleTapped: root.clicked(root.instanceId)
                onLongPressed: root.mouseLongPressed(root.instanceId)
            }
            TapHandler {
                acceptedDevices: PointerDevice.TouchScreen
                acceptedButtons: Qt.LeftButton
                onSingleTapped: root.clicked(root.instanceId)
                onLongPressed: root.touchLongPressed(root.instanceId)
            }
            TapHandler {
                acceptedDevices: PointerDevice.Mouse
                acceptedButtons: Qt.RightButton
                onSingleTapped: function(eventPoint) {
                    root.rightClicked(root.instanceId)
                    root.contextMenuRequested(root.instanceId, eventPoint.scenePosition.x, eventPoint.scenePosition.y)
                }
            }
            DragHandler {
                acceptedDevices: PointerDevice.Mouse
                target: null
                acceptedButtons: Qt.LeftButton
                grabPermissions: PointerHandler.CanTakeOverFromHandlersOfDifferentType
                property real lastTx: 0
                property real lastTy: 0
                onActiveChanged: { if (active) { lastTx = 0; lastTy = 0 } }
                onActiveTranslationChanged: {
                    var dx = activeTranslation.x - lastTx
                    var dy = activeTranslation.y - lastTy
                    lastTx = activeTranslation.x
                    lastTy = activeTranslation.y
                    var zoom = (root.parent && root.parent.scale) ? root.parent.scale : 1.0
                    root.moveDelta(dx / zoom, dy / zoom)
                }
            }
            PointHandler {
                acceptedButtons: Qt.LeftButton
                onActiveChanged: { if (!active) root.leftReleased() }
            }
        }
        Rectangle {
            id: borderLeft
            anchors.left: parent.left
            anchors.top: titleBar.bottom
            anchors.bottom: parent.bottom
            width: root._borderSlop
            color: "transparent"

            TapHandler {
                acceptedDevices: PointerDevice.Mouse
                acceptedButtons: Qt.LeftButton
                onPressedChanged: { if (pressed) root.mouseInteracted() }
                onSingleTapped: root.clicked(root.instanceId)
                onLongPressed: root.mouseLongPressed(root.instanceId)
            }
            TapHandler {
                acceptedDevices: PointerDevice.TouchScreen
                acceptedButtons: Qt.LeftButton
                onSingleTapped: root.clicked(root.instanceId)
                onLongPressed: root.touchLongPressed(root.instanceId)
            }
            TapHandler {
                acceptedDevices: PointerDevice.Mouse
                acceptedButtons: Qt.RightButton
                onSingleTapped: function(eventPoint) {
                    root.rightClicked(root.instanceId)
                    root.contextMenuRequested(root.instanceId, eventPoint.scenePosition.x, eventPoint.scenePosition.y)
                }
            }
            DragHandler {
                acceptedDevices: PointerDevice.Mouse
                target: null
                acceptedButtons: Qt.LeftButton
                grabPermissions: PointerHandler.CanTakeOverFromHandlersOfDifferentType
                property real lastTx: 0
                property real lastTy: 0
                onActiveChanged: { if (active) { lastTx = 0; lastTy = 0 } }
                onActiveTranslationChanged: {
                    var dx = activeTranslation.x - lastTx
                    var dy = activeTranslation.y - lastTy
                    lastTx = activeTranslation.x
                    lastTy = activeTranslation.y
                    var zoom = (root.parent && root.parent.scale) ? root.parent.scale : 1.0
                    root.moveDelta(dx / zoom, dy / zoom)
                }
            }
            PointHandler {
                acceptedButtons: Qt.LeftButton
                onActiveChanged: { if (!active) root.leftReleased() }
            }
        }
        Rectangle {
            id: borderRight
            anchors.right: parent.right
            anchors.top: titleBar.bottom
            anchors.bottom: parent.bottom
            width: root._borderSlop
            color: "transparent"

            TapHandler {
                acceptedDevices: PointerDevice.Mouse
                acceptedButtons: Qt.LeftButton
                onPressedChanged: { if (pressed) root.mouseInteracted() }
                onSingleTapped: root.clicked(root.instanceId)
                onLongPressed: root.mouseLongPressed(root.instanceId)
            }
            TapHandler {
                acceptedDevices: PointerDevice.TouchScreen
                acceptedButtons: Qt.LeftButton
                onSingleTapped: root.clicked(root.instanceId)
                onLongPressed: root.touchLongPressed(root.instanceId)
            }
            TapHandler {
                acceptedDevices: PointerDevice.Mouse
                acceptedButtons: Qt.RightButton
                onSingleTapped: function(eventPoint) {
                    root.rightClicked(root.instanceId)
                    root.contextMenuRequested(root.instanceId, eventPoint.scenePosition.x, eventPoint.scenePosition.y)
                }
            }
            DragHandler {
                acceptedDevices: PointerDevice.Mouse
                target: null
                acceptedButtons: Qt.LeftButton
                grabPermissions: PointerHandler.CanTakeOverFromHandlersOfDifferentType
                property real lastTx: 0
                property real lastTy: 0
                onActiveChanged: { if (active) { lastTx = 0; lastTy = 0 } }
                onActiveTranslationChanged: {
                    var dx = activeTranslation.x - lastTx
                    var dy = activeTranslation.y - lastTy
                    lastTx = activeTranslation.x
                    lastTy = activeTranslation.y
                    var zoom = (root.parent && root.parent.scale) ? root.parent.scale : 1.0
                    root.moveDelta(dx / zoom, dy / zoom)
                }
            }
            PointHandler {
                acceptedButtons: Qt.LeftButton
                onActiveChanged: { if (!active) root.leftReleased() }
            }
        }

        // ── Issue #805 评论 5907045450 第 3 部分：contentViewport ──
        // 中间区域，父 Embed 不挂 TapHandler/DragHandler/MouseArea，
        // 事件直接给 child Scene。
        Item {
            id: contentViewport
            anchors.left: borderLeft.right
            anchors.right: borderRight.left
            anchors.top: titleBar.bottom
            anchors.bottom: borderBottom.top
            clip: true

            // Issue #805 评论 5907045450 第 2 部分：递归渲染子 StarMapScene。
            // childPath = parent.pathSegments + EnterEmbed(embed.instanceId)。
            // 子 Scene 自己有 viewport 和手势状态。
            Loader {
                id: childSceneLoader
                anchors.fill: parent
                // 只在该 Embed 真正进入父 Scene 视口后才创建递归 child Scene。
                // asynchronous 避免一帧里同步构造多层 QML 对象树把 GUI 线程堵死。
                active: root.childSceneActivated
                        && root.targetStarmapId.length > 0
                        && root.rootStarmapId.length > 0
                asynchronous: true
                sourceComponent: starmapSceneComponent
            }
        }
    }

    // Issue #805 评论 5907045450 第 2 部分：子 StarMapScene 组件。
    Component {
        id: starmapSceneComponent
        StarMapScene {
            dt: root.dt
            starmapBackendRef: root.starmapBackendRef
            rootStarmapId: root.rootStarmapId
            pathSegments: root.parentPathSegments.concat([
                { type: "enterEmbed", instanceId: root.instanceId, nodeId: null }
            ])
            // Issue #805 评论 5908703621 问题 1：child Scene 的 pathKey 不能只写
            // root.instanceId，要用父路径继续拼，保证全局唯一且体现层级。
            pathKey: root.parentPathKey + "/embed_" + root.instanceId

            // Issue #805 评论 5908703621 问题 5：child Scene 的 editNodeRequested
            // 继续向父 Scene / Workspace 冒泡。已带正确的 owner 上下文，原样转发。
            // Issue #805 评论 5912394108：ownerScene 一并原样转发，保持指向真正
            // 拥有该节点的子 Scene，不被 Embed / 父 Scene 替换。
            onEditNodeRequested: function(ownerScene, ownerStarmapId, ownerPathKey, node) {
                root.editNodeRequested(ownerScene, ownerStarmapId, ownerPathKey, node)
            }
        }
    }

    // wobble 降速，和 StarMapNode.qml 一致；选中/按下/拖动时动画暂停
    SequentialAnimation on _wobbleAnimX {
        loops: Animation.Infinite
        running: !isSelected && !isBeingDragged && !chromeMouseTap.pressed && !chromeTouchTap.pressed
        NumberAnimation { to: 0.6; duration: 7000 + (wobbleIndex % 7) * 400; easing.type: Easing.InOutSine }
        NumberAnimation { to: -0.6; duration: 7000 + (wobbleIndex % 7) * 400; easing.type: Easing.InOutSine }
    }
    SequentialAnimation on _wobbleAnimY {
        loops: Animation.Infinite
        running: !isSelected && !isBeingDragged && !chromeMouseTap.pressed && !chromeTouchTap.pressed
        NumberAnimation { to: 0.4; duration: 8500 + (wobbleIndex % 5) * 300; easing.type: Easing.InOutSine }
        NumberAnimation { to: -0.4; duration: 8500 + (wobbleIndex % 5) * 300; easing.type: Easing.InOutSine }
    }

    // ---------------------------------------------------------------------------
    // Issue #805 评论 5908703621 问题 2：所有 PointerHandler 已移进 titleBar /
    // border 内部（parent Item 决定命中范围）。根 Item 和 contentViewport 祖先链
    // 上不再有任何 TapHandler / DragHandler / MouseArea / PointHandler，
    // 内部事件直接给 child Scene，不会被父 Embed 截走。
    // ---------------------------------------------------------------------------
}
