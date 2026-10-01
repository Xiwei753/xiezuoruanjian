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
    // Issue #805 评论 5908703621 问题 5：editNodeRequested 带 owner 上下文，
    // 让 Inspector 知道节点属于哪一层星图，更新/删除回到 owner Scene。
    // Issue #805 评论 5912394108：ownerScene(var) 携带真正拥有该节点的 Scene
    // 引用，Workspace 用它直接调 updateNodeFromInspector/deleteNodeFromInspector，
    // 不再写死 rootScene，确保第二层及更深节点的回写打到对应子 Scene 的 Controller。
    signal editNodeRequested(var ownerScene, string ownerStarmapId, string ownerPathKey, var node)
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
            // Issue #814 评论 5935346839: scene_resolve_failed 边界日志。
            if (starmapBackendRef) {
                starmapBackendRef.record_interaction(
                    "scene_resolve_failed", pathKey, rootStarmapId, "scene", "",
                    JSON.stringify({
                        "pathKey": pathKey,
                        "errorCode": (res && res.errorCode) ? res.errorCode : "",
                        "rootStarmapId": rootStarmapId
                    }))
            }
            return
        }
        var finalId = res.data && res.data.finalStarmapId ? res.data.finalStarmapId : ""
        if (finalId === "") {
            resolveError = qsTr("解析星图层级路径失败")
            finalStarmapId = ""
            // Issue #814 评论 5935346839: scene_resolve_failed 边界日志（finalId 为空）。
            if (starmapBackendRef) {
                starmapBackendRef.record_interaction(
                    "scene_resolve_failed", pathKey, rootStarmapId, "scene", "",
                    JSON.stringify({
                        "pathKey": pathKey,
                        "errorCode": "empty_final_id",
                        "rootStarmapId": rootStarmapId
                    }))
            }
            return
        }
        resolveError = ""
        finalStarmapId = finalId
        // Issue #814 评论 5935346839: scene_resolved 边界日志。
        if (starmapBackendRef) {
            starmapBackendRef.record_interaction(
                "scene_resolved", pathKey, rootStarmapId, "scene", finalId,
                JSON.stringify({
                    "pathKey": pathKey,
                    "rootStarmapId": rootStarmapId,
                    "finalStarmapId": finalId,
                    "depth": pathSegments.length
                }))
        }
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
        dt: scene.dt
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
        dt: scene.dt
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

        // Issue #805 评论 5908703621 问题 1：递归渲染上下文必须传给 Canvas，
        // Canvas 再传给 Embed delegate，否则 childSceneLoader 的 active 条件
        // （rootStarmapId.length > 0）不满足，子场景不会加载。
        rootStarmapId: scene.rootStarmapId
        pathSegments: scene.pathSegments
        pathKey: scene.pathKey

        // Issue #805 评论 5908703621 问题 5：本层 Canvas 上抛 editNodeRequested(var node)，
        // Scene 用 finalStarmapId/pathKey 包装成带 owner 上下文的三参数信号上抛。
        // Issue #805 评论 5912394108：本层节点属于这块 Scene，ownerScene 传 scene 自身。
        onEditNodeRequested: function(node) {
            scene.editNodeRequested(scene, scene.finalStarmapId, scene.pathKey, node)
        }
        // Issue #805 评论 5908703621 问题 5：child Scene 经 Embed 冒泡上来的
        // editNodeRequested 已经带正确的 owner 上下文，原样转发不再重新包装。
        // Issue #805 评论 5912394108：ownerScene 也原样转发，保持指向真正拥有
        // 该节点的子 Scene，不被本层 Scene 替换。
        onChildEditNodeRequested: function(ownerScene, ownerStarmapId, ownerPathKey, node) {
            scene.editNodeRequested(ownerScene, ownerStarmapId, ownerPathKey, node)
        }
        onNodeSelected: function(node) { scene.nodeSelected(node) }
        onSelectionCleared: { scene.selectionCleared() }
    }

    // 暴露 Canvas 的 resetInteraction 供外部调用
    function resetInteraction() {
        if (scene.finalStarmapId.length > 0)
            sceneCanvas.resetInteraction()
    }

    // Issue #805 评论 5908703621 问题 5：Inspector 更新/删除必须回到 owner Scene。
    // Scene 暴露 updateNodeFromInspector/deleteNodeFromInspector 转发到内部 Canvas，
    // Workspace 按 ownerPathKey 找到对应 Scene 实例调用。
    function updateNodeFromInspector(nodeId, patch) {
        if (scene.finalStarmapId.length > 0)
            sceneCanvas.updateNodeFromInspector(nodeId, patch)
    }

    function deleteNodeFromInspector(nodeId) {
        if (scene.finalStarmapId.length > 0)
            sceneCanvas.deleteNodeFromInspector(nodeId)
    }
}
