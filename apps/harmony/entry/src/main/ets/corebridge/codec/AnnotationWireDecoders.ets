import { DecodeResult } from './CoreWireDecoders'
import {
  AnnotationAutoMatchDto, AnnotationLinkResolutionDto, AnnotationNamedEntryDto, AnnotationObjectKind,
  AnnotationObjectLinkDto, AnnotationObjectRefDto, AnnotationTagBindingDto,
  AnnotationTagDto, AnnotationTargetDto, AnnotationTextAnchorDto, ChapterAnnotationsDto
} from '../dto/AnnotationDtos'

type AnnotationLinkResolutionState = 'exists' | 'missing' | 'projectNotDownloaded' | 'externalUrl'

function success<T>(value: T): DecodeResult<T> { return { ok: true, value: value } }
function failure<T>(message: string): DecodeResult<T> { return { ok: false, error: message } }

function record(value: Object, label: string): Record<string, Object> | null {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) return null
  return value as Record<string, Object>
}

function text(rec: Record<string, Object>, key: string): string | null {
  const value = rec[key]
  return typeof value === 'string' ? value : null
}

function number(rec: Record<string, Object>, key: string): number | null {
  const value = rec[key]
  return typeof value === 'number' ? value : null
}

function bool(rec: Record<string, Object>, key: string): boolean | null {
  const value = rec[key]
  return typeof value === 'boolean' ? value : null
}

function nullableString(rec: Record<string, Object>, key: string): string | null | undefined {
  const value: Object | undefined = rec[key]
  if (value === undefined) return undefined
  if (value === null) return null
  return typeof value === 'string' ? value : undefined
}

function strings(rec: Record<string, Object>, key: string): string[] | null {
  const value: Object | undefined = rec[key]
  if (!Array.isArray(value)) return null
  const output: string[] = []
  for (let i = 0; i < value.length; i++) {
    if (typeof value[i] !== 'string') return null
    output.push(value[i] as string)
  }
  return output
}

function array<T>(rec: Record<string, Object>, key: string,
  decoder: (item: Object) => DecodeResult<T>): T[] | null {
  const value: Object | undefined = rec[key]
  if (!Array.isArray(value)) return null
  const output: T[] = []
  for (let i = 0; i < value.length; i++) {
    const decoded = decoder(value[i] as Object)
    if (!decoded.ok || decoded.value === undefined) return null
    output.push(decoded.value)
  }
  return output
}

function nullableNumber(rec: Record<string, Object>, key: string): number | null | undefined {
  const value: Object | undefined = rec[key]
  if (value === undefined) return undefined
  if (value === null) return null
  return typeof value === 'number' ? value : undefined
}

export function decodeAnnotationObjectRef(value: Object): DecodeResult<AnnotationObjectRefDto> {
  const rec = record(value, 'AnnotationObjectRefDto')
  if (rec === null) return failure('expected object')
  const kind = text(rec, 'kind')
  const projectId = text(rec, 'projectId')
  const objectId = text(rec, 'objectId')
  if (kind === null || projectId === null || objectId === null) return failure('missing object ref fields')
  if (!['project', 'volume', 'chapter', 'textAnchor', 'starMap', 'starMapNode', 'starMapEmbed', 'namedEntry'].includes(kind)) {
    return failure('unknown object kind')
  }
  const decoded: AnnotationObjectRefDto = {
    kind: kind as AnnotationObjectKind, projectId: projectId, objectId: objectId
  }
  return success(decoded)
}

export function decodeAnnotationTarget(value: Object): DecodeResult<AnnotationTargetDto> {
  const rec = record(value, 'AnnotationTargetDto')
  if (rec === null) return failure('expected object')
  let objectRef: AnnotationObjectRefDto | null = null
  const rawRef: Object | undefined = rec['objectRef']
  if (rawRef !== undefined && rawRef !== null) {
    const decoded = decodeAnnotationObjectRef(rawRef)
    if (!decoded.ok || decoded.value === undefined) return failure(`objectRef: ${decoded.error}`)
    objectRef = decoded.value
  }
  const externalUrl = nullableString(rec, 'externalUrl')
  if (externalUrl === undefined) return failure('externalUrl must be a string or null')
  const decoded: AnnotationTargetDto = { objectRef: objectRef, externalUrl: externalUrl }
  return success(decoded)
}

