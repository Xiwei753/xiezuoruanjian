import { ChapterMeta, ChapterContent, ChapterSaveReceipt } from '../dto/ProjectDtos'
import { ChangedEntity, ResultEnvelope } from '../dto/ResultEnvelope'
import { NativeCoreModule } from './NativeCoreModule'
import { decodeChapterMeta, decodeChapterContent, decodeChapterSaveReceipt, decodeArray } from '../codec/CoreWireDecoders'

function nativeError<T>(e: Object): ResultEnvelope<T> {
  return { success: false, errorCode: 'NATIVE_ERROR', messageKey: 'error.native_error', messageArgs: { 'detail': String(e) } as Record<string, string>, rawError: String(e), warnings: [] as string[], changedPaths: [] as string[], changedEntities: [] as ChangedEntity[] }
}

export class NativeChapterBridge {
  private core: NativeCoreModule

  constructor(core: NativeCoreModule) {
    this.core = core
  }

  async listChapters(volumeId: string): Promise<ResultEnvelope<ChapterMeta[]>> {
    const projectId = await this.core.resolveVolumeProjectId(volumeId)
    if (!projectId) {
      return { success: false, errorCode: 'VOLUME_NOT_FOUND', messageKey: 'error.volume_not_found', rawError: `Volume ${volumeId} not found in any project`, warnings: [] as string[], changedPaths: [] as string[], changedEntities: [] as ChangedEntity[] }
    }
    try {
      const jsonStr: string = this.core.getNativeModule().nativeListChapters(projectId, volumeId)
      return this.core.decodeEnvelope<ChapterMeta[]>(jsonStr,
        (d: Object) => decodeArray<ChapterMeta>(d, decodeChapterMeta, 'ChapterMeta'))
    } catch (e) { return nativeError<ChapterMeta[]>(e) }
  }

  async loadChapter(chapterId: string): Promise<ResultEnvelope<ChapterContent>> {
    const loc = await this.core.resolveChapterLocation(chapterId)
    if (!loc) {
      return { success: false, errorCode: 'CHAPTER_NOT_FOUND', messageKey: 'error.chapter_not_found', rawError: `Chapter ${chapterId} not found`, warnings: [] as string[], changedPaths: [] as string[], changedEntities: [] as ChangedEntity[] }
    }
    try {
      const jsonStr: string = this.core.getNativeModule().nativeOpenChapter(loc.projectId, loc.volumeId, chapterId)
      return this.core.decodeEnvelope<ChapterContent>(jsonStr, decodeChapterContent)
    } catch (e) { return nativeError<ChapterContent>(e) }
  }

  async saveChapter(chapterId: string, text: string): Promise<ResultEnvelope<ChapterSaveReceipt>> {
    const loc = await this.core.resolveChapterLocation(chapterId)
    if (!loc) {
      return { success: false, errorCode: 'CHAPTER_NOT_FOUND', messageKey: 'error.chapter_not_found', rawError: `Chapter ${chapterId} not found`, warnings: [] as string[], changedPaths: [] as string[], changedEntities: [] as ChangedEntity[] }
    }
    try {
      const jsonStr: string = this.core.getNativeModule().nativeSaveChapter(loc.projectId, loc.volumeId, chapterId, text)
      return this.core.decodeEnvelope<ChapterSaveReceipt>(jsonStr, decodeChapterSaveReceipt)
    } catch (e) { return nativeError<ChapterSaveReceipt>(e) }
  }

  async clearChapter(chapterId: string): Promise<ResultEnvelope<ChapterSaveReceipt>> {
    const loc = await this.core.resolveChapterLocation(chapterId)
    if (!loc) {
      return { success: false, errorCode: 'CHAPTER_NOT_FOUND', messageKey: 'error.chapter_not_found', rawError: `Chapter ${chapterId} not found`, warnings: [] as string[], changedPaths: [] as string[], changedEntities: [] as ChangedEntity[] }
    }
    try {
      const jsonStr: string = this.core.getNativeModule().nativeClearChapter(loc.projectId, loc.volumeId, chapterId)
      return this.core.decodeEnvelope<ChapterSaveReceipt>(jsonStr, decodeChapterSaveReceipt)
    } catch (e) { return nativeError<ChapterSaveReceipt>(e) }
  }

  async createChapter(volumeId: string, name: string): Promise<ResultEnvelope<ChapterMeta>> {
    const projectId = await this.core.resolveVolumeProjectId(volumeId)
    if (!projectId) {
      return { success: false, errorCode: 'VOLUME_NOT_FOUND', messageKey: 'error.volume_not_found', rawError: `Volume ${volumeId} not found in any project`, warnings: [] as string[], changedPaths: [] as string[], changedEntities: [] as ChangedEntity[] }
    }
    try {
      const jsonStr: string = this.core.getNativeModule().nativeCreateChapter(projectId, volumeId, name)
      return this.core.decodeEnvelope<ChapterMeta>(jsonStr, decodeChapterMeta)
    } catch (e) { return nativeError<ChapterMeta>(e) }
  }

  async renameChapter(chapterId: string, newName: string): Promise<ResultEnvelope<boolean>> {
    const loc = await this.core.resolveChapterLocation(chapterId)
    if (!loc) {
      return { success: false, errorCode: 'CHAPTER_NOT_FOUND', messageKey: 'error.chapter_not_found', rawError: `Chapter ${chapterId} not found`, warnings: [] as string[], changedPaths: [] as string[], changedEntities: [] as ChangedEntity[] }
    }
    try {
      const jsonStr: string = this.core.getNativeModule().nativeRenameChapter(loc.projectId, loc.volumeId, chapterId, newName)
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) { return nativeError<boolean>(e) }
  }

  async deleteChapter(chapterId: string): Promise<ResultEnvelope<boolean>> {
    const loc = await this.core.resolveChapterLocation(chapterId)
    if (!loc) {
      return { success: false, errorCode: 'CHAPTER_NOT_FOUND', messageKey: 'error.chapter_not_found', rawError: `Chapter ${chapterId} not found`, warnings: [] as string[], changedPaths: [] as string[], changedEntities: [] as ChangedEntity[] }
    }
    try {
      const jsonStr: string = this.core.getNativeModule().nativeDeleteChapter(loc.projectId, loc.volumeId, chapterId)
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) { return nativeError<boolean>(e) }
  }

  async reorderChapters(volumeId: string, orderedChapterIds: string[]): Promise<ResultEnvelope<boolean>> {
    const projectId = await this.core.resolveVolumeProjectId(volumeId)
    if (!projectId) {
      return { success: false, errorCode: 'VOLUME_NOT_FOUND', messageKey: 'error.volume_not_found', rawError: `Volume ${volumeId} not found in any project`, warnings: [] as string[], changedPaths: [] as string[], changedEntities: [] as ChangedEntity[] }
    }
    try {
      const jsonStr: string = this.core.getNativeModule().nativeReorderChapters(projectId, volumeId, JSON.stringify(orderedChapterIds))
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) { return nativeError<boolean>(e) }
  }
}
