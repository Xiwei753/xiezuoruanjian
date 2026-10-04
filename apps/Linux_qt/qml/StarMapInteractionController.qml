// =============================================================================
// StarMapInteractionController.qml — 星图全局手势状态机
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：只持有瞬时手势状态（pointerMode / press 归属 / connect 源 / move 目标），
//   不读写 Core，不保存节点数据。#373 的交互规则只有一套状态机。
//
// Issue #822：由根 StarMapCanvas 唯一创建一次，整棵递归树共享同一个实例。
// 各层 StarMapSceneContent 只把"按下去的对象是不是我的"和位移增量喂进来，
// 状态只能被本状态机提升一次，delegate 自己的 TapHandler/DragHandler
// 不再各自决定 move 还是 connect。
//
// Issue #822 修的核心 bug：
//   旧实现里 idle -> move 和 idle -> connect 抢同一个左键手势。DragHandler 一有
//   位移就先 beginMove，等 TapHandler.onLongPressed 再调 beginConnect 时状态已经
//   不是 idle，beginConnect 直接返回 false，connect 永远进不去
//   （诊断包 371 次 starmap.* 里 connect_begin=0、connect_end=0）。
//   新实现改成显式的按下仲裁：
//     idle -> pressPending
//     pressPending -> move      （先超过拖动阈值）
//     pressPending -> connect   （先到系统长按时间且没超阈值）
//   松手统一从一个出口闭环：click / move / connect 都不会各走各的。
//
// 坐标约定：
//   - connect 相关坐标全部存 scene 坐标（整棵递归树的顶层 world 坐标），
//     这样预览线只存在于根 Canvas 的一层 overlay，不需要每层各画一条。
//   - move 相关坐标存"归属层"的局部坐标，提交时直接写回该层的 GraphController。
//
// 约束：
//   - 纯交互状态切换，不触碰 graphController，不持久化任何节点/边
//   - 长按计时用系统 mousePressAndHoldInterval，由根 Canvas 注入
// =============================================================================
import QtQuick