export function decodeAnnotationTag(value: Object): DecodeResult<AnnotationTagDto> {
  const rec = record(value, 'AnnotationTagDto')
  if (rec === null) return failure('expected object')
  const tagId = text(rec, 'tagId'); const projectId = text(rec, 'projectId'); const name = text(rec, 'name')
  const updatedAtMs = number(rec, 'updatedAtMs'); const color = nullableString(rec, 'color')
  const deletedAtMs = nullableNumber(rec, 'deletedAtMs')
  if (tagId === null || projectId === null || name === null || updatedAtMs === null || color === undefined || deletedAtMs === undefined) {
    return failure('missing tag fields')
  }
  const decoded: AnnotationTagDto = {
    tagId: tagId, projectId: projectId, name: name, color: color,
    updatedAtMs: updatedAtMs, deletedAtMs: deletedAtMs
  }
  return success(decoded)
}

export function decodeAnnotationTagBinding(value: Object): DecodeResult<AnnotationTagBindingDto> {
  const rec = record(value, 'AnnotationTagBindingDto')
  if (rec === null) return failure('expected object')
  const bindingId = text(rec, 'bindingId'); const projectId = text(rec, 'projectId'); const tagId = text(rec, 'tagId')
  const updatedAtMs = number(rec, 'updatedAtMs'); const deletedAtMs = nullableNumber(rec, 'deletedAtMs')
  const targetValue: Object | undefined = rec['target']
  if (bindingId === null || projectId === null || tagId === null || updatedAtMs === null || deletedAtMs === undefined || targetValue === undefined) {
    return failure('missing binding fields')
  }
  const target = decodeAnnotationObjectRef(targetValue)
  if (!target.ok || target.value === undefined) return failure(`target: ${target.error}`)
  const decoded: AnnotationTagBindingDto = {
    bindingId: bindingId, projectId: projectId, tagId: tagId,
    updatedAtMs: updatedAtMs, deletedAtMs: deletedAtMs, target: target.value
  }
  return success(decoded)
}

export function decodeAnnotationTextAnchor(value: Object): DecodeResult<AnnotationTextAnchorDto> {
  const rec = record(value, 'AnnotationTextAnchorDto')
  if (rec === null) return failure('expected object')
  const anchorId = text(rec, 'anchorId'); const projectId = text(rec, 'projectId'); const chapterId = text(rec, 'chapterId')
  const startByte = number(rec, 'startByte'); const endByte = number(rec, 'endByte'); const textRevision = number(rec, 'textRevision')
  const updatedAtMs = number(rec, 'updatedAtMs'); const deletedAtMs = nullableNumber(rec, 'deletedAtMs')
  if (anchorId === null || projectId === null || chapterId === null || startByte === null || endByte === null ||
      textRevision === null || updatedAtMs === null || deletedAtMs === undefined) return failure('missing anchor fields')
  const decoded: AnnotationTextAnchorDto = {
    anchorId: anchorId, projectId: projectId, chapterId: chapterId, startByte: startByte,
    endByte: endByte, textRevision: textRevision, updatedAtMs: updatedAtMs, deletedAtMs: deletedAtMs
  }
  return success(decoded)
}

export function decodeAnnotationNamedEntry(value: Object): DecodeResult<AnnotationNamedEntryDto> {
  const rec = record(value, 'AnnotationNamedEntryDto')
  if (rec === null) return failure('expected object')
  const entryId = text(rec, 'entryId'); const projectId = text(rec, 'projectId'); const canonicalName = text(rec, 'canonicalName')
  const aliases = strings(rec, 'aliases'); const confirmedMisspellings = strings(rec, 'confirmedMisspellings')
  const autoLinkEnabled = bool(rec, 'autoLinkEnabled'); const updatedAtMs = number(rec, 'updatedAtMs')
  const deletedAtMs = nullableNumber(rec, 'deletedAtMs')
  if (entryId === null || projectId === null || canonicalName === null || aliases === null || confirmedMisspellings === null ||
      autoLinkEnabled === null || updatedAtMs === null || deletedAtMs === undefined) return failure('missing named entry fields')
  const decoded: AnnotationNamedEntryDto = {
    entryId: entryId, projectId: projectId, canonicalName: canonicalName, aliases: aliases,
    confirmedMisspellings: confirmedMisspellings, autoLinkEnabled: autoLinkEnabled,
    updatedAtMs: updatedAtMs, deletedAtMs: deletedAtMs
  }
  return success(decoded)
}

