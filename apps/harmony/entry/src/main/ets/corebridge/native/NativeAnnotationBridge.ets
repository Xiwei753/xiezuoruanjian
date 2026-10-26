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

interface ProjectArgs {
  projectId: string
}

interface CreateTagArgs {
  projectId: string
  name: string
  color: string | null
}

interface RenameTagArgs {
  projectId: string
  tagId: string
  name: string
}

interface DeleteTagArgs {
  projectId: string
  tagId: string
}

interface TagBindingsArgs {
  projectId: string
  target: AnnotationObjectRefDto
  tagIds: string[]
}

interface TargetBindingsArgs {
  projectId: string
  target: AnnotationObjectRefDto
}

interface CreateTextAnchorArgs {
  projectId: string
  chapterId: string
  startByte: number
  endByte: number
  textRevision: number
  text: string
}

interface TextAnchorArgs {
  projectId: string
  anchorId: string
}

interface CreateNamedEntryArgs {
  projectId: string
  canonicalName: string
  aliases: string[]
  confirmedMisspellings: string[]
  autoLinkEnabled: boolean
}

interface UpdateNamedEntryArgs {
  projectId: string
  entryId: string
  canonicalName: string
  aliases: string[]
  confirmedMisspellings: string[]
  autoLinkEnabled: boolean
}

interface NamedEntryArgs {
  projectId: string
  entryId: string
}

interface CreateLinkArgs {
  source: AnnotationObjectRefDto
  target: AnnotationTargetDto
}

interface SourceArgs {
  source: AnnotationObjectRefDto
}

interface DeleteLinkArgs {
  projectId: string
  linkId: string
}

interface TargetArgs {
  target: AnnotationTargetDto
}

interface ChapterAnnotationsArgs {
  projectId: string
  chapterId: string
  textRevision: number
  text: string
  baselineId: string
}

interface ApplyAnnotationEditsArgs {
  projectId: string
  chapterId: string
  baselineId: string
  baseRevision: number
  textRevision: number
  cause: EditorTransactionCause
  edits: AnnotationEditDto[]
}

