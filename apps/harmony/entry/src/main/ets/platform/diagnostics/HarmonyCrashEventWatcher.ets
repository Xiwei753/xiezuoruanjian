// HarmonyCrashEventWatcher — 系统崩溃/冻屏/被杀事件观察器。
//
// 负责：
// 1. 启动时订阅 HiAppEvent 的 APP_CRASH / APP_FREEZE / APP_KILLED 事件；
// 2. 回放故障事件时，只提取并脱敏元数据，把 params 快照和 external_log 路径
//    投递给 DiagnosticsArchiveWorker；
// 3. Worker 在独立串行队列中创建 crashes/<eventKey>/、原子写入 params.json、
//    分块复制/清理 external_log、生成 fault ZIP，并在 ZIP 发布后写 complete.json；
// 4. 启动时请求 Worker 扫描未完成事件并从已保存的 params/fault 日志补全。
//
// 本类不在 ArkUI 线程执行 mkdir/write/copy/unlink/compress。APP_KILLED 没有
// external_log 时仍会保存 reason/time，属于正常情况。

import { common } from '@kit.AbilityKit'
import hilog from '@ohos.hilog'
import hiAppEvent from '@ohos.hiviewdfx.hiAppEvent'
import { PlatformApiResolver } from '../version/PlatformApiResolver'
import { HarmonyDiagnosticEvents, DiagnosticField } from './HarmonyDiagnosticEvents'
import { HarmonyDiagnosticsExporter } from './HarmonyDiagnosticsExporter'

const LOG_TAG = 'HarmonyCrashEventWatcher'
const SAFE_ENUM_PATTERN = /^[A-Za-z0-9_.:-]{1,96}$/

// 崩溃事件记录 — 用于序列化 params.json。
// ArkTS 严格模式要求对象字面量有显式 interface 类型（arkts-no-untyped-obj-literals）。
interface CrashRecord {
  domain: string
  name: string
  eventType: number
  params: Record<string, Object>
}

interface ThreadSummary {
  name: string
  tid: number
  state: string
}

export class HarmonyCrashEventWatcher {
  private static instance: HarmonyCrashEventWatcher
  private context: common.UIAbilityContext | null = null
  private exporter: HarmonyDiagnosticsExporter
  private watcher: hiAppEvent.Watcher | null = null
  private initialized: boolean = false
  private generation: number = 0
  private recoveryTimerId: number = -1
  // processedEventKeys 只合并同进程正在处理的同一事件；capture 失败后必须移除，
  // 让后续带着 external_log 的系统回放继续补全。持久完成状态由 Worker 的
  // complete.json + fault-state.json 判定，completedEventKeys 只是启动扫描缓存。
  private processedEventKeys: Set<string> = new Set()
  private completedEventKeys: Set<string> = new Set()

  private constructor() {
    this.exporter = new HarmonyDiagnosticsExporter()
  }

  static getInstance(): HarmonyCrashEventWatcher {
    if (!HarmonyCrashEventWatcher.instance) {
      HarmonyCrashEventWatcher.instance = new HarmonyCrashEventWatcher()
    }
    return HarmonyCrashEventWatcher.instance
  }

  init(context: common.UIAbilityContext): void {
    if (this.initialized) {
      return
    }
    this.initialized = true
    this.context = context
    this.exporter.setAbilityContext(context)
    this.generation += 1
    // 先让 Worker 扫一遍已有 crashes/<eventKey>，再订阅系统事件。这样冷启动
    // 回放的旧故障不会再次记结构化事件，也不会重复生成 fault ZIP。
    void this.prepareWatcher(this.generation)
  }

