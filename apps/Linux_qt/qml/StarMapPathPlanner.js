// =============================================================================
// StarMapPathPlanner.js — 跨层连线的宿主规划（递归星图的纯路径算法）
// =============================================================================
//
// Issue #822 评论 5972557963：边必须存进"两个端点所在 Scene 的最近公共祖先"，
// 不能永远存进起点那一层。Linux 侧的 from/to 是相对根星图的绝对路径
// （starmapId = rootStarmapId，segments 从根往下），规则与 Harmony
// StarMapGeometry.planCrossLayerRelation / buildTargetPathForHost 完全一致：
//
// - 父 Node → 子 Node：宿主 = 父 Scene，to = [enterEmbed(...)] + node
// - 子 Node → 父 Node：宿主 = 父 Scene，from = [enterEmbed(...)] + node
// - 同一子图里的两个 Node：宿主 = 这个子图，两端 segments 都为空
// - 两个兄弟子图里的 Node：宿主 = 共同父图，两端各带一段 enterEmbed
// - 更深层同理取最近公共祖先
//
// 纯路径计算：不读后端、不读 QML，宿主 starmapId 由调用方用宿主的
// finalStarmapId 填进 plan.from / plan.to（Core 要求端点从宿主图出发）。
// =============================================================================

.pragma library

// 深拷贝路径段（DTO 字段一一保留）。
function cloneSegment(seg) {
    return { type: seg.type, instanceId: seg.instanceId, nodeId: seg.nodeId }
}

function cloneSegments(segments) {
    var out = []
    for (var i = 0; i < segments.length; i++)
        out.push(cloneSegment(segments[i]))
    return out
}

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

// 两条 Scene 路径的最长公共前缀。
function commonScenePathPrefix(a, b) {
    var out = []
    var max = Math.min(a.length, b.length)
    for (var i = 0; i < max; i++) {
        if (!isSameSegment(a[i], b[i]))
            break
        out.push(cloneSegment(a[i]))
    }
    return out
}

// 把"相对根星图的绝对路径"解析成它真正属于哪个 Scene（scenePath）里的对象。
// 解析不出（段形状非法 / 末段不是容器段）时返回 null。
function resolveItemRef(path) {
    if (!path || !path.target)
        return null
    if (path.target.type === "node") {
        if (!path.target.nodeId)
            return null
        return {
            scenePath: cloneSegments(path.segments || []),
            kind: "node",
            itemId: path.target.nodeId
        }
    }
    // Embed-like 端点：最后一段就是被引用的那个容器自身
    // （正式 Embed = enterEmbed{instanceId}，旧 Portal = enterPortal{nodeId}），
    // 它属于上一段到达的 Scene。terminalSegment 原样保留，
    // 重建相对宿主的路径时不再无条件改写成 enterEmbed。
    var segments = path.segments || []
    var last = segments.length > 0 ? segments[segments.length - 1] : null
    if (!last || (last.type !== "enterEmbed" && last.type !== "enterPortal"))
        return null
    if (last.type === "enterEmbed" && !last.instanceId)
        return null
    if (last.type === "enterPortal" && !last.nodeId)
        return null
    return {
        scenePath: cloneSegments(segments.slice(0, segments.length - 1)),
        kind: "embed",
        itemId: uiInstanceIdOfSegment(last),
        terminalSegment: cloneSegment(last)
    }
}

// itemRef 相对宿主 Scene 的路径：segments 只保留"从宿主往下"的部分，
// 宿主本身不再出现在路径里。
function buildTargetPathForHost(hostSegments, itemRef) {
    var segments = cloneSegments(itemRef.scenePath.slice(hostSegments.length))
    if (itemRef.kind === "embed") {
        // Embed-like 端点直接沿用原来的 terminal segment：
        // 正式 Embed 是 enterEmbed，旧 Portal 是 enterPortal{nodeId}。
        // 渲染递归路径与 LCA 建边路径因此始终是同一份路径真相。
        segments.push(cloneSegment(itemRef.terminalSegment))
        return { starmapId: "", segments: segments, target: { type: "starmap" } }
    }
    return {
        starmapId: "",
        segments: segments,
        target: { type: "node", nodeId: itemRef.itemId }
    }
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

/**
 * 规划一条跨层关系（Edge 或 Link）应该落在哪张图上。
 *
 * Edge（语义边）与 Link（内部跳转）共用这套 LCA 宿主规划，不复制第二套算法：
 * 两者都要求关系存进"两端所在 Scene 的最近公共祖先"，只是落库时调各自的
 * create_starmap_edge_with_paths / add_starmap_link。
 *
 * @param fromPath 起点（相对根星图的绝对路径）
 * @param toPath   终点（相对根星图的绝对路径）
 * @returns { hostSegments, from, to }；自环或解析失败时返回 null。
 *          from/to 的 starmapId 是空串，调用方找到宿主 Content 后填
 *          host.finalStarmapId 再写 Core。
 */
function planCrossLayerRelation(fromPath, toPath) {
    var from = resolveItemRef(fromPath)
    var to = resolveItemRef(toPath)
    if (!from || !to)
        return null
    if (from.kind === to.kind && from.itemId === to.itemId
            && isSameScenePath(from.scenePath, to.scenePath))
        return null
    var hostSegments = commonScenePathPrefix(from.scenePath, to.scenePath)
    var planFrom = buildTargetPathForHost(hostSegments, from)
    var planTo = buildTargetPathForHost(hostSegments, to)
    if (isSameTargetPath(planFrom, planTo))
        return null
    return { hostSegments: hostSegments, from: planFrom, to: planTo }
}
