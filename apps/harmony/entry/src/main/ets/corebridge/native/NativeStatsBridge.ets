import { WritingStats, SpeedCurve, CurrentWritingSpeed } from '../dto/ProjectDtos'
import { EditorChangeStatsInput } from '../dto/EditorDtos'
import { ResultEnvelope, RecentEdit } from '../dto/ResultEnvelope'
import { NativeCoreModule } from './NativeCoreModule'
import { decodeWritingStats, decodeSpeedCurve, decodeCurrentWritingSpeed, decodeRecentEdit, decodeArray } from '../codec/CoreWireDecoders'

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

  // Issue #829：写作页状态栏的「今日进度」要当日口径，writer_core_get_writing_stats
  // 把区间写死成最近 30 天，所以这里把区间交给 Core 侧同一份汇总查询。
  async getWritingStatsSummary(startDate: string, endDate: string): Promise<ResultEnvelope<WritingStats>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeGetWritingStatsSummary(startDate, endDate)
      return this.core.decodeEnvelope<WritingStats>(jsonStr, decodeWritingStats)
    } catch (e) { return nativeError<WritingStats>(e) }
  }

  // Issue #829：历史写作速度曲线（按日期区间 + 分桶粒度）。曲线只负责历史形状，
  // 状态栏左段的实时「字/分」走 getCurrentWritingSpeed。
  async getWritingSpeedCurve(startDate: string, endDate: string,
    bucketMinutes: number): Promise<ResultEnvelope<SpeedCurve>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeGetWritingSpeedCurve(startDate, endDate, bucketMinutes)
      return this.core.decodeEnvelope<SpeedCurve>(jsonStr, decodeSpeedCurve)
    } catch (e) { return nativeError<SpeedCurve>(e) }
  }

  // Issue #829：状态栏左段的实时「字/分」。Core 以调用时刻为终点重算最近
  // windowSeconds 秒的窗口速度，停笔超过一个窗口后自然回落到 0。
  // 不读速度曲线最后一桶——曲线桶只生成到最后一个输入事件，停笔后会一直
  // 挂着非零值。也不在这里判断桶是否过期或强制 flush 统计事件。
  async getCurrentWritingSpeed(windowSeconds: number): Promise<ResultEnvelope<CurrentWritingSpeed>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeGetCurrentWritingSpeed(windowSeconds)
      return this.core.decodeEnvelope<CurrentWritingSpeed>(jsonStr, decodeCurrentWritingSpeed)
    } catch (e) { return nativeError<CurrentWritingSpeed>(e) }
  }

  // Issue #829 评论5：按编辑事务上报写作统计。
  // 入参 JSON 直接透传给 Core（cause→EventSource 的映射在 Core 侧做），
  // 所以这里不做 ArkTS 侧的字段猜测或 source 拼接。
  async recordEditorChangeStats(input: EditorChangeStatsInput): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = JSON.stringify(input)
      const ok: boolean = this.core.getNativeModule().nativeRecordEditorChangeStats(jsonStr)
      return { success: ok, data: ok, userMessage: ok ? undefined : 'record_editor_change_stats failed' }
    } catch (e) { return nativeError<boolean>(e) }
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
