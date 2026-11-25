import { StarMapTargetPath } from './StarMapDtos'

// #1060：对象种类与 Core ObjectRef 变体一一对应。
// 共享素材（starMap / nodeAsset）按 scope 归属，可跨画布；画布实例
// （nodeInstance / embedInstance / canvasAnchor）用 starmapId 定位画布。
export type AnnotationObjectKind = 'project' | 'volume' | 'chapter' | 'textAnchor' |
  'namedEntry' | 'starMap' | 'nodeAsset' | 'nodeInstance' | 'embedInstance' | 'canvasAnchor'

// Core 的 projectId 是 Option：空串表示全局素材域（共享星图/节点素材）。
// starmapId / anchorId 只在画布实例与画布锚点上出现。
export interface AnnotationObjectRefDto {
  kind: AnnotationObjectKind
  projectId: string
  starmapId?: string
  anchorId?: string
  objectId: string
}

/**
 * 共享节点素材的对象引用（#1060）。
 *
 * 标签、链接按 assetId 绑定，不按画布实例 id：同一素材在多个画布上的实例
 * 共享同一份标签。调用方必须传 node.assetId。
 */
export function nodeAssetRef(projectId: string, assetId: string): AnnotationObjectRefDto {
  return { kind: 'nodeAsset', projectId: projectId, objectId: assetId }
}

// #1060：链接目标要么是稳定对象引用，要么是画布导航路径（起点 + 穿越段 + 终点），
// 二者互斥。画布导航用于"跳到某个星图层里的节点/端口"，不把画布实例硬塞成对象引用。
export interface AnnotationTargetDto {
  objectRef?: AnnotationObjectRefDto
  canvasRoute?: StarMapTargetPath
}

export interface AnnotationTagDto {
  tagId: string
  projectId: string
  name: string
  color: string | null
  updatedAtMs: number
  deletedAtMs: number | null
}

export interface AnnotationTagBindingDto {
  bindingId: string
  projectId: string
  tagId: string
  target: AnnotationObjectRefDto
  updatedAtMs: number
  deletedAtMs: number | null
}

export interface AnnotationTextAnchorDto {
  anchorId: string
  projectId: string
  chapterId: string
  startByte: number
  endByte: number
  updatedAtMs: number
  deletedAtMs: number | null
}

export interface AnnotationNamedEntryDto {
  entryId: string
  projectId: string
  canonicalName: string
  aliases: string[]
  confirmedMisspellings: string[]
  autoLinkEnabled: boolean
  updatedAtMs: number
  deletedAtMs: number | null
}

export interface AnnotationObjectLinkDto {
  linkId: string
  source: AnnotationObjectRefDto
  target: AnnotationTargetDto
  origin: 'manual'
  updatedAtMs: number
  deletedAtMs: number | null
}

export interface AnnotationLinkResolutionDto {
  state: 'exists' | 'missing' | 'projectNotDownloaded'
  target: AnnotationTargetDto
}

export interface AnnotationAutoMatchDto {
  startByte: number
  endByte: number
  matchedText: string
  entryIds: string[]
  candidates: AnnotationObjectRefDto[]
}

export interface ChapterAnnotationsDto {
  projectId: string
  chapterId: string
  textRevision: number
  baselineId: string
  anchors: AnnotationTextAnchorDto[]
  bindings: AnnotationTagBindingDto[]
  links: AnnotationObjectLinkDto[]
  autoMatches: AnnotationAutoMatchDto[]
}

export interface AnnotationEditDto {
  oldStartByte: number
  oldEndByte: number
  newStartByte: number
  newEndByte: number
}

export interface AnnotationRangeDto {
  startByte: number
  endByte: number
  textRevision: number
  kind: 'tagBackground' | 'linkUnderline'
}
