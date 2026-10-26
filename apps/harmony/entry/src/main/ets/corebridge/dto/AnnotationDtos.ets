export type AnnotationObjectKind = 'project' | 'volume' | 'chapter' | 'textAnchor' |
  'starMap' | 'starMapNode' | 'starMapEmbed' | 'namedEntry'

export interface AnnotationObjectRefDto {
  kind: AnnotationObjectKind
  projectId: string
  objectId: string
}

export interface AnnotationTargetDto {
  objectRef: AnnotationObjectRefDto | null
  externalUrl: string | null
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
  textRevision: number
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
  state: 'exists' | 'missing' | 'projectNotDownloaded' | 'externalUrl'
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
