package com.xiwei.sujian.feature.starmap.data.model

/**
 * 搜索结果要定位的星图对象 — 只描述身份，不携带平台 UI 状态。
 *
 * 链接没有独立几何，必须通过其 source 路径解析到实际拥有的
 * 节点 / 连线 / 嵌入后再高亮。
 */
sealed interface StarMapSelectionTarget {
    val id: String

    data class Node(override val id: String) : StarMapSelectionTarget

    data class Edge(override val id: String) : StarMapSelectionTarget

    data class Embed(override val id: String) : StarMapSelectionTarget

    data class Link(override val id: String) : StarMapSelectionTarget
}

/** 星图里真正拥有几何位置的对象。 */
sealed interface StarMapSelectionOwner {
    val id: String
    val key: String

    data class Node(val node: StarMapGraphNode) : StarMapSelectionOwner {
        override val id: String get() = node.id
        override val key: String get() = "node:$id"
    }

    data class Edge(val edge: StarMapGraphEdge) : StarMapSelectionOwner {
        override val id: String get() = edge.id
        override val key: String get() = "edge:$id"
    }

    data class Embed(val embed: StarMapEmbedData) : StarMapSelectionOwner {
        override val id: String get() = embed.instanceId
        override val key: String get() = "embed:$id"
    }
}

/** 一次定位请求的解析结果；owner 为空表示目标已不存在。 */
data class StarMapResolvedSelection(
    val requested: StarMapSelectionTarget,
    val owner: StarMapSelectionOwner,
)

/**
 * 在已加载的星图快照里解析搜索结果目标。
 *
 * 节点/连线/嵌入直接按 ID 查找；链接先找到记录，再把 source 路径
 * 解析为当前星图中可见的节点 / 连线 / 嵌入。解析不到 owner 时返回 null，
 * 调用方据此判定目标已不存在，而不是把“打开了星图”算作成功。
 */
fun StarMapData.resolveSelectionTarget(target: StarMapSelectionTarget): StarMapResolvedSelection? {
    val owner =
        when (target) {
            is StarMapSelectionTarget.Node ->
                graph.nodes.firstOrNull { it.id == target.id }?.let(StarMapSelectionOwner::Node)
            is StarMapSelectionTarget.Edge ->
                graph.edges.firstOrNull { it.id == target.id }?.let(StarMapSelectionOwner::Edge)
            is StarMapSelectionTarget.Embed ->
                embeds.firstOrNull { it.instanceId == target.id }?.let(StarMapSelectionOwner::Embed)
            is StarMapSelectionTarget.Link ->
                links.firstOrNull { it.linkId == target.id }?.let { resolvePathOwner(it.source) }
        } ?: return null
    // 目标存在但端点无法解析时没有可定位几何，不能当作可成功定位。
    if (ownerCenter(owner) == null) return null
    return StarMapResolvedSelection(requested = target, owner = owner)
}

/** 目标对象在星图坐标系里的中心点。 */
fun StarMapData.selectionCenter(target: StarMapSelectionTarget): StarMapPointData? {
    val resolved = resolveSelectionTarget(target) ?: return null
    return ownerCenter(resolved.owner)
}

fun StarMapData.ownerCenter(owner: StarMapSelectionOwner): StarMapPointData? =
    when (owner) {
        is StarMapSelectionOwner.Node -> owner.node.position
        is StarMapSelectionOwner.Embed -> owner.embed.position
        is StarMapSelectionOwner.Edge -> {
            val from = positionForPath(owner.edge.from)
            val to = positionForPath(owner.edge.to)
            if (from == null || to == null) {
                null
            } else {
                StarMapPointData(
                    x = (from.x + to.x) / 2f,
                    y = (from.y + to.y) / 2f,
                )
            }
        }
    }

/** 连线端点路径在当前星图中的位置；嵌套路径落到最外层嵌入的坐标。 */
fun StarMapData.positionForPath(path: StarMapTargetPathData): StarMapPointData? =
    resolvePathOwner(path)?.let(::ownerCenter)

/**
 * 把一条 target path 解析为当前星图中可见的 owner。
 *
 * - 带 segments：目标位于嵌套星图，外层可见对象是 segments 里的嵌入实例；
 * - 直接 node/anchor：owner 是该节点；
 * - entity：按 entityType 尝试节点 / 连线 / 嵌入；
 * - 其余（章节范围、纯星图目标）没有当前画布几何。
 */
private fun StarMapData.resolvePathOwner(path: StarMapTargetPathData): StarMapSelectionOwner? {
    path.segments.firstOrNull()?.instanceId?.let { instanceId ->
        embeds.firstOrNull { it.instanceId == instanceId }?.let { return StarMapSelectionOwner.Embed(it) }
    }

    path.target.nodeId?.let { nodeId ->
        graph.nodes.firstOrNull { it.id == nodeId }?.let { return StarMapSelectionOwner.Node(it) }
    }

    val entityId = path.target.entityId ?: return null
    graph.nodes.firstOrNull { it.id == entityId }?.let { return StarMapSelectionOwner.Node(it) }
    graph.edges.firstOrNull { it.id == entityId }?.let { return StarMapSelectionOwner.Edge(it) }
    embeds.firstOrNull { it.instanceId == entityId }?.let { return StarMapSelectionOwner.Embed(it) }
    return null
}