export function decodeAnnotationLink(value: Object): DecodeResult<AnnotationObjectLinkDto> {
  const rec = record(value, 'AnnotationObjectLinkDto')
  if (rec === null) return failure('expected object')
  const linkId = text(rec, 'linkId'); const origin = text(rec, 'origin')
  const updatedAtMs = number(rec, 'updatedAtMs'); const deletedAtMs = nullableNumber(rec, 'deletedAtMs')
  const sourceValue: Object | undefined = rec['source']; const targetValue: Object | undefined = rec['target']
  if (linkId === null || origin !== 'manual' || updatedAtMs === null || deletedAtMs === undefined || sourceValue === undefined || targetValue === undefined) {
    return failure('missing link fields')
  }
  const source = decodeAnnotationObjectRef(sourceValue); const target = decodeAnnotationTarget(targetValue)
  if (!source.ok || source.value === undefined || !target.ok || target.value === undefined) return failure('invalid link endpoints')
  const decoded: AnnotationObjectLinkDto = {
    linkId: linkId, origin: 'manual', updatedAtMs: updatedAtMs, deletedAtMs: deletedAtMs,
    source: source.value, target: target.value
  }
  return success(decoded)
}

export function decodeAnnotationAutoMatch(value: Object): DecodeResult<AnnotationAutoMatchDto> {
  const rec = record(value, 'AnnotationAutoMatchDto')
  if (rec === null) return failure('expected object')
  const startByte = number(rec, 'startByte'); const endByte = number(rec, 'endByte'); const matchedText = text(rec, 'matchedText')
  const entryIds = strings(rec, 'entryIds'); const candidates = array(rec, 'candidates', decodeAnnotationObjectRef)
  if (startByte === null || endByte === null || matchedText === null || entryIds === null || candidates === null) return failure('missing auto match fields')
  const decoded: AnnotationAutoMatchDto = {
    startByte: startByte, endByte: endByte, matchedText: matchedText, entryIds: entryIds, candidates: candidates
  }
  return success(decoded)
}

export function decodeChapterAnnotations(value: Object): DecodeResult<ChapterAnnotationsDto> {
  const rec = record(value, 'ChapterAnnotationsDto')
  if (rec === null) return failure('expected object')
  const projectId = text(rec, 'projectId'); const chapterId = text(rec, 'chapterId'); const textRevision = number(rec, 'textRevision')
  const baselineId = text(rec, 'baselineId')
  const anchors = array(rec, 'anchors', decodeAnnotationTextAnchor)
  const bindings = array(rec, 'bindings', decodeAnnotationTagBinding)
  const links = array(rec, 'links', decodeAnnotationLink)
  const autoMatches = array(rec, 'autoMatches', decodeAnnotationAutoMatch)
  if (projectId === null || chapterId === null || textRevision === null || baselineId === null ||
      anchors === null || bindings === null || links === null || autoMatches === null) {
    return failure('missing chapter annotation fields')
  }
  const decoded: ChapterAnnotationsDto = {
    projectId: projectId, chapterId: chapterId, textRevision: textRevision, baselineId: baselineId,
    anchors: anchors, bindings: bindings, links: links, autoMatches: autoMatches
  }
  return success(decoded)
}

export function decodeAnnotationLinkResolution(value: Object): DecodeResult<AnnotationLinkResolutionDto> {
  const rec = record(value, 'AnnotationLinkResolutionDto')
  if (rec === null) return failure('expected object')
  const state = text(rec, 'state'); const targetValue: Object | undefined = rec['target']
  if (state === null || !['exists', 'missing', 'projectNotDownloaded', 'externalUrl'].includes(state) || targetValue === undefined) {
    return failure('missing resolution fields')
  }
  const target = decodeAnnotationTarget(targetValue)
  if (!target.ok || target.value === undefined) return failure(`target: ${target.error}`)
  const decoded: AnnotationLinkResolutionDto = {
    state: state as AnnotationLinkResolutionState, target: target.value
  }
  return success(decoded)
}
