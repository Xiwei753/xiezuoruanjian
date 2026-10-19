import { WritingStats, SpeedCurve, CurrentWritingSpeed } from '../dto/ProjectDtos'
import { WritingStatsExplorer, WritingStatsHeatmap } from '../dto/StatsDtos'
import { EditorChangeStatsInput } from '../dto/EditorDtos'
import { ResultEnvelope, RecentEdit } from '../dto/ResultEnvelope'
import { NativeCoreModule } from './NativeCoreModule'
import { decodeWritingStats, decodeSpeedCurve, decodeCurrentWritingSpeed, decodeRecentEdit, decodeArray, decodeWritingStatsExplorer, decodeWritingStatsHeatmap } from '../codec/CoreWireDecoders'

function nativeError<T>(e: Object): ResultEnvelope<T> {
  return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e)}
}

// 统计页 Explorer 请求结果（Issue #945）：requestSeq 是本次 Bridge 实例内单调递增的
// 请求序号。统计查询跑在线程池，用户快速切换时段时旧范围的响应可能晚于新范围到达；
// 调用方拿到结果后先用 isLatestExplorerRequest 过滤旧序号，不覆盖当前时段快照。
export interface StatsExplorerRequestResult {
  requestSeq: number
  envelope: ResultEnvelope<WritingStatsExplorer>
}

// 每日码字日历请求结果（Issue #956）：与 Explorer 同一套旧请求过滤机制。
// 用户快速切换年份时旧年份的响应可能晚到；调用方拿到结果后先用
// isLatestHeatmapRequest 过滤旧序号，不覆盖当前年份快照。
export interface StatsHeatmapRequestResult {
  requestSeq: number
  envelope: ResultEnvelope<WritingStatsHeatmap>
}

export class NativeStatsBridge {
  private isInitialized: boolean = false
  private core: NativeCoreModule
  private explorerRequestSeq: number = 0
  private heatmapRequestSeq: number = 0

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

  // Issue #944/#945：统计页 Explorer 快照——生涯摘要 + 四张时段卡 + 当前选中时段详情。
  // preset 只传 Core 约定的固定字符串（last7Days / last30Days / last6Months / lifetime /
  // custom）；customStart/customEnd 只在 preset=custom 时是用户所选日期，其余传空串
  // （空串在 C/Rust 侧表示「未提供」），未来结束日原样透传，不在端侧改写成今天。
  //
  // nativeGetWritingStatsExplorer 返回的是 Promise —— 真正的切线程点在 C++ 侧
  // （Node-API async work，execute 回调跑在线程池）。ArkTS 的 async 函数不会自己开线程，
  // 光在 ArkTS 侧包 async 没有意义，必须 await 那个 Promise。
  async getWritingStatsExplorer(preset: string, customStartDate: string, customEndDate: string): Promise<StatsExplorerRequestResult> {
    this.explorerRequestSeq += 1
    const requestSeq: number = this.explorerRequestSeq
    let envelope: ResultEnvelope<WritingStatsExplorer>
    try {
      const jsonStr: string = await this.core.getNativeModule().nativeGetWritingStatsExplorer(preset, customStartDate, customEndDate)
      envelope = this.core.decodeEnvelope<WritingStatsExplorer>(jsonStr, decodeWritingStatsExplorer)
    } catch (e) {
      envelope = nativeError<WritingStatsExplorer>(e)
    }
    return { requestSeq: requestSeq, envelope: envelope }
  }

  /** 该请求序号是否仍是最新一次统计查询（旧范围晚到结果返回 false）。 */
  isLatestExplorerRequest(requestSeq: number): boolean {
    return requestSeq === this.explorerRequestSeq
  }