  private async prepareWatcher(generation: number): Promise<void> {
    try {
      const knownKeys = await this.exporter.listKnownFaultEventKeys()
      for (const key of knownKeys) {
        this.completedEventKeys.add(key)
      }
    } catch (e) {
      hilog.warn(0x0000, LOG_TAG, 'load known fault keys failed: %{public}s',
        e instanceof Error ? e.name : 'UNKNOWN_ERROR')
    }
    if (!this.initialized || this.generation !== generation || this.context === null) {
      return
    }
    this.subscribeCrashEvents()
    // 先给系统回放留一个短窗口：未完成故障若仍带 external_log 路径，优先由
    // fault_capture 复制；3 秒后恢复只处理 fault-state 已确认附件齐全的事件，
    // 不把“等待时间到了”当作日志已到齐。
    if (this.recoveryTimerId >= 0) {
      clearTimeout(this.recoveryTimerId)
    }
    this.recoveryTimerId = setTimeout(() => {
      this.recoveryTimerId = -1
      if (this.initialized && this.generation === generation) {
        void this.exporter.recoverIncompleteFaults()
      }
    }, 3000)
  }

  destroy(): void {
    this.generation += 1
    if (this.recoveryTimerId >= 0) {
      clearTimeout(this.recoveryTimerId)
      this.recoveryTimerId = -1
    }
    if (this.watcher !== null) {
      try {
        hiAppEvent.removeWatcher(this.watcher)
      } catch (e) {
        hilog.error(0x0000, LOG_TAG, 'removeWatcher failed: %{public}s', String(e))
      }
      this.watcher = null
    }
    this.processedEventKeys.clear()
    this.completedEventKeys.clear()
    this.initialized = false
    this.context = null
  }

  // APP_FREEZE 自 API 11 可用，兼容基线是 API 12，无需按版本分流；
  // APP_KILLED 是 API 20 新增，低版本注册会导致整个 watcher 失败。
  private subscribeCrashEvents(): void {
    try {
      const apiVersion = PlatformApiResolver.getInstance().getApiLevel()
      const eventNames: string[] = [hiAppEvent.event.APP_CRASH, hiAppEvent.event.APP_FREEZE]
      if (apiVersion >= 20) {
        eventNames.push(hiAppEvent.event.APP_KILLED)
      }

      this.watcher = {
        name: 'sujianCrashWatcher',
        appEventFilters: [
          {
            domain: hiAppEvent.domain.OS,
            names: eventNames
          }
        ],
        onReceive: (domain: string, appEventGroups: Array<hiAppEvent.AppEventGroup>) => {
          this.onCrashEventsReceived(domain, appEventGroups)
        }
      }
      hiAppEvent.addWatcher(this.watcher)
      hilog.info(0x0000, LOG_TAG, 'Subscribed to fault events (API %{public}d): %{public}s',
        apiVersion, eventNames.join(', '))
    } catch (e) {
      hilog.error(0x0000, LOG_TAG, 'subscribeCrashEvents failed: %{public}s', String(e))
    }
  }

  private onCrashEventsReceived(domain: string, appEventGroups: Array<hiAppEvent.AppEventGroup>): void {
    try {
      for (const group of appEventGroups) {
        for (const eventInfo of group.appEventInfos) {
          this.handleCrashEvent(eventInfo)
        }
      }
    } catch (e) {
      hilog.error(0x0000, LOG_TAG, 'onCrashEventsReceived failed: %{public}s', String(e))
    }
  }

