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

// StarMapAnchorRoleDto
export type AnchorRole = 'Source' | 'Destination' | 'Reference' | 'Custom'

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
  type: string  // "enterPortal" | "enterEmbed"
  instanceId: string | null
  nodeId: string | null
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

// StarMapTargetPathDto
export interface StarMapTargetPath {
  starmapId: string
  segments: StarMapPathSegment[]
  target: StarMapTargetDetail
}

// StarMapPointDto
export interface StarMapPoint {
  x: number
  y: number
}

// StarMapNodeStyleDto
export interface StarMapNodeStyle {
  fillColor: string | null
}

// StarMapPortalDto
export interface StarMapPortal {
  destinationStarmapId: string
  destinationTarget: StarMapTargetDetail | null
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
  position: StarMapPoint
  style: StarMapNodeStyle
  provenance: StarMapProvenance
  createdAt: number
  updatedAt: number
}

// ─── 边 ───

// StarMapEdgeKindDto
export type EdgeKind = 'Contains' | 'References' | 'AppearsIn' | 'Causes' | 'RelatedTo'
  | 'LocatedAt' | 'CharacterRelation' | 'Timeline' | 'Foreshadows' | 'Resolves'
  | 'DependsOn' | 'ConflictsWith' | 'Custom'

// StarMapEdgeDto
export interface StarMapEdge {
  id: string
  from: StarMapTargetPath
  to: StarMapTargetPath
  kind: EdgeKind
  label: string | null
  payload: string | null  // JSON 字符串，非对象
  createdAt: number
  updatedAt: number
}

// ─── 内嵌 / 链接 / 超链接 ───

// StarMapEmbedDto
export interface StarMapEmbed {
  instanceId: string
  targetStarmapId: string
  label: string | null
  position: StarMapPoint
  hostPath: StarMapTargetPath
  provenance: StarMapProvenance
  createdAt: number
  updatedAt: number
}

// StarMapLinkDto
export interface StarMapLink {
  linkId: string
  source: StarMapTargetPath
  target: StarMapTargetPath
  label: string | null
  createdAt: number
  updatedAt: number
}

// StarMapHyperlinkDto
export interface StarMapHyperlink {
  hyperlinkId: string
  source: StarMapTargetPath
  targetUri: string
  label: string | null
  createdAt: number
  updatedAt: number
}

// ─── 图 ───

// StarMapGraphDto
export interface StarMapGraph {
  schemaVersion: number
  starmapId: string
  nodes: StarMapNode[]
  edges: StarMapEdge[]
  embeds: StarMapEmbed[]
  links: StarMapLink[]
  hyperlinks: StarMapHyperlink[]
}

// ─── 元信息 ───

// StarMapMetaDto
export interface StarMapMeta {
  starmapId: string
  title: string
  description: string
  projectId: string | null
  accentColor: string
  createdAt: number
  updatedAt: number
}