interface AutoNameLinksArgs {
  projectId: string
  chapterId: string
  textRevision: number
  text: string
}

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
    const args: ProjectArgs = { projectId: projectId }
    return this.call('listTags', args,
      (data: Object) => decodeArray(data, decodeAnnotationTag, 'AnnotationTagDto[]'))
  }

  createTag(projectId: string, name: string): Promise<ResultEnvelope<AnnotationTagDto>> {
    const args: CreateTagArgs = { projectId: projectId, name: name, color: null }
    return this.call('createTag', args, decodeAnnotationTag)
  }

  renameTag(projectId: string, tagId: string, name: string): Promise<ResultEnvelope<AnnotationTagDto>> {
    const args: RenameTagArgs = { projectId: projectId, tagId: tagId, name: name }
    return this.call('renameTag', args, decodeAnnotationTag)
  }

  deleteTag(projectId: string, tagId: string): Promise<ResultEnvelope<boolean>> {
    const args: DeleteTagArgs = { projectId: projectId, tagId: tagId }
    return this.call('deleteTag', args, decodeBoolean)
  }

  setTagBindings(projectId: string, target: AnnotationObjectRefDto, tagIds: string[]): Promise<ResultEnvelope<AnnotationTagBindingDto[]>> {
    const args: TagBindingsArgs = { projectId: projectId, target: target, tagIds: tagIds }
    return this.call('setTagBindings', args,
      (data: Object) => decodeArray(data, decodeAnnotationTagBinding, 'AnnotationTagBindingDto[]'))
  }

  listBindingsForTarget(projectId: string, target: AnnotationObjectRefDto): Promise<ResultEnvelope<AnnotationTagBindingDto[]>> {
    const args: TargetBindingsArgs = { projectId: projectId, target: target }
    return this.call('listBindingsForTarget', args,
      (data: Object) => decodeArray(data, decodeAnnotationTagBinding, 'AnnotationTagBindingDto[]'))
  }

  createTextAnchor(projectId: string, chapterId: string, startByte: number, endByte: number,
    textRevision: number, text: string): Promise<ResultEnvelope<AnnotationTextAnchorDto>> {
    const args: CreateTextAnchorArgs = { projectId: projectId, chapterId: chapterId,
      startByte: startByte, endByte: endByte, textRevision: textRevision, text: text }
    return this.call('createTextAnchor', args, decodeAnnotationTextAnchor)
  }

  getTextAnchor(projectId: string, anchorId: string): Promise<ResultEnvelope<AnnotationTextAnchorDto | null>> {
    const args: TextAnchorArgs = { projectId: projectId, anchorId: anchorId }
    return this.call('getTextAnchor', args,
      (data: Object) => data === null
        ? { ok: true, value: null }
        : decodeAnnotationTextAnchor(data))
  }

  listNamedEntries(projectId: string): Promise<ResultEnvelope<AnnotationNamedEntryDto[]>> {
    const args: ProjectArgs = { projectId: projectId }
    return this.call('listNamedEntries', args,
      (data: Object) => decodeArray(data, decodeAnnotationNamedEntry, 'AnnotationNamedEntryDto[]'))
  }

  createNamedEntry(projectId: string, canonicalName: string, autoLinkEnabled: boolean,
    aliases: string[] = [], confirmedMisspellings: string[] = []): Promise<ResultEnvelope<AnnotationNamedEntryDto>> {
    const args: CreateNamedEntryArgs = { projectId: projectId, canonicalName: canonicalName,
      aliases: aliases, confirmedMisspellings: confirmedMisspellings,
      autoLinkEnabled: autoLinkEnabled }
    return this.call('createNamedEntry', args, decodeAnnotationNamedEntry)
  }

  updateNamedEntry(entry: AnnotationNamedEntryDto): Promise<ResultEnvelope<AnnotationNamedEntryDto>> {
    const args: UpdateNamedEntryArgs = { projectId: entry.projectId, entryId: entry.entryId,
      canonicalName: entry.canonicalName, aliases: entry.aliases,
      confirmedMisspellings: entry.confirmedMisspellings, autoLinkEnabled: entry.autoLinkEnabled }
    return this.call('updateNamedEntry', args, decodeAnnotationNamedEntry)
  }

  deleteNamedEntry(projectId: string, entryId: string): Promise<ResultEnvelope<boolean>> {
    const args: NamedEntryArgs = { projectId: projectId, entryId: entryId }
    return this.call('deleteNamedEntry', args, decodeBoolean)
  }

  createLink(source: AnnotationObjectRefDto, target: AnnotationTargetDto): Promise<ResultEnvelope<AnnotationObjectLinkDto>> {
    const args: CreateLinkArgs = { source: source, target: target }
    return this.call('createLink', args, decodeAnnotationLink)
  }

  listLinksForSource(source: AnnotationObjectRefDto): Promise<ResultEnvelope<AnnotationObjectLinkDto[]>> {
    const args: SourceArgs = { source: source }
    return this.call('listLinksForSource', args,
      (data: Object) => decodeArray(data, decodeAnnotationLink, 'AnnotationObjectLinkDto[]'))
  }

  deleteLink(projectId: string, linkId: string): Promise<ResultEnvelope<boolean>> {
    const args: DeleteLinkArgs = { projectId: projectId, linkId: linkId }
    return this.call('deleteLink', args, decodeBoolean)
  }

  resolveLink(target: AnnotationTargetDto): Promise<ResultEnvelope<AnnotationLinkResolutionDto>> {
    const args: TargetArgs = { target: target }
    return this.call('resolveLink', args, decodeAnnotationLinkResolution)
  }

  findBacklinks(target: AnnotationTargetDto): Promise<ResultEnvelope<AnnotationObjectLinkDto[]>> {
    const args: TargetArgs = { target: target }
    return this.call('findBacklinks', args,
      (data: Object) => decodeArray(data, decodeAnnotationLink, 'AnnotationObjectLinkDto[]'))
  }

  getChapterAnnotations(projectId: string, chapterId: string, textRevision: number,
    text: string, baselineId: string): Promise<ResultEnvelope<ChapterAnnotationsDto>> {
    const args: ChapterAnnotationsArgs = { projectId: projectId, chapterId: chapterId,
      textRevision: textRevision, text: text, baselineId: baselineId }
    return this.call('getChapterAnnotations', args, decodeChapterAnnotations)
  }

  applyAnnotationEdits(projectId: string, chapterId: string, baselineId: string, baseRevision: number,
    textRevision: number, cause: EditorTransactionCause, edits: AnnotationEditDto[]): Promise<ResultEnvelope<boolean>> {
    const args: ApplyAnnotationEditsArgs = { projectId: projectId, chapterId: chapterId,
      baselineId: baselineId, baseRevision: baseRevision, textRevision: textRevision,
      cause: cause, edits: edits }
    return this.call('applyAnnotationEdits', args, decodeBoolean)
  }

  findAutoNameLinks(projectId: string, chapterId: string, textRevision: number,
    text: string): Promise<ResultEnvelope<AnnotationAutoMatchDto[]>> {
    const args: AutoNameLinksArgs = { projectId: projectId, chapterId: chapterId,
      textRevision: textRevision, text: text }
    return this.call('findAutoNameLinks', args,
      (data: Object) => decodeArray(data, decodeAnnotationAutoMatch, 'AnnotationAutoMatchDto[]'))
  }
}
