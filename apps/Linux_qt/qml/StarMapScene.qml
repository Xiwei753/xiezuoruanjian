// =============================================================================
// StarMapScene.qml — 星图场景实例（递归渲染容器）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：单个星图层的渲染容器，支持递归嵌套
//
// Issue #805 评论 5907045450 第 2 部分：
//   替代旧的"下钻换整页 graph"模型。StarMapScene 是"场景实例"，每个 Scene
//   自己持有 viewport 和手势状态（通过内部 StarMapCanvas 的 interaction）。
//   每个 Embed 内部用 Loader 创建下一层 StarMapScene。
//   同一目标 StarMap 被两个 Embed 引用时 pathKey 不同，选择/viewport/手势
//   状态分开。
//
// 属性：
//   - rootStarmapId：根星图 ID（整个递归树的根，不变）
//   - pathSegments：从根到本层的路径段数组（根层为 []）
//   - pathKey：本 Scene 实例的唯一 key（用于手势状态隔离）
//   - finalStarmapId：解析出的本层星图 ID（只读，由 resolve_starmap_path 得出）
//
// 初始化用 resolve_starmap_path(rootStarmapId, pathSegments) 解析出
// finalStarmapId，再用 StarMapCanvas 渲染。解析出来的 finalStarmapId 只
// 属于这块 Scene，不能写回 Workspace。
//
// 约束：
//   - 纯 UI 容器，不读写 Core 业务状态（通过 Canvas/Controller 委托）
//   - 每个 Scene 实例独立 viewport（panX/panY/zoomLevel 在内部 Canvas）
//   - 手势状态归属于 scene path（内部 Canvas 的 interaction）
// =============================================================================

import QtQuick
import QtQuick.Controls

Item {
    id: scene

    required property var dt
    property var starmapBackendRef: null

    // 根星图 ID（整个递归树的根）
    property string rootStarmapId: ""

    // 从根到本层的路径段数组（根层为 []）
    // 每项形如 { type: "enterEmbed", instanceId, nodeId: null } 或
    // { type: "enterPortal", instanceId: null, nodeId }
    property var pathSegments: []

    // 本 Scene 实例的唯一 key（用于手势状态隔离）
    // 根 Scene 的 pathKey 为 "root"，子 Scene 为 "root/embed_<instanceId>/..."
    property string pathKey: "root"

    // 解析出的本层星图 ID（由 resolve_starmap_path 得出）
    property string finalStarmapId: ""

    // 解析错误（非空时显示错误，不渲染 Canvas）
    property string resolveError: ""

    // 对外信号
    signal editNodeRequested(var node)
    signal nodeSelected(var node)
    signal selectionCleared()

    // ── 路径解析 ──
    // 用 rootStarmapId + pathSegments 解析出 finalStarmapId。
    // 解析失败时 resolveError 非空，finalStarmapId 为 ""。
    function resolvePath() {
        if (rootStarmapId === "") {
            resolveError = ""
            finalStarmapId = ""
            return
        }
        if (!starmapBackendRef) {
            // 后端未注入时不报错：创建顺序不保证，等后端到了再解析。
            resolveError = ""
            finalStarmapId = ""
            return
        }
        var res = starmapBackendRef.resolve_starmap_path(rootStarmapId, JSON.stringify(pathSegments))
        if (!res || res.success !== true) {
            resolveError = qsTr("解析星图层级路径失败")
                    + (res && res.errorCode ? " (" + res.errorCode + ")" : "")
            finalStarmapId = ""
            return
        }
        var finalId = res.data && res.data.finalStarmapId ? res.data.finalStarmapId : ""
        if (finalId === "") {
            resolveError = qsTr("解析星图层级路径失败")
            finalStarmapId = ""
            return
        }
        resolveError = ""
        finalStarmapId = finalId
    }

    onRootStarmapIdChanged: resolvePath()
    onPathSegmentsChanged: resolvePath()
    onStarmapBackendRefChanged: {
        if (rootStarmapId !== "" && finalStarmapId === "")
            resolvePath()
    }
    Component.onCompleted: resolvePath()

    // ── 错误态 ──
    AppText {
        anchors.centerIn: parent
        visible: scene.resolveError.length > 0
        text: scene.resolveError
        color: dt.error
        font.pointSize: dt.fontSmPt
        wrapMode: Text.Wrap
        horizontalAlignment: Text.AlignHCenter
    }

    // ── 等待解析 ──
    AppText {
        anchors.centerIn: parent
        visible: scene.rootStarmapId !== "" && scene.finalStarmapId === "" && scene.resolveError === ""
        text: qsTr("加载中…")
        color: dt.textMuted
        font.pointSize: dt.fontSmPt
    }

    // ── 星图画布 ──
    // 每个 Scene 自己持有 Canvas，Canvas 内部的 interaction 是本 Scene 专属的
    // 手势状态机（pan/connect/move）。不同 Scene 的手势状态互不干扰。
    StarMapCanvas {
        id: sceneCanvas
        anchors.fill: parent
        visible: scene.finalStarmapId.length > 0
        dt: scene.dt
        starmapId: scene.finalStarmapId
        starmapBackendRef: scene.starmapBackendRef

        // 递归渲染上下文：传给 Canvas，Canvas 再传给 Embed delegate，
        // Embed 的 contentViewport 用这些构造子 Scene 的 pathSegments。
        // Issue #805 评论 5907045450 第 2 部分：Embed 内部用 Loader 创建
        // 下一层 StarMapScene（childPath = parent.pathSegments + EnterEmbed(instanceId)）。
        // 这些属性在 Canvas 上声明，供 Embed delegate 读取。
        // （StarMapCanvas 需要新增 rootStarmapId / pathSegments 属性）

        onEditNodeRequested: function(node) { scene.editNodeRequested(node) }
        onNodeSelected: function(node) { scene.nodeSelected(node) }
        onSelectionCleared: { scene.selectionCleared() }
    }

    // 暴露 Canvas 的 resetInteraction 供外部调用
    function resetInteraction() {
        if (scene.finalStarmapId.length > 0)
            sceneCanvas.resetInteraction()
    }
}