  // 事件回调只做内存/字符串工作：eventKey、白名单 params、external_log 路径。
  // 目录、原子写入、大文件复制和 ZIP 都交给 Worker；跨进程完成状态由 Worker
  // 的 complete.json 判断，不能因为 params.json 已存在就跳过故障包。
  private handleCrashEvent(event: hiAppEvent.AppEventInfo): void {
    try {
      if (this.context === null) {
        return
      }
      const params = event.params as Record<string, Object>
      const time = this.extractTime(params)
      const externalLog = this.extractExternalLog(params)
      const timestamp = this.formatTimestamp(time)
      const eventKey = this.sanitizeEventKey(`${timestamp}-${event.name}`)
      if (this.completedEventKeys.has(eventKey)) {
        hilog.info(0x0000, LOG_TAG, 'Completed fault event skipped: %{public}s', eventKey)
        return
      }
      if (this.processedEventKeys.has(eventKey)) {
        hilog.info(0x0000, LOG_TAG, 'Concurrent fault event merged in process: %{public}s', eventKey)
        return
      }
      this.processedEventKeys.add(eventKey)

      const reason = this.extractNonSensitiveReason(params)
      const eventFields: DiagnosticField[] = [{ key: 'eventName', value: event.name }]
      if (reason.length > 0) {
        eventFields.push({ key: 'reason', value: reason })
      }
      if (event.name === hiAppEvent.event.APP_CRASH) {
        HarmonyDiagnosticEvents.record('Error', 'System', 'system.app_crash', 'HarmonyCrashEventWatcher', eventFields)
      } else if (event.name === hiAppEvent.event.APP_FREEZE) {
        HarmonyDiagnosticEvents.record('Error', 'System', 'system.app_freeze', 'HarmonyCrashEventWatcher', eventFields)
      } else if (event.name === hiAppEvent.event.APP_KILLED) {
        HarmonyDiagnosticEvents.record('Warn', 'System', 'system.app_killed', 'HarmonyCrashEventWatcher', eventFields)
      }

      const logOverLimit = params['log_over_limit'] === true
      const crashRecord: CrashRecord = {
        domain: hiAppEvent.domain.OS,
        name: event.name,
        eventType: event.eventType,
        params: this.safeCrashParams(params, externalLog.length)
      }
      void this.persistFault(eventKey, JSON.stringify(crashRecord, null, 2),
        externalLog, logOverLimit)
    } catch (e) {
      hilog.error(0x0000, LOG_TAG, 'handleCrashEvent failed: %{public}s', String(e))
    }
  }

  private async persistFault(eventKey: string, paramsJson: string,
    externalLog: string[], logOverLimit: boolean): Promise<void> {
    let completed = false
    try {
      const archivePath = await this.exporter.captureFault(
        eventKey, paramsJson, externalLog, externalLog.length, logOverLimit
      )
      if (archivePath === null) {
        hilog.warn(0x0000, LOG_TAG, 'Fault capture incomplete for event %{public}s', eventKey)
        return
      }
      completed = true
      this.completedEventKeys.add(eventKey)
      hilog.info(0x0000, LOG_TAG, 'Fault archive generated for event %{public}s', eventKey)
    } catch (e) {
      hilog.error(0x0000, LOG_TAG, 'persistFault exception: %{public}s',
        e instanceof Error ? e.name : 'UNKNOWN_ERROR')
    } finally {
      // 同进程并发合并结束：成功则转入 completedEventKeys；失败/未完成则释放
      // eventKey，后续系统回放可以继续复制缺失的 external_log。
      this.processedEventKeys.delete(eventKey)
      if (!completed) {
        hilog.warn(0x0000, LOG_TAG, 'Fault event remains incomplete: %{public}s', eventKey)
      }
    }
  }

  // ── 工具方法 ──────────────────────────────────────────────────

  private extractTime(params: Record<string, Object>): number {
    const timeVal = params['time']
    if (typeof timeVal === 'number') {
      return timeVal
    }
    return Date.now()
  }

  private extractExternalLog(params: Record<string, Object>): string[] {
    const logVal = params['external_log']
    if (!Array.isArray(logVal)) {
      return []
    }
    const paths: string[] = []
    const entries = logVal as Object[]
    for (const item of entries) {
      if (typeof item === 'string' && item.length > 0) {
        paths.push(item)
      }
    }
    return paths
  }

  // 只取系统枚举/短名称，不取 exception.message、stack 等可能含正文/路径的字段。
  private extractNonSensitiveReason(params: Record<string, Object>): string {
    const freezeType = params['freeze_type']
    if (typeof freezeType === 'string') {
      return freezeType
    }
    const crashType = params['crash_type']
    if (typeof crashType === 'string') {
      return crashType
    }
    const exceptionName = this.readExceptionName(params)
    if (exceptionName.length > 0) {
      return exceptionName
    }
    const reason = params['reason']
    if (typeof reason === 'string') {
      return reason
    }
    return ''
  }

