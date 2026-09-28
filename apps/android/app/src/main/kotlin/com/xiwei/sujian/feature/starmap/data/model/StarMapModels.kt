package com.xiwei.sujian.feature.starmap.data.model

/**
 * StarMapModels — 星图数据模型
 *
 * 定义星图相关的数据类和枚举，与 Rust Core 的星图数据结构一一对应。
 *
 * ## 架构定位
 * - 这些模型是 Rust Core UniFFI DTO 的 Kotlin 映射
 * - 字段名使用 Kotlin camelCase 命名，与 Core serde rename_all = "camelCase" 契约一致
 * - 所有数据通过 UniFFI typed bridge 传输，不经过 Gson JSON 反序列化
 *
 * ## 包含模型
 * - StarMapMeta：星图元数据
 * - StarMapNodeKind：节点类型枚举（角色、地点、事件等）
 * - StarMapEdgeKind：连线类型枚举
 * - StarMapPointData：星图文档坐标点（节点 position 与嵌入落点的唯一真相）
 * - StarMapData：星图完整数据（图 + 嵌入 + 链接）
 *
 * Core 收口后布局、视口、边几何、命中与动画策略已全部退出 Core，
 * 坐标真相统一收敛在节点的 position 上，本文件不再保留显示层模型。
 */

data class StarMapMeta(
    val starmapId: String,
    val title: String,
    val description: String,
    val projectId: String?,
    val accentColor: String,
    val createdAt: Long,
    val updatedAt: Long,
)

enum class StarMapNodeKind {
    Character,
    Event,
    Location,
    Item,
    Concept,
    Theme,
    Note,
    Organization,
    Timeline,
    Plot,
    Foreshadowing,
    Chapter,
    Custom,
}

enum class StarMapEdgeKind {
    Contains,
    References,
    AppearsIn,
    Causes,
    RelatedTo,
    LocatedAt,
    CharacterRelation,
    Timeline,
    Foreshadows,
    Resolves,
    DependsOn,
    ConflictsWith,
    Custom,
}

data class StarMapGraphNode(
    val id: String,
    val title: String,
    val kind: StarMapNodeKind,
    val payload: Map<String, Any>? = null,
    val tags: List<String> = emptyList(),
    val contentKind: String? = null,
    val contentBody: String? = null,
    val contentSummary: String? = null,
    val contentProjectId: String? = null,
    val contentVolumeId: String? = null,
    val contentRangeStart: Int? = null,
    val contentRangeEnd: Int? = null,
    val contentEntityType: String? = null,
    val contentEntityId: String? = null,
    val contentLabel: String? = null,
    val contentChapterId: String? = null,
    val contentUri: String? = null,
    val anchors: List<StarMapAnchorData> = emptyList(),
    val portal: StarMapPortalData? = null,
    val position: StarMapPointData = StarMapPointData(),
    val style: StarMapNodeStyleData = StarMapNodeStyleData(),
    val provenance: StarMapProvenanceData? = null,
    val createdAt: Long = 0,
    val updatedAt: Long = 0,
)

data class StarMapPortalData(
    val destinationStarmapId: String = "",
    val destinationTarget: StarMapTargetDetailData? = null,
)

data class StarMapAnchorData(
    val anchorId: String,
    val targetKind: String = "Chapter",
    val targetProjectId: String? = null,
    val targetVolumeId: String? = null,
    val targetRangeStart: Int? = null,
    val targetRangeEnd: Int? = null,
    val targetEntityType: String? = null,
    val targetPayload: String? = null,
    val targetChapterId: String? = null,
    val targetEntityId: String? = null,
    val targetStarmapId: String? = null,
    val targetUri: String? = null,
    val label: String? = null,
    val role: String = "Source",
)

data class StarMapProvenanceData(
    val source: String = "Human",
    val sourceId: String? = null,
    val generatedBy: String? = null,
    val promptId: String? = null,
    val reviewStatus: String = "Accepted",
    val createdFromAnchor: String? = null,
)

data class StarMapPathSegmentData(
    val kind: String,
    val instanceId: String? = null,
    val nodeId: String? = null,
)

data class StarMapTargetPathData(
    val starmapId: String,
    val segments: List<StarMapPathSegmentData> = emptyList(),
    val target: StarMapTargetDetailData,
)

data class StarMapTargetDetailData(
    val kind: String,
    val nodeId: String? = null,
    val anchorId: String? = null,
    val projectId: String? = null,
    val volumeId: String? = null,
    val chapterId: String? = null,
    val rangeStart: UInt? = null,
    val rangeEnd: UInt? = null,
    val entityType: String? = null,
    val entityId: String? = null,
    val uri: String? = null,
)

data class StarMapGraphEdge(
    val id: String,
    val from: StarMapTargetPathData,
    val to: StarMapTargetPathData,
    val kind: StarMapEdgeKind,
    val label: String? = null,
    val payload: Map<String, Any>? = null,
    val createdAt: Long = 0,
    val updatedAt: Long = 0,
)

data class StarMapGraphData(
    val schemaVersion: Int,
    val starmapId: String,
    val nodes: List<StarMapGraphNode>,
    val edges: List<StarMapGraphEdge>,
)

/**
 * 星图文档坐标点。
 *
 * Core 收口后 position 是节点在星图坐标系里的唯一位置真相（原先由已退出的
 * 显示层布局节点承载），嵌入星图的落点也复用同一结构。
 */
data class StarMapPointData(
    val x: Float = 0f,
    val y: Float = 0f,
)

/** 节点显示样式，目前 Core 只保留一个自定义填充色。 */
data class StarMapNodeStyleData(
    val fillColor: String? = null,
)

data class StarMapNodePatch(
    val title: String? = null,
    val kind: StarMapNodeKind? = null,
    val payload: Map<String, Any>? = null,
    val tags: List<String>? = null,
)

data class StarMapEdgePatch(
    val kind: StarMapEdgeKind? = null,
    val label: String? = null,
    val payload: Map<String, Any>? = null,
)

data class StarMapData(
    val graph: StarMapGraphData,
    val embeds: List<StarMapEmbedData> = emptyList(),
    val links: List<StarMapLinkData> = emptyList(),
    val hyperlinks: List<StarMapHyperlinkData> = emptyList(),
    val loadPhase: String = "CurrentViewportObjects",
    val packageRevision: ULong = 0u,
    val sinceRevision: ULong = 0u,
    val complete: Boolean = false,
)

data class StarMapEmbedData(
    val instanceId: String,
    val targetStarmapId: String,
    val label: String? = null,
    val hostPath: StarMapTargetPathData? = null,
    val provenance: StarMapProvenanceData? = null,
    val position: StarMapPointData = StarMapPointData(),
)

data class StarMapLinkData(
    val linkId: String,
    val source: StarMapTargetPathData,
    val target: StarMapTargetPathData,
    val label: String? = null,
)

data class StarMapHyperlinkData(
    val hyperlinkId: String,
    val source: StarMapTargetPathData? = null,
    val targetUri: String,
    val label: String? = null,
)

data class StarMapPhasedSnapshotResult(
    val data: StarMapData,
    val diagnostics: List<StarMapLoadDiagnostic> = emptyList(),
)

data class StarMapLoadDiagnostic(
    val kind: String,
    val objectType: String,
    val objectId: String,
    val detail: String? = null,
)
