// ─── StarMap DTOs ─── 过渡模型，禁止扩展。
// 等 Core 的 StarMap 合同稳定后整段替换为 typed DTO。
// sourceId/targetId → EndpointPath；parentId/childIds → ChildMapPlacement。

export interface StarMapMeta {
  id: string
  title: string
  description: string
  nodeCount: number
  edgeCount: number
  createdAt: string
  updatedAt: string
}

export interface StarMapGraph {
  schemaVersion: number
  id: string
  starmapId: string
  title: string
  nodes: StarMapNode[]
  edges: StarMapEdge[]
  embeds: StarMapEmbed[]
  links: StarMapLink[]
  layout?: StarMapLayout
  createdAt: number
  updatedAt: number
}

export interface StarMapNode {
  id: string
  title: string
  kind: NodeType
  payload?: Record<string, Object>
  tags?: string[]
  description?: string
  color?: string
  icon?: string
  metadata?: Record<string, string>
  parentId?: string
  childIds?: string[]
}

export type NodeType = 'Character' | 'Event' | 'Location' | 'Item' | 'Concept' | 'Theme' | 'Note' | 'Organization' | 'Timeline' | 'Plot' | 'Foreshadowing' | 'Chapter' | 'Custom'

export interface StarMapEdge {
  id: string
  from?: string
  to?: string
  kind: EdgeKind
  label?: string
  payload?: Record<string, Object>
  createdAt: number
  updatedAt: number
}

export type EdgeKind = 'Contains' | 'References' | 'AppearsIn' | 'Causes' | 'RelatedTo' | 'LocatedAt' | 'CharacterRelation' | 'Timeline' | 'Foreshadows' | 'Resolves' | 'DependsOn' | 'ConflictsWith' | 'Custom'

export interface StarMapEmbed {
  id: string
  nodeId: string
  chapterId: string
  offset: number
  length: number
  text: string
  context?: string
}

export interface StarMapLink {
  id: string
  sourceNodeId: string
  targetStarmapId: string
  label?: string
  description?: string
}

export interface StarMapViewport {
  scale: number
  offsetX: number
  offsetY: number
  width: number
  height: number
}

export interface StarMapLayout {
  kind: 'Freeform' | 'AutoRadial' | 'Custom'
  nodes: StarMapLayoutNode[]
}

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
  orbitGroup?: string
}

export interface StarMapEdgeRender {
  edgeId: string
  startX: number
  startY: number
  endX: number
  endY: number
  arrowTipX: number
  arrowTipY: number
  arrowLeftX: number
  arrowLeftY: number
  arrowRightX: number
  arrowRightY: number
  labelX?: number
  labelY?: number
}

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