  // params.json 只保留稳定、非敏感的故障元数据。external_log 路径、exception
  // message、stack 和正文内容都不进入归档；threads 只保留名称/TID/状态摘要。
  private safeCrashParams(params: Record<string, Object>,
    externalLogCount: number): Record<string, Object> {
    const safe: Record<string, Object> = {}
    safe['external_log_count'] = externalLogCount
    this.copyNumber(params, safe, 'time')
    this.copyNumber(params, safe, 'pid')
    this.copyNumber(params, safe, 'uid')
    this.copyNumber(params, safe, 'process_id')
    this.copyNumber(params, safe, 'thread_id')
    this.copyEnum(params, safe, 'crash_type')
    this.copyEnum(params, safe, 'freeze_type')
    this.copyEnum(params, safe, 'reason')
    this.copyEnum(params, safe, 'process_name')
    this.copyEnum(params, safe, 'thread_name')
    this.copyEnum(params, safe, 'bundle_name')
    this.copyEnum(params, safe, 'bundle_version')
    const exceptionName = this.readExceptionName(params)
    if (exceptionName.length > 0) {
      safe['exception_name'] = exceptionName
    }
    this.copyThreads(params, safe)
    const foreground = params['foreground']
    if (typeof foreground === 'boolean') {
      safe['foreground'] = foreground
    }
    const logOverLimit = params['log_over_limit']
    if (typeof logOverLimit === 'boolean') {
      safe['log_over_limit'] = logOverLimit
    }
    return safe
  }

  private readExceptionName(params: Record<string, Object>): string {
    const exception = params['exception']
    if (exception !== null && typeof exception === 'object') {
      const record = exception as Record<string, Object>
      const name = record['name']
      if (typeof name === 'string' && SAFE_ENUM_PATTERN.test(name)) {
        return name
      }
    }
    return ''
  }

  private copyThreads(params: Record<string, Object>, safe: Record<string, Object>): void {
    const value = params['threads']
    if (!Array.isArray(value)) {
      return
    }
    const summaries: ThreadSummary[] = []
    const entries = value as Object[]
    for (let index = 0; index < entries.length && summaries.length < 16; index++) {
      const item = entries[index]
      if (item === null || typeof item !== 'object') {
        continue
      }
      const record = item as Record<string, Object>
      const summary: ThreadSummary = {
        name: this.readSafeString(record, 'name') || this.readSafeString(record, 'thread_name'),
        tid: this.readNumber(record, 'tid', this.readNumber(record, 'thread_id', -1)),
        state: this.readSafeString(record, 'state')
      }
      if (summary.name.length > 0 || summary.tid >= 0 || summary.state.length > 0) {
        summaries.push(summary)
      }
    }
    safe['thread_count'] = entries.length
    if (summaries.length > 0) {
      safe['threads'] = summaries
    }
  }

  private readNumber(source: Record<string, Object>, key: string, fallback: number): number {
    const value = source[key]
    if (typeof value === 'number') {
      return value
    }
    return fallback
  }

  private readSafeString(source: Record<string, Object>, key: string): string {
    const value = source[key]
    if (typeof value === 'string' && SAFE_ENUM_PATTERN.test(value)) {
      return value
    }
    return ''
  }

  private copyNumber(source: Record<string, Object>, target: Record<string, Object>, key: string): void {
    const value = source[key]
    if (typeof value === 'number') {
      target[key] = value
    }
  }

  private copyEnum(source: Record<string, Object>, target: Record<string, Object>, key: string): void {
    const value = source[key]
    if (typeof value === 'string' && SAFE_ENUM_PATTERN.test(value)) {
      target[key] = value
    }
  }

  private sanitizeEventKey(key: string): string {
    return key.replace(/[^a-zA-Z0-9_-]/g, '_')
  }

  private formatTimestamp(timeMs: number): string {
    const date = new Date(timeMs)
    const year = date.getFullYear()
    const month = (date.getMonth() + 1).toString().padStart(2, '0')
    const day = date.getDate().toString().padStart(2, '0')
    const hours = date.getHours().toString().padStart(2, '0')
    const minutes = date.getMinutes().toString().padStart(2, '0')
    const seconds = date.getSeconds().toString().padStart(2, '0')
    return `${year}${month}${day}-${hours}${minutes}${seconds}`
  }
}
