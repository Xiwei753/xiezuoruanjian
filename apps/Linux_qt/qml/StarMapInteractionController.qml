// =============================================================================
// StarMapInteractionController.qml — 星图鼠标交互状态机
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：只持有瞬时手势状态（pointerMode / connect 源 / move 目标 / pan），
//   不读写 Core，不保存节点数据。#373 的鼠标规则只有一套状态机。
// 约束：
//   - 纯交互状态切换，不触碰 graphController，不持久化任何节点/边
//   - Canvas 收到状态变化后决定调哪个图操作（建边/提交移动等）
// =============================================================================
import QtQuick

QtObject {
    id: interaction

    // 鼠标状态机：idle / pan / connect / move
    //   idle    — 无活跃拖拽手势
    //   pan     — 长按空白或中键后拖动，平移画布
    //   connect — 长按节点/Embed 后拖动，拉线预览
    //   move    — 右键菜单"移动"后左键拖动，仅移动指定节点/Embed
    property string pointerMode: "idle"

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

    // ── pan ──
    function beginPan() { pointerMode = "pan" }
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

    // ── move ──
    function beginMove(nodeId) {
        if (pointerMode !== "idle") return false
        pointerMode = "move"
        pressedNodeId = nodeId
        return true
    }
    function beginEmbedMove(instanceId) {
        if (pointerMode !== "idle") return false
        pointerMode = "move"
        pressedEmbedId = instanceId
        return true
    }
    function endMove() {
        pointerMode = "idle"
        pressedNodeId = ""
        pressedEmbedId = ""
    }

    // 统一复位所有瞬时状态（切换星图/失焦等场景调用）
    function reset() {
        pointerMode = "idle"
        connectFromKind = ""
        connectFromId = ""
        connectFromPath = null
        connectFromNodeId = ""
        pressedNodeId = ""
        pressedEmbedId = ""
    }
}
