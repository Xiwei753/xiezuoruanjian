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
//   - 不用 findEmbedAt() 把整个矩形都判成 Embed 命中。
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
        }

        // ── Issue #805 评论 5907045450 第 3 部分：四条边框命中区域 ──
        // 边框有少量 hit slop，命中时选择/移动作用于 Embed。
        Rectangle {
            id: borderTop
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.top: parent.top
            height: root._borderSlop
            color: "transparent"
        }
        Rectangle {
            id: borderBottom
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            height: root._borderSlop
            color: "transparent"
        }
        Rectangle {
            id: borderLeft
            anchors.left: parent.left
            anchors.top: titleBar.bottom
            anchors.bottom: parent.bottom
            width: root._borderSlop
            color: "transparent"
        }
        Rectangle {
            id: borderRight
            anchors.right: parent.right
            anchors.top: titleBar.bottom
            anchors.bottom: parent.bottom
            width: root._borderSlop
            color: "transparent"
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
                active: root.targetStarmapId.length > 0 && root.rootStarmapId.length > 0
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
            pathKey: root.instanceId
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
    // Issue #805 评论 5907045450 第 3 部分：交互 handler 只挂在 chrome 区域。
    // contentViewport 不挂 handler，事件穿透给 child Scene。
    // ---------------------------------------------------------------------------
    TapHandler {
        id: chromeMouseTap
        acceptedDevices: PointerDevice.Mouse
        acceptedButtons: Qt.LeftButton
        exclusiveSignals: TapHandler.SingleTap | TapHandler.DoubleTap
        target: titleBar

        onPressedChanged: { if (pressed) root.mouseInteracted() }

        onSingleTapped: root.clicked(root.instanceId)
        onLongPressed: root.mouseLongPressed(root.instanceId)
    }

    TapHandler {
        id: chromeTouchTap
        acceptedDevices: PointerDevice.TouchScreen
        acceptedButtons: Qt.LeftButton
        exclusiveSignals: TapHandler.SingleTap | TapHandler.DoubleTap
        target: titleBar

        onSingleTapped: root.clicked(root.instanceId)
        onLongPressed: root.touchLongPressed(root.instanceId)
    }

    TapHandler {
        acceptedDevices: PointerDevice.Mouse
        acceptedButtons: Qt.RightButton
        target: titleBar
        onPressedChanged: { if (pressed) root.mouseInteracted() }
        onSingleTapped: function(eventPoint) {
            root.rightClicked(root.instanceId)
            root.contextMenuRequested(root.instanceId, eventPoint.scenePosition.x, eventPoint.scenePosition.y)
        }
    }

    // 边框命中：单击选择 Embed（与标题条对称）
    TapHandler {
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchScreen
        acceptedButtons: Qt.LeftButton
        target: borderTop
        onSingleTapped: root.clicked(root.instanceId)
    }
    TapHandler {
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchScreen
        acceptedButtons: Qt.LeftButton
        target: borderBottom
        onSingleTapped: root.clicked(root.instanceId)
    }
    TapHandler {
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchScreen
        acceptedButtons: Qt.LeftButton
        target: borderLeft
        onSingleTapped: root.clicked(root.instanceId)
    }
    TapHandler {
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchScreen
        acceptedButtons: Qt.LeftButton
        target: borderRight
        onSingleTapped: root.clicked(root.instanceId)
    }

    // ---------------------------------------------------------------------------
    // 拖动跟踪：DragHandler 只挂在 chrome 区域，只上抛原始移动增量。
    // contentViewport 内的拖动由 child Scene 处理（子图 pan）。
    // ---------------------------------------------------------------------------
    DragHandler {
        id: embedDragHandler
        acceptedDevices: PointerDevice.Mouse
        target: null
        acceptedButtons: Qt.LeftButton
        grabPermissions: PointerHandler.CanTakeOverFromHandlersOfDifferentType

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
            var zoom = (root.parent && root.parent.scale) ? root.parent.scale : 1.0
            root.moveDelta(dx / zoom, dy / zoom)
        }
    }

    // 左键 press→release 观察：PointHandler 用 passive grab（挂在 chrome）
    PointHandler {
        id: leftPointTracker
        acceptedButtons: Qt.LeftButton
        target: titleBar

        onActiveChanged: {
            if (!active) {
                root.leftReleased()
            }
        }
    }
}
