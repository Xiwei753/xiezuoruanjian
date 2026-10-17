// =============================================================================
// StarMapPathPlanner.js — 星图路径的 UI 侧工具
// =============================================================================
//
// 宿主规划（一条 Edge / Link 到底落在哪张图上）已经收回 Core：
//   - Core 规则：core/writer_core/src/starmap/graph/path_planner.rs
//   - 桥接：apps/Linux_qt/src/starmap_bridge.rs::plan_cross_layer_connection
//   - 调用点：StarMapSceneContent.planCrossLayerConnection(fromPath, toPath)
// QML 不再持有最近公共祖先、端点相对宿主路径等业务规则。
//
// 本文件只保留渲染/交互需要的纯 UI 路径工具：
//   - uiInstanceIdOfSegment：路径段 → UI 侧 Embed instanceId（递归下钻找宿主
//     Content 用；旧 portal 归一到 "legacy-portal:<nodeId>"）。
//   - isSameTargetPath / isSameScenePath / isSameSegment：端点语义相等比较
//     （供 listLinksForSource 沿祖先链找 Link）。
// =============================================================================

.pragma library

function isSameSegment(a, b) {
    return a.type === b.type && a.instanceId === b.instanceId && a.nodeId === b.nodeId
}

function isSameScenePath(a, b) {
    if (a.length !== b.length)
        return false
    for (var i = 0; i < a.length; i++) {
        if (!isSameSegment(a[i], b[i]))
            return false
    }
    return true
}

function isSameTargetPath(a, b) {
    if (!isSameScenePath(a.segments, b.segments))
        return false
    if (a.target.type !== b.target.type)
        return false
    return (a.target.nodeId || null) === (b.target.nodeId || null)
}

/**
 * 路径段 → UI 侧的 Embed instanceId（递归下钻找宿主 Content 用）。
 *
 * 旧 portal 在 UI 里按同一约定归一到 "legacy-portal:<nodeId>"：
 * 段语义只在这里实现一次，Content 不自行判断 portal 身份。
 *
 * @param seg StarMapPathSegment（enterEmbed / enterPortal）
 * @returns UI instanceId；非容器段返回空串
 */
function uiInstanceIdOfSegment(seg) {
    if (!seg)
        return ""
    if (seg.type === "enterEmbed")
        return seg.instanceId || ""
    if (seg.type === "enterPortal" && seg.nodeId)
        return "legacy-portal:" + seg.nodeId
    return ""
}
