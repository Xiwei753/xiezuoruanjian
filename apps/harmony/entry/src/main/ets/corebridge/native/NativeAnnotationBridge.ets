import { ResultEnvelope } from '../dto/ResultEnvelope'
import { EditorTransactionCause } from '../dto/EditorDtos'
import {
  AnnotationAutoMatchDto, AnnotationEditDto, AnnotationLinkResolutionDto, AnnotationNamedEntryDto, AnnotationObjectLinkDto,
  AnnotationObjectRefDto, AnnotationTagBindingDto, AnnotationTagDto, AnnotationTargetDto,
  AnnotationTextAnchorDto, ChapterAnnotationsDto
} from '../dto/AnnotationDtos'
import {
  decodeAnnotationAutoMatch, decodeAnnotationLink, decodeAnnotationNamedEntry,
  decodeAnnotationTag, decodeAnnotationTagBinding, decodeAnnotationTextAnchor,
  decodeChapterAnnotations, decodeAnnotationLinkResolution
} from '../codec/AnnotationWireDecoders'
import { decodeArray, DecodeResult } from '../codec/CoreWireDecoders'
import { NativeCoreModule } from './NativeCoreModule'

function nativeError<T>(error: Object): ResultEnvelope<T> {
  return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(error) }
}

function decodeBoolean(value: Object): DecodeResult<boolean> {
  if (typeof value !== 'boolean') return { ok: false, error: 'expected boolean' }
  return { ok: true, value: value as boolean }
}

/** Typed ArkTS facade over Core's shared annotation DTO operations. */
export class NativeAnnotationBridge {
  private core: NativeCoreModule = new NativeCoreModule()

  private async call<T>(operation: string, args: Object,
    decoder: (data: Object) => DecodeResult<T>): Promise<ResultEnvelope<T>> {
    try {
      const json: string = this.core.getNativeModule().nativeAnnotationCall(
        operation, JSON.stringify(args)) as string
      return this.core.decodeEnvelope<T>(json, decoder)
    } catch (error) {
      return nativeError<T>(error as Object)
    }
  }

  listTags(projectId: string): Promise<ResultEnvelope<AnnotationTagDto[]>> {
    return this.call('listTags', { projectId: projectId },
      (data: Object) => decodeArray(data, decodeAnnotationTag, 'AnnotationTagDto[]'))
  }

  createTag(projectId: string, name: string): Promise<ResultEnvelope<AnnotationTagDto>> {
    return this.call('createTag', { projectId: projectId, name: name, color: null }, decodeAnnotationTag)
  }

  renameTag(projectId: string, tagId: string, name: string): Promise<ResultEnvelope<AnnotationTagDto>> {
    return this.call('renameTag', { projectId: projectId, tagId: tagId, name: name }, decodeAnnotationTag)
  }

  deleteTag(projectId: string, tagId: string): Promise<ResultEnvelope<boolean>> {
    return this.call('deleteTag', { projectId: projectId, tagId: tagId }, decodeBoolean)
  }

  setTagBindings(projectId: string, target: AnnotationObjectRefDto, tagIds: string[]): Promise<ResultEnvelope<AnnotationTagBindingDto[]>> {
    return this.call('setTagBindings', { projectId: projectId, target: target, tagIds: tagIds },
      (data: Object) => decodeArray(data, decodeAnnotationTagBinding, 'AnnotationTagBindingDto[]'))
  }

  listBindingsForTarget(projectId: string, target: AnnotationObjectRefDto): Promise<ResultEnvelope<AnnotationTagBindingDto[]>> {
    return this.call('listBindingsForTarget', { projectId: projectId, target: target },
      (data: Object) => decodeArray(data, decodeAnnotationTagBinding, 'AnnotationTagBindingDto[]'))
  }

  createTextAnchor(projectId: string, chapterId: string, startByte: number, endByte: number,
    textRevision: number, text: string): Promise<ResultEnvelope<AnnotationTextAnchorDto>> {
    return this.call('createTextAnchor', { projectId: projectId, chapterId: chapterId,
      startByte: startByte, endByte: endByte, textRevision: textRevision, text: text }, decodeAnnotationTextAnchor)
  }

