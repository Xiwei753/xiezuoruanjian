// =============================================================================
// StarMapInteractionController.qml — 星图全局手势状态机
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：只持有瞬时手势状态（pointerMode / press 归属 / move 目标 / connect 源），
//   不读写 Core，不保存节点数据。#373 的交互规则只有一套状态机。
//
// Issue #832：整棵星图只有一个原始输入主人 StarMapInputRouter。
//   - press / move / connect 身份都保存完整的
//     scenePathKey + kind + id + targetPath，不再退化成裸 nodeId；
//   - 状态提升（pressPending -> move / connect / contextPending / pan）只允许
//     唯一 Router 调用，递归 delegate 不再各自决定；
//   - 长按计时不在这里发信号，超时由 Router 自己的 Timer 触发后调用这里的
//     提升入口；
//   - Node / Embed 的视觉动画通过 isPressedTarget() / isMovingTarget() 查询，
//     不再依赖组件自己的 TapHandler.pressed。
//
// 坐标约定：
//   - connect 相关坐标全部存 scene 坐标（整棵递归树的顶层 world 坐标），
//     这样预览线只存在于根 Canvas 的一层 overlay，不需要每层各画一条。
//   - move 相关坐标存"归属层"的局部坐标，提交时直接写回该层的 GraphController。
//
// 约束：
//   - 纯交互状态切换，不触碰 graphController，不持久化任何节点/边
//   - 长按计时用系统 mousePressAndHoldInterval，由 Router 注入
// =============================================================================
import QtQuick

