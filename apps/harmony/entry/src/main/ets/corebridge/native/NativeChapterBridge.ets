import { ChapterMeta, ChapterContent, ChapterSaveReceipt, ChapterVersion } from '../dto/ProjectDtos'
import { ResultEnvelope } from '../dto/ResultEnvelope'
import { NativeCoreModule } from './NativeCoreModule'
import { decodeChapterMeta, decodeChapterContent, decodeChapterSaveReceipt, decodeArray } from '../codec/CoreWireDecoders'

function nativeError<T>(e: Object): ResultEnvelope<T> {
  return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e)}
}

export class NativeChapterBridge {
  private core: NativeCoreModule

  constructor(core: NativeCoreModule) {
    this.core = core
  }

  async listChapters(volumeId: string): Promise<ResultEnvelope<ChapterMeta[]>> {
    const projectId = await this.core.resolveVolumeProjectId(volumeId)
    if (!projectId) {
      return { success: false, errorCode: 'VOLUME_NOT_FOUND', userMessage: `Volume ${volumeId} not found in any project`}
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
      return { success: false, errorCode: 'CHAPTER_NOT_FOUND', userMessage: `Chapter ${chapterId} not found`}
    }
    try {
      const jsonStr: string = this.core.getNativeModule().nativeOpenChapter(loc.projectId, loc.volumeId, chapterId)
      return this.core.decodeEnvelope<ChapterContent>(jsonStr, decodeChapterContent)
    } catch (e) { return nativeError<ChapterContent>(e) }
  }

  async saveChapter(chapterId: string, text: string): Promise<ResultEnvelope<ChapterSaveReceipt>> {
    const loc = await this.core.resolveChapterLocation(chapterId)
    if (!loc) {
      return { success: false, errorCode: 'CHAPTER_NOT_FOUND', userMessage: `Chapter ${chapterId} not found`}
    }
    try {
      const jsonStr: string = this.core.getNativeModule().nativeSaveChapter(loc.projectId, loc.volumeId, chapterId, text)
      return this.core.decodeEnvelope<ChapterSaveReceipt>(jsonStr, decodeChapterSaveReceipt)
    } catch (e) { return nativeError<ChapterSaveReceipt>(e) }
  }

  // Issue #1054：空正文保存必须由调用方显式声明意图；普通 saveChapter 仍走安全接口。
  async saveChapterWithOptions(chapterId: string, text: string,
    allowEmptyOverwrite: boolean): Promise<ResultEnvelope<ChapterSaveReceipt>> {
    const loc = await this.core.resolveChapterLocation(chapterId)
    if (!loc) {
      return { success: false, errorCode: 'CHAPTER_NOT_FOUND', userMessage: `Chapter ${chapterId} not found`}
    }
    try {
      const jsonStr: string = this.core.getNativeModule().nativeSaveChapterWithOptions(
        loc.projectId, loc.volumeId, chapterId, text, allowEmptyOverwrite)
      return this.core.decodeEnvelope<ChapterSaveReceipt>(jsonStr, decodeChapterSaveReceipt)
    } catch (e) { return nativeError<ChapterSaveReceipt>(e) }
  }

  // Issue #1059：完整版本 CAS 的保存/清空。expected 必须来自最近一次 open/save。
  async saveChapterVersioned(chapterId: string, text: string, expected: ChapterVersion,
    baseContent: string, allowEmptyOverwrite: boolean): Promise<ResultEnvelope<ChapterSaveReceipt>> {
    const loc = await this.core.resolveChapterLocation(chapterId)
    if (!loc) {
      return { success: false, errorCode: 'CHAPTER_NOT_FOUND', userMessage: `Chapter ${chapterId} not found`}
    }
    try {
      const payload: string = JSON.stringify({
        projectId: loc.projectId,
        volumeId: loc.volumeId,
        chapterId: chapterId,
        content: text,
        generation: expected.objectGeneration,
        revision: expected.revision,
        sha256: expected.contentSha256,
        baseContent: baseContent,
        allowEmptyOverwrite: allowEmptyOverwrite,
        clear: false
      })
      const jsonStr: string = await this.core.getNativeModule().nativeSaveChapterVersioned(payload)
      return this.core.decodeEnvelope<ChapterSaveReceipt>(jsonStr, decodeChapterSaveReceipt)
    } catch (e) { return nativeError<ChapterSaveReceipt>(e) }
  }

