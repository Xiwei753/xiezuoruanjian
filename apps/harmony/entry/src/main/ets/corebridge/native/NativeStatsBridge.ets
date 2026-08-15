import { WritingStats } from '../dto/ProjectDtos'
import { ChangedEntity, ResultEnvelope, RecentEdit } from '../dto/ResultEnvelope'
import { NativeCoreModule } from './NativeCoreModule'

interface WritingEventDto {
  deviceId: string
  platform: string
  projectId: string
  volumeId: string
  chapterId: string
  oldText: string
  newText: string
  durationSeconds: number
  sessionId: string
}

function nativeError<T>(e: unknown): ResultEnvelope<T> {
  return { success: false, errorCode: 'NATIVE_ERROR', messageKey: 'error.native_error', messageArgs: { 'detail': String(e) } as Record<string, string>, rawError: String(e), warnings: [] as string[], changedPaths: [] as string[], changedEntities: [] as ChangedEntity[] }
}

export class NativeStatsBridge {
  private isInitialized: boolean = false
  constructor(private core: NativeCoreModule) {}

  setInitialized(v: boolean) { this.isInitialized = v }

  async getWritingStats(): Promise<ResultEnvelope<WritingStats>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeGetWritingStats()
      return this.core.parseEnvelope<WritingStats>(jsonStr)
    } catch (e) { return nativeError<WritingStats>(e) }
  }

  async calculateWordCount(text: string): Promise<number> {
    if (!this.isInitialized) { return -1 }
    try {
      return this.core.getNativeModule().nativeCalculateWordCount(text)
    } catch (e) { return -1 }
  }

  async processWritingEvent(deviceId: string, platform: string, projectId: string, volumeId: string,
    chapterId: string, oldText: string, newText: string, durationSeconds: number, sessionId: string): Promise<ResultEnvelope<boolean>> {
    try {
      const eventObj: WritingEventDto = {
        deviceId: deviceId,
        platform: platform,
        projectId: projectId,
        volumeId: volumeId,
        chapterId: chapterId,
        oldText: oldText,
        newText: newText,
        durationSeconds: durationSeconds,
        sessionId: sessionId
      }
      const eventJson = JSON.stringify(eventObj)
      const jsonStr: string = this.core.getNativeModule().nativeProcessWritingEvent(eventJson)
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) { return nativeError<boolean>(e) }
  }

  async getRecentEdits(): Promise<ResultEnvelope<RecentEdit[]>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeGetRecentEdits()
      return this.core.parseEnvelope<RecentEdit[]>(jsonStr)
    } catch (e) { return nativeError<RecentEdit[]>(e) }
  }
}
