// ─── StarMap DTOs ───
// Issue #753：字段与 Core api/types/starmap/*.rs 的 DTO 一字不差对齐。
// Core DTO 是唯一数据结构，Harmony FFI 只负责搬运。
//
// 线格式约定（已逐个核对 Core 源码）：
// - 所有 struct 都是 `#[serde(rename_all = "camelCase")]`。
// - starmap DTO 里没有任何 `skip_serializing_if`，因此 Rust `Option<T>` 字段
//   一定以 `"k":null` 出现 → ArkTS 写成 `T | null`，不写成可选属性。
// - 所有 enum 都没有 `rename_all` → 线格式是 Rust 变体名原样（PascalCase）。
// - 判别式对象（content / anchorTarget / endpoint / targetDetail / pathSegment）
//   是普通 struct，字段 `#[serde(rename = "type")]`，值与 Core 里手写的字符串一致。

// ─── 节点 ───

// StarMapNodeKindDto
export type NodeType = 'Character' | 'Event' | 'Location' | 'Item' | 'Concept' | 'Theme' | 'Note'
  | 'Organization' | 'Timeline' | 'Plot' | 'Foreshadowing' | 'Chapter' | 'Custom'

// StarMapOpenBehaviorDto
export type OpenBehavior = 'Inspector' | 'ExpandCard' | 'WritingMode' | 'JumpToAnchor' | 'EnterPortal' | 'Custom'

// StarMapAnchorRoleDto
export type AnchorRole = 'Source' | 'Destination' | 'Reference' | 'Custom'

// StarMapPortalModeDto
export type PortalMode = 'EnterChild' | 'PreviewInline' | 'ReferenceOnly'

// StarMapPortalPreviewPolicyDto
export type PortalPreviewPolicy = 'Auto' | 'Always' | 'Never'

// StarMapSourceKindDto
export type SourceKind = 'Human' | 'Import' | 'Plugin' | 'Ai' | 'System' | 'Unknown'

// StarMapReviewStatusDto
export type ReviewStatus = 'Accepted' | 'Draft' | 'NeedsReview' | 'Rejected' | 'Unknown'

// StarMapNodeContentDto
export interface StarMapNodeContent {
  type: string  // "empty" | "inline" | "chapterRef" | "entityRef" | "externalRef"
  summary: string | null
  body: string | null
  projectId: string | null
  volumeId: string | null
  chapterId: string | null
  rangeStart: number | null
  rangeEnd: number | null
  entityType: string | null
  entityId: string | null
  uri: string | null
  label: string | null
}

// StarMapAnchorTargetDto
export interface StarMapAnchorTarget {
  type: string  // "chapterRange" | "project" | "volume" | "chapter" | "character" | "item"
                // | "location" | "event" | "starmap" | "external" | "custom"
  projectId: string | null
  volumeId: string | null
  chapterId: string | null
  rangeStart: number | null
  rangeEnd: number | null
  entityId: string | null
  entityType: string | null
  starmapId: string | null
  uri: string | null
  payload: string | null  // JSON 字符串，非对象
}

// StarMapAnchorDto
export interface StarMapAnchor {
  anchorId: string
  target: StarMapAnchorTarget
  label: string | null
  role: AnchorRole
}

// StarMapPathSegmentDto
export interface StarMapPathSegment {
  type: string  // 目前只有 "enterChild"
  starmapId: string | null
}

// StarMapTargetDetailDto
export interface StarMapTargetDetail {
  type: string  // "starmap" | "node" | "anchor" | "chapterRange" | "entity" | "external"
  nodeId: string | null
  anchorId: string | null
  projectId: string | null
  volumeId: string | null
  chapterId: string | null
  rangeStart: number | null
  rangeEnd: number | null
  entityType: string | null
  entityId: string | null
  uri: string | null
}

// StarMapDeepTargetDto
export interface StarMapDeepTarget {
  starmapId: string
  path: StarMapPathSegment[]
  target: StarMapTargetDetail
}

// StarMapPortalDto
export interface StarMapPortal {
  targetStarmapId: string
  deepTarget: StarMapDeepTarget | null
  mode: PortalMode
  previewPolicy: PortalPreviewPolicy
}

// StarMapDisplayPolicyDto
export interface StarMapDisplayPolicy {
  importance: number
  minVisibleScale: number
  titleScale: number
  summaryScale: number
  detailScale: number
  maxPreviewChars: number
  minReadablePx: number
}

// StarMapProvenanceDto
export interface StarMapProvenance {
  source: SourceKind
  sourceId: string | null
  generatedBy: string | null
  promptId: string | null
  reviewStatus: ReviewStatus
  createdFromAnchor: string | null
}

// StarMapNodeDto
export interface StarMapNode {
  id: string
  title: string
  kind: NodeType
  payload: string | null  // JSON 字符串，非对象
  tags: string[]
  content: StarMapNodeContent
  anchors: StarMapAnchor[]
  portal: StarMapPortal | null
  displayPolicy: StarMapDisplayPolicy
  openBehavior: OpenBehavior
  provenance: StarMapProvenance
  createdAt: number
  updatedAt: number
}

// ─── 边 ───