QtObject {
    id: interaction

    // 输入来源："" / "mouse" / "touch"
    //   Canvas 在开始交互时设置此属性，用于区分鼠标和触屏行为
    property string pointerSource: ""

    // 交互状态机：idle / pressPending / pan / connect / move / contextPending
    //   idle           — 无活跃手势
    //   pressPending   — 已按下但还没决定是 move 还是 connect（按下仲裁中）
    //   pan            — 背景拖动或中键拖动，平移全局相机
    //   connect        — 长按节点/Embed 后拖动，拉线预览
    //   move           — 超过拖动阈值后移动节点/Embed
    //   contextPending — 触屏长按后等待：不移动弹菜单，移动超阈值转 connect
    property string pointerMode: "idle"

    // Issue #822：拖动阈值与长按阈值。
    // dragThreshold 走 Qt 的拖动判定语义（scene 像素）；
    // longPressInterval 由根 Canvas 注入系统 mousePressAndHoldInterval。
    readonly property real dragThreshold: 8.0
    property real longPressInterval: 800

    // 触屏长按后移动超过此阈值才从 contextPending 转 connect。
    // 单位是原始 Qt scene 像素，和 dragThreshold 同一口径。
    readonly property real moveThreshold: 10.0

    // ── pressPending：按下归属（完整 targetPath + 归属层 pathKey）──
    property string pressScenePathKey: ""
    property string pressKind: ""   // "node" / "embed"
    property string pressId: ""
    // StarMapTargetPathDto JS 对象（完整路径，不退化成 nodeId）
    property var pressTargetPath: null
    property real pressSceneX: 0
    property real pressSceneY: 0
    // 按下后累计的屏幕位移分量（原始 Qt scene 像素，不做任何 world/fit 换算）。
    // pressDragDistance 是"离按下点的直线距离" = |(pressDragX, pressDragY)|；
    // 不是每次增量的长度累加 —— 连续两次同方向 5px 必须算 10px 而不是 ~7px。
    property real pressDragX: 0
    property real pressDragY: 0
    property real pressDragDistance: 0

    // 长按计时开关：Timer 必须挂在 Item 下（QtObject 没有默认属性），
    // 真正的 Timer 由根 StarMapCanvas 持有，触发后回调 pressTimeout()，
    // 状态提升由手势归属层完成。
    property bool pressTimerActive: false

    signal pressTimeout()

    // connect 模式源端
    // "node" 或 "embed"
    property string connectFromKind: ""
    // nodeId 或 instanceId
    property string connectFromId: ""
    // StarMapTargetPathDto JS 对象
    property var connectFromPath: null
    // 源端所属层的 pathKey，松手时确认不是自己连自己
    property string connectFromScenePathKey: ""
    // 保留给预览线绘制兼容路径
    property string connectFromNodeId: ""
    // 源端中心 + 当前终点，全部 scene 坐标
    property real connectFromSceneX: 0
    property real connectFromSceneY: 0
    property real connectMouseX: 0
    property real connectMouseY: 0
    // 预览线实际画出来的端点（scene 坐标）：悬停在合法 target 上时贴到目标的
    // 宿主可见边界，松手时正式边与预览不再跳变。connectMouseX/Y 保持原始鼠标
    // 位置，松手命中仍用它。没有合法 target 时等于 connectMouse。
    property real connectPreviewEndX: 0
    property real connectPreviewEndY: 0

    // move 模式目标（归属层局部坐标）
    property string moveScenePathKey: ""
    property string pressedNodeId: ""
    property string pressedEmbedId: ""
    // move 模式当前临时坐标（拖动期间未提交的显示位置）
    property real moveX: 0
    property real moveY: 0

    // ── 按下仲裁 ──
    // 只在 idle 受理按下。登记归属和坐标，不移动也不连线。
    function beginPress(kind, id, targetPath, scenePathKey, sceneX, sceneY) {
        if (pointerMode !== "idle")
            return false
        pointerMode = "pressPending"
        pressScenePathKey = scenePathKey
        pressKind = kind
        pressId = id
        pressTargetPath = targetPath
        pressSceneX = sceneX
        pressSceneY = sceneY
        pressDragX = 0
        pressDragY = 0
        pressDragDistance = 0
        pressTimerActive = true
        return true
    }

    // 累计按下后的位移。dx/dy 必须是原始 Qt scene 像素：阈值 8px 是屏幕口径，
    // 换算成 world 单位后再判断会随全局缩放放大/缩小。
    // 是否超阈值由归属层判断后调用提升。
    function noteDragDelta(dx, dy) {
        if (pointerMode !== "pressPending" && pointerMode !== "contextPending")
            return
        pressDragX += dx
        pressDragY += dy
        pressDragDistance = Math.hypot(pressDragX, pressDragY)
    }

    // pressPending -> move：先超过拖动阈值。
    function pressPendingToMove(kind, id, scenePathKey, startX, startY) {
        if (pointerMode !== "pressPending")
            return false
        if (pressKind !== kind || pressId !== id || pressScenePathKey !== scenePathKey)
            return false
        pressTimerActive = false
        pointerMode = "move"
        moveScenePathKey = scenePathKey
        pressedNodeId = kind === "node" ? id : ""
        pressedEmbedId = kind === "embed" ? id : ""
        moveX = startX
        moveY = startY
        return true
    }

    // pressPending -> connect：先到长按时间且没超拖动阈值。
    function pressPendingToConnect(kind, id, targetPath, centerSceneX, centerSceneY) {
        if (pointerMode !== "pressPending")
            return false
        pressTimerActive = false
        pointerMode = "connect"
        connectFromKind = kind
        connectFromId = id
        connectFromPath = targetPath
        connectFromScenePathKey = pressScenePathKey
        connectFromNodeId = kind === "node" ? id : ""
        connectFromSceneX = centerSceneX
        connectFromSceneY = centerSceneY
        connectMouseX = centerSceneX
        connectMouseY = centerSceneY
        connectPreviewEndX = centerSceneX
        connectPreviewEndY = centerSceneY
        return true
    }

    // pressPending 松手：只是普通点击，不移动也不连线。
    function cancelPressPending() {
        if (pointerMode !== "pressPending")
            return
        pressTimerActive = false
        pointerMode = "idle"
        pressScenePathKey = ""
        pressKind = ""
        pressId = ""
        pressTargetPath = null
        pressDragX = 0
        pressDragY = 0
        pressDragDistance = 0
    }

    // ── pan ──
    // Issue #798 评论 5892406254: 不在内部 reset 一个正在进行的 move/connect，
    // 那会只清状态机不清几何缓存。只有 idle 才允许进入 pan；
    // 真正的取消必须从 Canvas 的 resetInteraction() 走，transient UI 和
    // edge cache 一起清掉。
    function beginPan() {
        if (pointerMode !== "idle")
            return false
        pressTimerActive = false
        pointerMode = "pan"
        return true
    }
    function endPan() { if (pointerMode === "pan") pointerMode = "idle" }

    // ── connect ──
    // connect 阶段的移动只更新全局预览线终点（scene 坐标）。
    function updateConnect(sx, sy) {
        if (pointerMode !== "connect")
            return
        connectMouseX = sx
        connectMouseY = sy
    }
    function endConnect() {
        pressTimerActive = false
        pointerMode = "idle"
        connectFromKind = ""
        connectFromId = ""
        connectFromPath = null
        connectFromScenePathKey = ""
        connectFromNodeId = ""
        connectPreviewEndX = 0
        connectPreviewEndY = 0
    }

    // ── contextPending（触屏长按预备态）──
    // 触屏长按节点/子星图时进入此状态：不移动则弹菜单，移动超过阈值则转 connect
    function beginContextPending(kind, id, targetPath, scenePathKey, centerSceneX, centerSceneY) {
        if (pointerMode !== "idle")
            return false
        pointerMode = "contextPending"
        pointerSource = "touch"
        connectFromKind = kind
        connectFromId = id
        connectFromPath = targetPath
        connectFromScenePathKey = scenePathKey
        connectFromNodeId = kind === "node" ? id : ""
        connectFromSceneX = centerSceneX
        connectFromSceneY = centerSceneY
        connectMouseX = centerSceneX
        connectMouseY = centerSceneY
        connectPreviewEndX = centerSceneX
        connectPreviewEndY = centerSceneY
        pressDragX = 0
        pressDragY = 0
        pressDragDistance = 0
        return true
    }

    // 从 contextPending 转为 connect（触屏移动超过阈值后）
    function contextPendingToConnect() {
        if (pointerMode !== "contextPending")
            return false
        pointerMode = "connect"
        return true
    }

    // contextPending 取消（触屏松手，不移动，弹菜单）
    // 返回之前的状态信息，让调用方知道该弹谁的菜单
    function endContextPending() {
        if (pointerMode !== "contextPending")
            return false
        var kind = connectFromKind
        var id = connectFromId
        pointerMode = "idle"
        connectFromKind = ""
        connectFromId = ""
        connectFromPath = null
        connectFromScenePathKey = ""
        connectFromNodeId = ""
        pressDragX = 0
        pressDragY = 0
        pressDragDistance = 0
        return { kind: kind, id: id }
    }

    // ── move ──
    // 右键菜单"移动"直接进 move（不进 pressPending：菜单已经确定了目标）。
    function beginMove(nodeId, scenePathKey, startX, startY) {
        if (pointerMode !== "idle")
            return false
        pressTimerActive = false
        pointerMode = "move"
        moveScenePathKey = scenePathKey
        pressedNodeId = nodeId
        pressedEmbedId = ""
        moveX = startX
        moveY = startY
        return true
    }
    function beginEmbedMove(instanceId, scenePathKey, startX, startY) {
        if (pointerMode !== "idle")
            return false
        pressTimerActive = false
        pointerMode = "move"
        moveScenePathKey = scenePathKey
        pressedNodeId = ""
        pressedEmbedId = instanceId
        moveX = startX
        moveY = startY
        return true
    }
    function updateMove(x, y) {
        if (pointerMode !== "move")
            return
        moveX = x
        moveY = y
    }
    function endMove() {
        pressTimerActive = false
        pointerMode = "idle"
        moveScenePathKey = ""
        pressedNodeId = ""
        pressedEmbedId = ""
        moveX = 0
        moveY = 0
    }

    // 统一复位所有瞬时状态（切换星图/失焦等场景调用）
    function reset() {
        pressTimerActive = false
        pointerMode = "idle"
        pointerSource = ""
        pressScenePathKey = ""
        pressKind = ""
        pressId = ""
        pressTargetPath = null
        pressDragX = 0
        pressDragY = 0
        pressDragDistance = 0
        connectFromKind = ""
        connectFromId = ""
        connectFromPath = null
        connectFromScenePathKey = ""
        connectFromNodeId = ""
        connectPreviewEndX = 0
        connectPreviewEndY = 0
        moveScenePathKey = ""
        pressedNodeId = ""
        pressedEmbedId = ""
        moveX = 0
        moveY = 0
    }
}