QtObject {
    id: interaction

    // 输入来源："" / "mouse" / "touch"
    //   Router 在按下时设置，用于区分鼠标和触屏行为
    property string pointerSource: ""

    // 交互状态机：idle / pressPending / pan / connect / move / contextPending / pinch
    //   idle           — 无活跃手势
    //   pressPending   — 已按下但还没决定是 move / connect / 平移（按下仲裁中）
    //   pan            — 平移全局相机
    //   connect        — 长按节点/Embed 后拖动，拉线预览
    //   move           — 移动节点/Embed
    //   contextPending — 触屏长按后等待：不移动弹菜单，移动超阈值转 connect
    //   pinch          — 双指缩放接管：单指业务状态已全部清空，缩放期间不再有业务
    property string pointerMode: "idle"

    // 拖动阈值与长按阈值。
    // dragThreshold 走 Qt 的拖动判定语义（scene 像素）；
    // longPressInterval 由 Router 注入系统 mousePressAndHoldInterval。
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

    // 长按计时开关：Timer 由唯一 Router 持有，触发后由 Router 调提升入口。
    property bool pressTimerActive: false

    // ── connect 模式源端（完整身份）──
    // "node" 或 "embed"
    property string connectFromKind: ""
    // nodeId 或 instanceId
    property string connectFromId: ""
    // StarMapTargetPathDto JS 对象
    property var connectFromPath: null
    // 源端所属层的 pathKey，松手时确认不是自己连自己
    property string connectFromScenePathKey: ""
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

    // ── Issue #834：菜单发起的 armed 态 ──
    // linkArmed：右键菜单"内部链接 → 选择目标"后进入，下一次点 node/embed 是 target。
    //   Link 是内部跳转（StarMapLink），不是语义边，不画预览线。
    // connectArmed：右键菜单"连线"后进入，source 已知，等用户点 target 拉线。
    //   复用 connect 模式预览，pointerMode 设 "connect"，但不伪造 pressPending。
    property bool linkArmed: false
    property string linkFromKind: ""
    property string linkFromId: ""
    property var linkFromPath: null
    property string linkFromScenePathKey: ""

    property bool connectArmed: false

    // ── move 模式目标（归属层局部坐标 + 完整身份）──
    property string moveScenePathKey: ""
    property string moveKind: ""    // "node" / "embed"
    property string moveId: ""
    property var moveTargetPath: null
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
    function noteDragDelta(dx, dy) {
        if (pointerMode !== "pressPending" && pointerMode !== "contextPending")
            return
        pressDragX += dx
        pressDragY += dy
        pressDragDistance = Math.hypot(pressDragX, pressDragY)
    }

    // pressPending -> move：先超过拖动阈值。
    // 完整身份（scenePathKey / kind / id / targetPath）一并落进 move 状态。
    function pressPendingToMove(scenePathKey, kind, id, targetPath, startX, startY) {
        if (pointerMode !== "pressPending")
            return false
        if (pressScenePathKey !== scenePathKey || pressKind !== kind || pressId !== id)
            return false
        pressTimerActive = false
        pointerMode = "move"
        moveScenePathKey = scenePathKey
        moveKind = kind
        moveId = id
        moveTargetPath = targetPath
        moveX = startX
        moveY = startY
        return true
    }

    // pressPending -> connect：先到长按时间且没超拖动阈值。
    function pressPendingToConnect(kind, id, targetPath, centerSceneX, centerSceneY) {
        if (pointerMode !== "pressPending")
            return false
        if (pressKind !== kind || pressId !== id)
            return false
        pressTimerActive = false
        pointerMode = "connect"
        connectFromKind = kind
        connectFromId = id
        connectFromPath = targetPath
        connectFromScenePathKey = pressScenePathKey
        connectFromSceneX = centerSceneX
        connectFromSceneY = centerSceneY
        connectMouseX = centerSceneX
        connectMouseY = centerSceneY
        connectPreviewEndX = centerSceneX
        connectPreviewEndY = centerSceneY
        return true
    }

    // pressPending -> contextPending：触屏长按预备态。
    // 身份从 press 状态原样搬过来，不移动则弹菜单，移动超阈值转 connect。
    function pressPendingToContextPending(centerSceneX, centerSceneY) {
        if (pointerMode !== "pressPending")
            return false
        pressTimerActive = false
        pointerMode = "contextPending"
        pointerSource = "touch"
        connectFromKind = pressKind
        connectFromId = pressId
        connectFromPath = pressTargetPath
        connectFromScenePathKey = pressScenePathKey
        connectFromSceneX = centerSceneX
        connectFromSceneY = centerSceneY
        connectMouseX = centerSceneX
        connectMouseY = centerSceneY
        connectPreviewEndX = centerSceneX
        connectPreviewEndY = centerSceneY
        return true
    }

    // pressPending -> pan：触屏"未长按直接滑动"优先平移。
    // 起点是不是 node/embed 都一样；单指业务状态在这里整体让位给相机。
    function pressPendingToPan() {
        if (pointerMode !== "pressPending")
            return false
        pressTimerActive = false
        pointerMode = "pan"
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
    // 只有 idle（空白按下后的拖动）才允许进入 pan；
    // 触屏从 node/embed 按下出发的滑动走 pressPendingToPan()。
    function beginPan() {
        if (pointerMode !== "idle")
            return false
        pressTimerActive = false
        pointerMode = "pan"
        return true
    }
    function endPan() { if (pointerMode === "pan") pointerMode = "idle" }

    // ── pinch（双指缩放优先）──
    // 双指一旦激活就整体接管：先清单指留下的瞬时现场再进 pinch；
    // 缩放结束后整体复位。
    function beginPinch() {
        reset()
        pointerMode = "pinch"
        pointerSource = "touch"
    }
    function endPinch() {
        if (pointerMode === "pinch")
            reset()
    }

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
        connectArmed = false
        connectFromKind = ""
        connectFromId = ""
        connectFromPath = null
        connectFromScenePathKey = ""
        connectPreviewEndX = 0
        connectPreviewEndY = 0
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
        pressDragX = 0
        pressDragY = 0
        pressDragDistance = 0
        return { kind: kind, id: id }
    }

    // ── Issue #834：菜单发起的 armed 态入口 ──
    // 菜单"内部链接 → 选择目标"进入 linkArmed。不画预览（Link 不是语义边），
    // pointerMode 保持 idle，下一次 tap 由 Router 路由到 finishLink。
    function beginLinkArmed(kind, id, sourcePath, scenePathKey) {
        linkArmed = true
        linkFromKind = kind
        linkFromId = id
        linkFromPath = sourcePath
        linkFromScenePathKey = scenePathKey
    }

    // 菜单"连线"进入 connectArmed。source 已知，等用户点 target。
    // 复用 connect 模式预览：pointerMode 设 "connect"，connectFrom* 落 source，
    // 但不伪造 pressPending（菜单发起，没有真实按下流）。
    function beginConnectFromMenu(kind, id, sourcePath, scenePathKey, sceneX, sceneY) {
        connectArmed = true
        pressTimerActive = false
        pointerMode = "connect"
        connectFromKind = kind
        connectFromId = id
        connectFromPath = sourcePath
        connectFromScenePathKey = scenePathKey
        connectFromSceneX = sceneX
        connectFromSceneY = sceneY
        connectMouseX = sceneX
        connectMouseY = sceneY
        connectPreviewEndX = sceneX
        connectPreviewEndY = sceneY
    }

    // 取消 armed 态（点空白 / Escape / 右键再开菜单）。
    // linkArmed 与 connectArmed 都走这里，清干净后回 idle。
    function cancelArmed() {
        linkArmed = false
        linkFromKind = ""
        linkFromId = ""
        linkFromPath = null
        linkFromScenePathKey = ""
        connectArmed = false
        if (pointerMode === "connect")
            endConnect()
    }

    // ── move ──
    // 右键菜单"移动"直接进 move（不进 pressPending：菜单已经确定了目标），
    // 完整身份与手势路径同源。
    function beginMove(kind, id, targetPath, scenePathKey, startX, startY) {
        if (pointerMode !== "idle")
            return false
        pressTimerActive = false
        pointerMode = "move"
        moveScenePathKey = scenePathKey
        moveKind = kind
        moveId = id
        moveTargetPath = targetPath
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
        moveKind = ""
        moveId = ""
        moveTargetPath = null
        moveX = 0
        moveY = 0
    }

    // ── 视觉查询（Node / Embed 动画绑定用）──
    // 当前手势是否仍按在这个目标上（pressPending / contextPending / connect /
    // move 都算：四态身份都指向同一个被按住的对象）。
    function isPressedTarget(scenePathKey, kind, id) {
        if (pointerMode === "pressPending" || pointerMode === "contextPending")
            return pressScenePathKey === scenePathKey && pressKind === kind && pressId === id
        if (pointerMode === "connect")
            return connectFromScenePathKey === scenePathKey
                && connectFromKind === kind && connectFromId === id
        if (pointerMode === "move")
            return moveScenePathKey === scenePathKey && moveKind === kind && moveId === id
        return false
    }

    // 当前是否正在移动这个目标（显示位置读 moveX/moveY 的唯一判据）。
    function isMovingTarget(scenePathKey, kind, id) {
        return pointerMode === "move"
                && moveScenePathKey === scenePathKey
                && moveKind === kind
                && moveId === id
    }

    // 统一复位所有瞬时状态（切换星图/失焦/双指接管等场景调用）
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
        connectPreviewEndX = 0
        connectPreviewEndY = 0
        moveScenePathKey = ""
        moveKind = ""
        moveId = ""
        moveTargetPath = null
        moveX = 0
        moveY = 0
        linkArmed = false
        linkFromKind = ""
        linkFromId = ""
        linkFromPath = null
        linkFromScenePathKey = ""
        connectArmed = false
    }
}