// StarMapEdgeKindDto
export type EdgeKind = 'Contains' | 'References' | 'AppearsIn' | 'Causes' | 'RelatedTo'
  | 'LocatedAt' | 'CharacterRelation' | 'Timeline' | 'Foreshadows' | 'Resolves'
  | 'DependsOn' | 'ConflictsWith' | 'Custom'

// StarMapEdgeEndpointDto
export interface StarMapEdgeEndpoint {
  type: string  // "node" | "anchor" | "starmap" | "deepTarget"
  nodeId: string | null
  anchorId: string | null
  target: StarMapDeepTarget | null
}

// StarMapEndpointPathSegmentDto
export interface StarMapEndpointPathSegment {
  type: string  // 目前只有 "enterChildMap"
  starmapId: string | null
}

// StarMapEndpointPathDto
export interface StarMapEndpointPath {
  segments: StarMapEndpointPathSegment[]
  endpoint: StarMapEdgeEndpoint
}

// StarMapEdgeDto
export interface StarMapEdge {
  id: string
  from: string | null
  to: string | null
  kind: EdgeKind
  label: string | null
  payload: string | null  // JSON 字符串，非对象
  fromTarget: StarMapDeepTarget | null
  toTarget: StarMapDeepTarget | null
  fromEndpoint: StarMapEdgeEndpoint | null
  toEndpoint: StarMapEdgeEndpoint | null
  fromEndpointPath: StarMapEndpointPath | null
  toEndpointPath: StarMapEndpointPath | null
  createdAt: number
  updatedAt: number
}

// ─── 内嵌 / 链接 / 超链接 ───

// StarMapEmbedPlacementDto
export interface StarMapEmbedPlacement {
  x: number
  y: number
  width: number
  height: number
  scale: number
  zIndex: number
  collapsed: boolean
}

// StarMapEmbedViewportDto
export interface StarMapEmbedViewport {
  scale: number
  offsetX: number
  offsetY: number
}

// StarMapEndpointDto（link 端点，与 edge endpoint 不同）
export interface StarMapEndpoint {
  type: string  // "node" | "anchor" | "starmap"
  nodeId: string | null
  anchorId: string | null
}

// StarMapEmbedDto
export interface StarMapEmbed {
  instanceId: string
  targetStarmapId: string
  label: string | null
  displayPolicy: StarMapDisplayPolicy
  openBehavior: OpenBehavior
  placement: StarMapEmbedPlacement
  targetViewport: StarMapEmbedViewport
  sourceNodeId: string | null
  hostEndpoint: StarMapEndpoint | null
  provenance: StarMapProvenance
  createdAt: number
  updatedAt: number
}

// StarMapLinkDto
export interface StarMapLink {
  linkId: string
  source: StarMapEndpoint
  target: StarMapDeepTarget
  label: string | null
  createdAt: number
  updatedAt: number
}

// StarMapHyperlinkDto
export interface StarMapHyperlink {
  hyperlinkId: string
  source: StarMapEndpointPath
  targetUri: string
  label: string | null
  createdAt: number
  updatedAt: number
}

// ─── 图 ───

// StarMapGraphDto
export interface StarMapGraph {
  schemaVersion: number
  id: string
  starmapId: string
  title: string
  nodes: StarMapNode[]
  edges: StarMapEdge[]
  embeds: StarMapEmbed[]
  links: StarMapLink[]
  hyperlinks: StarMapHyperlink[]
  createdAt: number
  updatedAt: number
}

// ─── 元信息 / 布局 / 渲染 ───

// StarMapMetaDto
export interface StarMapMeta {
  starmapId: string
  title: string
  description: string
  projectId: string | null
  parentStarmapId: string | null
  isMainForProject: boolean
  accentColor: string
  createdAt: number
  updatedAt: number
  nodeCount: number
  edgeCount: number
  linkedChapterCount: number
  childStarmapCount: number
}

// StarMapViewportDto
export interface StarMapViewport {
  scale: number
  offsetX: number
  offsetY: number
  width: number
  height: number
}

// StarMapLayoutKindDto
export type LayoutKind = 'Freeform' | 'AutoRadial' | 'Custom'

// StarMapLayoutDto
export interface StarMapLayout {
  kind: LayoutKind
  nodes: StarMapLayoutNode[]
}

// StarMapLayoutNodeDto
export interface StarMapLayoutNode {
  nodeId: string
  x: number
  y: number
  width: number
  height: number
  radius: number
  collapsed: boolean
  zIndex: number
  scale: number
  depth: number
  focusWeight: number
  orbitGroup: string | null
}

// StarMapEdgeRenderDto
export interface StarMapEdgeRender {
  edgeId: string
  fromCx: number
  fromCy: number
  toCx: number
  toCy: number
  startX: number
  startY: number
  endX: number
  endY: number
  offsetX: number
  offsetY: number
  arrowTipX: number
  arrowTipY: number
  arrowLeftX: number
  arrowLeftY: number
  arrowRightX: number
  arrowRightY: number
  labelX: number
  labelY: number
  label: string | null
  hasBidirectional: boolean
}

// StarMapMotionPolicyDto
export interface StarMapMotionPolicy {
  enabled: boolean
  idleWobbleEnabled: boolean
  idleAmplitudeVp: number
  idlePeriodMs: number
  dragLiftScale: number
  dragShadowBoost: number
  settleDurationMs: number
  reduceMotion: boolean
}
