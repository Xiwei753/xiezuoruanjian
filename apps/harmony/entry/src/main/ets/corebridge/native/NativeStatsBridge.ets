import { WritingStats } from '../dto/ProjectDtos'
import { ResultEnvelope, RecentEdit } from '../dto/ResultEnvelope'
import { NativeCoreModule } from './NativeCoreModule'
import { decodeWritingStats, decodeRecentEdit, decodeArray } from '../codec/CoreWireDecoders'

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

function nativeError<T>(e: Object): ResultEnvelope<T> {
  return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e)}
}

export class NativeStatsBridge {
  private isInitialized: boolean = false
  private core: NativeCoreModule

  constructor(core: NativeCoreModule) {
    this.core = core
  }

  setInitialized(v: boolean) { this.isInitialized = v }

  async getWritingStats(): Promise<ResultEnvelope<WritingStats>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeGetWritingStats()
      return this.core.decodeEnvelope<WritingStats>(jsonStr, decodeWritingStats)
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
      return this.core.decodeEnvelope<RecentEdit[]>(jsonStr,
        (d: Object) => decodeArray<RecentEdit>(d, decodeRecentEdit, 'RecentEdit'))
    } catch (e) { return nativeError<RecentEdit[]>(e) }
  }
}
