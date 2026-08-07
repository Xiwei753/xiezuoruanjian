package com.xiwei.sujian.model

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
 * - StarMapData：星图完整数据（节点 + 连线 + 布局）
 */

data class StarMapMeta(
    val starmapId: String,
    val title: String,
    val description: String,
    val projectId: String?,
    val parentStarmapId: String?,
    val isMainForProject: Boolean,
    val accentColor: String,
    val createdAt: Long,
    val updatedAt: Long,
    val nodeCount: Int,
    val edgeCount: Int,
    val linkedChapterCount: Int,
    val childStarmapCount: Int,
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
    val openBehavior: String? = null,
    val displayPolicy: StarMapDisplayPolicyData? = null,
    val provenance: StarMapProvenanceData? = null,
    val createdAt: Long = 0,
    val updatedAt: Long = 0,
)

data class StarMapPortalData(
    val targetStarmapId: String = "",
    val deepTarget: StarMapDeepTargetData? = null,
    val mode: String = "Navigate",
    val previewPolicy: String = "Inline",
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

data class StarMapDisplayPolicyData(
    val importance: Float = 1f,
    val minVisibleScale: Float = 0f,
    val titleScale: Float = 1f,
    val summaryScale: Float = 1f,
    val detailScale: Float = 1f,
    val maxPreviewChars: Int = 120,
    val minReadablePx: Float = 12f,
)

data class StarMapProvenanceData(
    val source: String = "Human",
    val sourceId: String? = null,
    val generatedBy: String? = null,
    val promptId: String? = null,
    val reviewStatus: String = "Accepted",
    val createdFromAnchor: String? = null,
)

data class StarMapEdgeEndpointData(
    val kind: String,
    val nodeId: String? = null,
    val anchorId: String? = null,
    val deepTarget: StarMapDeepTargetData? = null,
)

data class StarMapEndpointPathSegmentData(
    val kind: String,
    val starmapId: String? = null,
)

data class StarMapEndpointPathData(
    val segments: List<StarMapEndpointPathSegmentData> = emptyList(),
    val endpoint: StarMapEdgeEndpointData,
)

data class StarMapDeepTargetData(
    val starmapId: String,
    val path: List<StarMapPathSegmentData> = emptyList(),
    val target: StarMapTargetDetailData,
)

data class StarMapPathSegmentData(
    val kind: String,
    val starmapId: String? = null,
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

data class StarMapEndpointData(
    val kind: String,
    val nodeId: String? = null,
    val anchorId: String? = null,
)

data class StarMapGraphEdge(
    val id: String,
    val from: String,
    val to: String,
    val kind: StarMapEdgeKind,
    val label: String? = null,
    val payload: Map<String, Any>? = null,
    val fromTarget: StarMapDeepTargetData? = null,
    val toTarget: StarMapDeepTargetData? = null,
    val fromEndpoint: StarMapEdgeEndpointData? = null,
    val toEndpoint: StarMapEdgeEndpointData? = null,
    val fromEndpointPath: StarMapEndpointPathData? = null,
    val toEndpointPath: StarMapEndpointPathData? = null,
    val createdAt: Long = 0,
    val updatedAt: Long = 0,
)

data class StarMapGraphData(
    val schemaVersion: Int,
    val id: String,
    val starmapId: String,
    val title: String,
    val nodes: List<StarMapGraphNode>,
    val edges: List<StarMapGraphEdge>,
    val createdAt: Long,
    val updatedAt: Long,
)

enum class StarMapLayoutKind {
    Freeform,
    AutoRadial,
    Custom,
}

data class StarMapLayoutNodeData(
    val nodeId: String,
    val x: Float,
    val y: Float,
    val width: Float,
    val height: Float,
    val radius: Float,
    val collapsed: Boolean,
    val zIndex: Int,
    val scale: Float = 1f,
    val depth: Float = 0f,
    val focusWeight: Float = 1f,
    val orbitGroup: String? = null,
)

data class StarMapLayoutData(
    val kind: StarMapLayoutKind,
    val nodes: List<StarMapLayoutNodeData>,
)

data class StarMapEdgeRenderData(
    val edgeId: String,
    val fromCx: Float,
    val fromCy: Float,
    val toCx: Float,
    val toCy: Float,
    val startX: Float,
    val startY: Float,
    val endX: Float,
    val endY: Float,
    val offsetX: Float,
    val offsetY: Float,
    val arrowTipX: Float,
    val arrowTipY: Float,
    val arrowLeftX: Float,
    val arrowLeftY: Float,
    val arrowRightX: Float,
    val arrowRightY: Float,
    val labelX: Float,
    val labelY: Float,
    val label: String? = null,
    val hasBidirectional: Boolean,
)

data class StarMapViewportData(
    val scale: Float = 1f,
    val offsetX: Float = 0f,
    val offsetY: Float = 0f,
    val width: Float = 0f,
    val height: Float = 0f,
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
    val layout: StarMapLayoutData,
    val edgeRenders: List<StarMapEdgeRenderData> = emptyList(),
    val viewport: StarMapViewportData = StarMapViewportData(),
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
    val sourceNodeId: String? = null,
    val hostEndpoint: StarMapEndpointData? = null,
    val displayPolicy: StarMapDisplayPolicyData? = null,
    val openBehavior: String = "Inspector",
    val provenance: StarMapProvenanceData? = null,
    val placement: StarMapEmbedPlacementData = StarMapEmbedPlacementData(),
    val targetViewport: StarMapEmbedViewportData = StarMapEmbedViewportData(),
)

data class StarMapEmbedPlacementData(
    val x: Float = 0f,
    val y: Float = 0f,
    val width: Float = 200f,
    val height: Float = 150f,
    val scale: Float = 1f,
    val zIndex: Int = 0,
    val collapsed: Boolean = false,
)

data class StarMapEmbedViewportData(
    val scale: Float = 1f,
    val offsetX: Float = 0f,
    val offsetY: Float = 0f,
)

data class StarMapLinkData(
    val linkId: String,
    val source: StarMapEndpointData = StarMapEndpointData(kind = "Node"),
    val target: StarMapDeepTargetData? = null,
    val label: String? = null,
)

data class StarMapHyperlinkData(
    val hyperlinkId: String,
    val source: StarMapEndpointPathData? = null,
    val targetUri: String,
    val label: String? = null,
    val targetStarmapId: String? = null,
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

/**
 * 星图动画策略参数，与 Rust Core 的 StarMapMotionPolicyDto 一一对应。
 *
 * ## 架构定位
 * - 跨端共享的动画策略，由 Core 层下发
 * - Android 端据此控制 idle wobble、drag lift、settle 等动画行为
 */
data class StarMapMotionPolicyData(
    val enabled: Boolean = true,
    val idleWobbleEnabled: Boolean = true,
    val idleAmplitudeVp: Float = 2.0f,
    val idlePeriodMs: Int = 4200,
    val dragLiftScale: Float = 1.04f,
    val dragShadowBoost: Float = 8.0f,
    val settleDurationMs: Int = 220,
    val reduceMotion: Boolean = false,
)