  // Issue #956：每日码字日历——指定公历年的逐日人工输入真值（本机 + 已同步设备
  // 事件去重后）。year 是本地年份；Core 只接受 1970–2100，越界返回结构化错误，
  // 这里原样把 envelope 交给页面错误区域，不用假 0 数据遮盖失败。
  //
  // nativeGetWritingStatsHeatmap 返回的是 Promise——真正切线程点在 C++ 侧
  // （Node-API async work，execute 回调跑在线程池），必须 await。
  async getWritingStatsHeatmap(year: number): Promise<StatsHeatmapRequestResult> {
    this.heatmapRequestSeq += 1
    const requestSeq: number = this.heatmapRequestSeq
    let envelope: ResultEnvelope<WritingStatsHeatmap>
    try {
      const jsonStr: string = await this.core.getNativeModule().nativeGetWritingStatsHeatmap(year)
      envelope = this.core.decodeEnvelope<WritingStatsHeatmap>(jsonStr, decodeWritingStatsHeatmap)
    } catch (e) {
      envelope = nativeError<WritingStatsHeatmap>(e)
    }
    return { requestSeq: requestSeq, envelope: envelope }
  }

  /** 该请求序号是否仍是最新一次日历查询（旧年份晚到结果返回 false）。 */
  isLatestHeatmapRequest(requestSeq: number): boolean {
    return requestSeq === this.heatmapRequestSeq
  }

  // Issue #829：写作页状态栏的「今日进度」要当日口径，writer_core_get_writing_stats
  // 把区间写死成最近 30 天，所以这里把区间交给 Core 侧同一份汇总查询。
  async getWritingStatsSummary(startDate: string, endDate: string): Promise<ResultEnvelope<WritingStats>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeGetWritingStatsSummary(startDate, endDate)
      return this.core.decodeEnvelope<WritingStats>(jsonStr, decodeWritingStats)
    } catch (e) { return nativeError<WritingStats>(e) }
  }

  // Issue #829 评论10：「今天是哪一天」由 Core 的本地日历口径决定，端侧不传日期。
  // 每日统计按事件发生地的本地午夜分桶，端侧自己拼 YYYY-MM-DD 一旦时区口径错开，
  // 凌晨就会出现「今日进度提前清零」。
  async getTodayWritingStatsSummary(): Promise<ResultEnvelope<WritingStats>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeGetTodayWritingStatsSummary()
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

  // Issue #829 评论5/7：按编辑事务上报写作统计。
  // 入参 JSON 直接透传给 Core（cause→EventSource 的映射在 Core 侧做），
  // 所以这里不做 ArkTS 侧的字段猜测或 source 拼接。
  // deviceId 也不在这里传：设备身份由 Core 自己读/建（见 record_editor_change_stats）。
  //
  // nativeRecordEditorChangeStats 返回的是 Promise —— 真正的切线程点在 C++ 侧
  // （Node-API async work，execute 回调跑在线程池）。ArkTS 的 async 函数不会自己
  // 开线程，所以这里必须 await 那个 Promise，而不是包一层 async 就当异步了。
  // Promise reject（Core 返回 false，或 work 被中止）不能当「这条已完成」吞掉。
  async recordEditorChangeStats(input: EditorChangeStatsInput): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = JSON.stringify(input)
      const ok: boolean = await this.core.getNativeModule().nativeRecordEditorChangeStats(jsonStr)
      return { success: ok, data: ok, userMessage: ok ? undefined : 'record_editor_change_stats failed' }
    } catch (e) { return nativeError<boolean>(e) }
  }

  async calculateWordCount(text: string): Promise<number> {
    if (!this.isInitialized) { return -1 }
    try {
      return this.core.getNativeModule().nativeCalculateWordCount(text)
    } catch (e) { return -1 }
  }

  async getRecentEdits(): Promise<ResultEnvelope<RecentEdit[]>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeGetRecentEdits()
      return this.core.decodeEnvelope<RecentEdit[]>(jsonStr,
        (d: Object) => decodeArray<RecentEdit>(d, decodeRecentEdit, 'RecentEdit'))
    } catch (e) { return nativeError<RecentEdit[]>(e) }
  }
}