  getTextAnchor(projectId: string, anchorId: string): Promise<ResultEnvelope<AnnotationTextAnchorDto | null>> {
    return this.call('getTextAnchor', { projectId: projectId, anchorId: anchorId },
      (data: Object) => data === null
        ? { ok: true, value: null }
        : decodeAnnotationTextAnchor(data))
  }

  listNamedEntries(projectId: string): Promise<ResultEnvelope<AnnotationNamedEntryDto[]>> {
    return this.call('listNamedEntries', { projectId: projectId },
      (data: Object) => decodeArray(data, decodeAnnotationNamedEntry, 'AnnotationNamedEntryDto[]'))
  }

  createNamedEntry(projectId: string, canonicalName: string, autoLinkEnabled: boolean,
    aliases: string[] = [], confirmedMisspellings: string[] = []): Promise<ResultEnvelope<AnnotationNamedEntryDto>> {
    return this.call('createNamedEntry', { projectId: projectId, canonicalName: canonicalName,
      aliases: aliases, confirmedMisspellings: confirmedMisspellings,
      autoLinkEnabled: autoLinkEnabled }, decodeAnnotationNamedEntry)
  }

  updateNamedEntry(entry: AnnotationNamedEntryDto): Promise<ResultEnvelope<AnnotationNamedEntryDto>> {
    return this.call('updateNamedEntry', { projectId: entry.projectId, entryId: entry.entryId,
      canonicalName: entry.canonicalName, aliases: entry.aliases,
      confirmedMisspellings: entry.confirmedMisspellings, autoLinkEnabled: entry.autoLinkEnabled }, decodeAnnotationNamedEntry)
  }

  deleteNamedEntry(projectId: string, entryId: string): Promise<ResultEnvelope<boolean>> {
    return this.call('deleteNamedEntry', { projectId: projectId, entryId: entryId }, decodeBoolean)
  }

  createLink(source: AnnotationObjectRefDto, target: AnnotationTargetDto): Promise<ResultEnvelope<AnnotationObjectLinkDto>> {
    return this.call('createLink', { source: source, target: target }, decodeAnnotationLink)
  }

  listLinksForSource(source: AnnotationObjectRefDto): Promise<ResultEnvelope<AnnotationObjectLinkDto[]>> {
    return this.call('listLinksForSource', { source: source },
      (data: Object) => decodeArray(data, decodeAnnotationLink, 'AnnotationObjectLinkDto[]'))
  }

  deleteLink(projectId: string, linkId: string): Promise<ResultEnvelope<boolean>> {
    return this.call('deleteLink', { projectId: projectId, linkId: linkId }, decodeBoolean)
  }

  resolveLink(target: AnnotationTargetDto): Promise<ResultEnvelope<AnnotationLinkResolutionDto>> {
    return this.call('resolveLink', { target: target }, decodeAnnotationLinkResolution)
  }

  findBacklinks(target: AnnotationTargetDto): Promise<ResultEnvelope<AnnotationObjectLinkDto[]>> {
    return this.call('findBacklinks', { target: target },
      (data: Object) => decodeArray(data, decodeAnnotationLink, 'AnnotationObjectLinkDto[]'))
  }

  getChapterAnnotations(projectId: string, chapterId: string, textRevision: number,
    text: string, baselineId: string): Promise<ResultEnvelope<ChapterAnnotationsDto>> {
    return this.call('getChapterAnnotations', { projectId: projectId, chapterId: chapterId,
      textRevision: textRevision, text: text, baselineId: baselineId }, decodeChapterAnnotations)
  }

  applyAnnotationEdits(projectId: string, chapterId: string, baselineId: string, baseRevision: number,
    textRevision: number, cause: EditorTransactionCause, edits: AnnotationEditDto[]): Promise<ResultEnvelope<boolean>> {
    return this.call('applyAnnotationEdits', { projectId: projectId, chapterId: chapterId,
      baselineId: baselineId, baseRevision: baseRevision, textRevision: textRevision,
      cause: cause, edits: edits }, decodeBoolean)
  }

  findAutoNameLinks(projectId: string, chapterId: string, textRevision: number,
    text: string): Promise<ResultEnvelope<AnnotationAutoMatchDto[]>> {
    return this.call('findAutoNameLinks', { projectId: projectId, chapterId: chapterId,
      textRevision: textRevision, text: text },
      (data: Object) => decodeArray(data, decodeAnnotationAutoMatch, 'AnnotationAutoMatchDto[]'))
  }
}
