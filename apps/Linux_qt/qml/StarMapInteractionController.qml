// =============================================================================
// StarMapInteractionController.qml — 星图交互状态机
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：只持有瞬时手势状态（pointerMode / connect 源 / move 目标 / pan），
//   不读写 Core，不保存节点数据。#373 的交互规则只有一套状态机。
// 约束：
//   - 纯交互状态切换，不触碰 graphController，不持久化任何节点/边
//   - Canvas 收到状态变化后决定调哪个图操作（建边/提交移动等）
// =============================================================================
import QtQuick

QtObject {
    id: interaction

    // 输入来源："" / "mouse" / "touch"
    //   Canvas 在开始交互时设置此属性，用于区分鼠标和触屏行为
    property string pointerSource: ""

    // 交互状态机：idle / pan / connect / move / contextPending / connectPending
    //   idle           — 无活跃拖拽手势
    //   pan            — 长按空白或中键后拖动，平移画布
    //   connect        — 长按节点/Embed 后拖动，拉线预览
    //   move           — 右键菜单"移动"后左键拖动，仅移动指定节点/Embed
    //   contextPending — 触屏长按后等待：不移动则弹菜单，移动超过阈值则转 connect
    //   connectPending — 触屏长按后已开始移动，准备进入 connect（过渡态）
    property string pointerMode: "idle"

    // 触屏长按后移动超过此阈值才从 contextPending 转 connect
    readonly property real _moveThreshold: 10.0

    // connect 模式源端
    property string connectFromKind: ""   // "node" 或 "embed"
    property string connectFromId: ""     // nodeId 或 instanceId
    property var connectFromPath: null    // StarMapTargetPathDto JS 对象
    property string connectFromNodeId: "" // 保留给预览线绘制兼容路径
    property real connectMouseX: 0
    property real connectMouseY: 0

    // move 模式目标
    property string pressedNodeId: ""
    property string pressedEmbedId: ""
    // move 模式当前临时坐标（拖动期间未提交的显示位置）
    property real moveX: 0
    property real moveY: 0

    // ── pan ──
    function beginPan() {
        // Issue #798 评论 5892406254: 不在内部 reset 一个正在进行的 move/connect，
        // 那会只清状态机不清几何缓存。只有 idle 才允许进入 pan；
        // 真正的取消必须从 Canvas 的 resetInteraction() 走，transient UI 和
        // edge cache 一起清掉。
        if (pointerMode !== "idle") return false
        pointerMode = "pan"
        return true
    }
    function endPan() { if (pointerMode === "pan") pointerMode = "idle" }

    // ── connect ──
    function beginConnect(kind, id, path, startX, startY) {
        if (pointerMode !== "idle") return false
        pointerMode = "connect"
        connectFromKind = kind
        connectFromId = id
        connectFromPath = path
        connectFromNodeId = kind === "node" ? id : ""
        connectMouseX = startX
        connectMouseY = startY
        return true
    }
    function updateConnect(mx, my) {
        if (pointerMode !== "connect") return
        connectMouseX = mx
        connectMouseY = my
    }
    function endConnect() {
        pointerMode = "idle"
        connectFromKind = ""
        connectFromId = ""
        connectFromPath = null
        connectFromNodeId = ""
    }

    // ── contextPending（触屏长按预备态）──
    // 触屏长按节点/子星图时进入此状态：不移动则弹菜单，移动超过阈值则转 connect
    function beginContextPending(kind, id, path, startX, startY) {
        if (pointerMode !== "idle") return false
        pointerMode = "contextPending"
        pointerSource = "touch"
        connectFromKind = kind
        connectFromId = id
        connectFromPath = path
        connectFromNodeId = kind === "node" ? id : ""
        connectMouseX = startX
        connectMouseY = startY
        return true
    }

    // 从 contextPending 转为 connect（触屏移动超过阈值后）
    function contextPendingToConnect() {
        if (pointerMode !== "contextPending") return false
        pointerMode = "connect"
        return true
    }

    // contextPending 取消（触屏松手，不移动，弹菜单）
    // 返回之前的状态信息，让 Canvas 知道该弹谁的菜单
    function endContextPending() {
        if (pointerMode !== "contextPending") return false
        var kind = connectFromKind
        var id = connectFromId
        pointerMode = "idle"
        connectFromKind = ""
        connectFromId = ""
        connectFromPath = null
        connectFromNodeId = ""
        return { kind: kind, id: id }
    }

    // ── move ──
    function beginMove(nodeId, startX, startY) {
        if (pointerMode !== "idle") return false
        pointerMode = "move"
        pressedNodeId = nodeId
        pressedEmbedId = ""
        moveX = startX
        moveY = startY
        return true
    }
    function beginEmbedMove(instanceId, startX, startY) {
        if (pointerMode !== "idle") return false
        pointerMode = "move"
        pressedEmbedId = instanceId
        pressedNodeId = ""
        moveX = startX
        moveY = startY
        return true
    }
    function updateMove(x, y) {
        if (pointerMode !== "move") return
        moveX = x
        moveY = y
    }
    function endMove() {
        pointerMode = "idle"
        pressedNodeId = ""
        pressedEmbedId = ""
        moveX = 0
        moveY = 0
    }

    // 统一复位所有瞬时状态（切换星图/失焦等场景调用）
    function reset() {
        pointerMode = "idle"
        pointerSource = ""
        connectFromKind = ""
        connectFromId = ""
        connectFromPath = null
        connectFromNodeId = ""
        pressedNodeId = ""
        pressedEmbedId = ""
        moveX = 0
        moveY = 0
    }
}