  async clearChapterVersioned(chapterId: string, expected: ChapterVersion,
    baseContent: string): Promise<ResultEnvelope<ChapterSaveReceipt>> {
    const loc = await this.core.resolveChapterLocation(chapterId)
    if (!loc) {
      return { success: false, errorCode: 'CHAPTER_NOT_FOUND', userMessage: `Chapter ${chapterId} not found`}
    }
    try {
      const payload: string = JSON.stringify({
        projectId: loc.projectId,
        volumeId: loc.volumeId,
        chapterId: chapterId,
        generation: expected.objectGeneration,
        revision: expected.revision,
        sha256: expected.contentSha256,
        baseContent: baseContent,
        allowEmptyOverwrite: true,
        clear: true
      })
      const jsonStr: string = await this.core.getNativeModule().nativeClearChapterVersioned(payload)
      return this.core.decodeEnvelope<ChapterSaveReceipt>(jsonStr, decodeChapterSaveReceipt)
    } catch (e) { return nativeError<ChapterSaveReceipt>(e) }
  }

  async clearChapter(chapterId: string): Promise<ResultEnvelope<ChapterSaveReceipt>> {
    const loc = await this.core.resolveChapterLocation(chapterId)
    if (!loc) {
      return { success: false, errorCode: 'CHAPTER_NOT_FOUND', userMessage: `Chapter ${chapterId} not found`}
    }
    try {
      const jsonStr: string = this.core.getNativeModule().nativeClearChapter(loc.projectId, loc.volumeId, chapterId)
      return this.core.decodeEnvelope<ChapterSaveReceipt>(jsonStr, decodeChapterSaveReceipt)
    } catch (e) { return nativeError<ChapterSaveReceipt>(e) }
  }

  async createChapter(volumeId: string, name: string): Promise<ResultEnvelope<ChapterMeta>> {
    const projectId = await this.core.resolveVolumeProjectId(volumeId)
    if (!projectId) {
      return { success: false, errorCode: 'VOLUME_NOT_FOUND', userMessage: `Volume ${volumeId} not found in any project`}
    }
    try {
      const jsonStr: string = this.core.getNativeModule().nativeCreateChapter(projectId, volumeId, name)
      return this.core.decodeEnvelope<ChapterMeta>(jsonStr, decodeChapterMeta)
    } catch (e) { return nativeError<ChapterMeta>(e) }
  }

  async renameChapter(chapterId: string, newName: string): Promise<ResultEnvelope<boolean>> {
    const loc = await this.core.resolveChapterLocation(chapterId)
    if (!loc) {
      return { success: false, errorCode: 'CHAPTER_NOT_FOUND', userMessage: `Chapter ${chapterId} not found`}
    }
    try {
      const jsonStr: string = this.core.getNativeModule().nativeRenameChapter(loc.projectId, loc.volumeId, chapterId, newName)
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) { return nativeError<boolean>(e) }
  }

  async deleteChapter(chapterId: string): Promise<ResultEnvelope<boolean>> {
    const loc = await this.core.resolveChapterLocation(chapterId)
    if (!loc) {
      return { success: false, errorCode: 'CHAPTER_NOT_FOUND', userMessage: `Chapter ${chapterId} not found`}
    }
    try {
      const jsonStr: string = this.core.getNativeModule().nativeDeleteChapter(loc.projectId, loc.volumeId, chapterId)
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) { return nativeError<boolean>(e) }
  }

  async reorderChapters(volumeId: string, orderedChapterIds: string[]): Promise<ResultEnvelope<boolean>> {
    const projectId = await this.core.resolveVolumeProjectId(volumeId)
    if (!projectId) {
      return { success: false, errorCode: 'VOLUME_NOT_FOUND', userMessage: `Volume ${volumeId} not found in any project`}
    }
    try {
      const jsonStr: string = this.core.getNativeModule().nativeReorderChapters(projectId, volumeId, JSON.stringify(orderedChapterIds))
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) { return nativeError<boolean>(e) }
  }
}
