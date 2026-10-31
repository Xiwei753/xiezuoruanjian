// HarmonyCrashEventWatcher — 系统崩溃/冻屏/被杀事件观察器。
//
// 负责：
// 1. 启动时订阅 HiAppEvent 的 APP_CRASH / APP_FREEZE / APP_KILLED 事件；
// 2. 系统回放故障事件时，把白名单 params 和 external_log 复制到
//    diagnostics/crashes/<eventKey>/；按 eventKey 跨进程去重，避免每次冷启动
//    都把同一条 APP_KILLED 重新记录、重新打包；
// 3. 事件记录完成后请求一次独立的 fault 归档包（不覆盖 latest/previous）。
//
// 本类不扫描/压缩目录：ZIP 由 DiagnosticsArchiveWorker 生成。APP_KILLED 没有
// external_log 时仍会保存 reason/time，属于正常情况。

import { common } from '@kit.AbilityKit'
import hilog from '@ohos.hilog'
import fs from '@ohos.file.fs'
import hiAppEvent from '@ohos.hiviewdfx.hiAppEvent'
import { PlatformApiResolver } from '../version/PlatformApiResolver'
import { HarmonyDiagnosticEvents, DiagnosticField } from './HarmonyDiagnosticEvents'
import { HarmonyDiagnosticsExporter } from './HarmonyDiagnosticsExporter'

const LOG_TAG = 'HarmonyCrashEventWatcher'
const DIAGNOSTICS_DIR = 'diagnostics'

// 崩溃事件记录 — 用于序列化 params.json。
// ArkTS 严格模式要求对象字面量有显式 interface 类型（arkts-no-untyped-obj-literals）。
interface CrashRecord {
  domain: string
  name: string
  eventType: number
  params: Record<string, Object>
}

export class HarmonyCrashEventWatcher {
  private static instance: HarmonyCrashEventWatcher
  private context: common.UIAbilityContext | null = null
  private exporter: HarmonyDiagnosticsExporter
  private watcher: hiAppEvent.Watcher | null = null
  private initialized: boolean = false
  private processedEventKeys: Set<string> = new Set()

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
    this.subscribeCrashEvents()
  }

  destroy(): void {
    if (this.watcher !== null) {
      try {
        hiAppEvent.removeWatcher(this.watcher)
      } catch (e) {
        hilog.error(0x0000, LOG_TAG, 'removeWatcher failed: %{public}s', String(e))
      }
      this.watcher = null
    }
    this.processedEventKeys.clear()
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

  // 1. 按 eventKey 跨进程去重（params.json 已存在即视为已收尾）；
  // 2. 记录结构化事件；
  // 3. 写 params.json / 复制 external_log；
  // 4. 请求一次 fault 归档。
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
      const crashesRoot = `${this.context.filesDir}/${DIAGNOSTICS_DIR}/crashes`
      const crashesDir = `${crashesRoot}/${eventKey}`

      // 内存集合是快路径；params.json 是跨进程持久去重标记。系统回放同一事件时
      // 不再重复记录、不再重复复制日志、不再重复打包。
      if (this.processedEventKeys.has(eventKey) || this.exists(`${crashesDir}/params.json`)) {
        hilog.info(0x0000, LOG_TAG, 'Duplicate fault event key skipped: %{public}s', eventKey)
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

      fs.mkdirSync(crashesDir, true)

      const crashRecord: CrashRecord = {
        domain: hiAppEvent.domain.OS,
        name: event.name,
        eventType: event.eventType,
        params: this.safeCrashParams(params)
      }
      this.writeAtomic(`${crashesDir}/params.json`, JSON.stringify(crashRecord, null, 2))

      if (externalLog.length > 0) {
        for (let i = 0; i < externalLog.length; i++) {
          this.copyFile(externalLog[i], crashesDir, `fault-${i + 1}.log`)
        }
      } else {
        hilog.info(0x0000, LOG_TAG, 'No external_log in event %{public}s', event.name)
      }

      void this.generateFaultArchive(eventKey)
    } catch (e) {
      hilog.error(0x0000, LOG_TAG, 'handleCrashEvent failed: %{public}s', String(e))
    }
  }

  // fault 归档由 Worker 读取 crashes/<eventKey>/params.json + fault-N.log 并压缩；
  // 这里只投递一次请求，不在 ArkUI 线程做任何文件复制/压缩。
  private async generateFaultArchive(eventKey: string): Promise<void> {
    try {
      const archivePath = await this.exporter.exportFault(eventKey)
      if (archivePath === null) {
        hilog.warn(0x0000, LOG_TAG, 'Fault archive failed for event %{public}s', eventKey)
        return
      }
      hilog.info(0x0000, LOG_TAG, 'Fault archive generated for event %{public}s', eventKey)
    } catch (e) {
      hilog.error(0x0000, LOG_TAG, 'generateFaultArchive exception: %{public}s',
        e instanceof Error ? e.name : 'UNKNOWN_ERROR')
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
    if (Array.isArray(logVal)) {
      return logVal as string[]
    }
    return []
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
    const reason = params['reason']
    if (typeof reason === 'string') {
      return reason
    }
    return ''
  }

  // params.json 只保留稳定、非敏感的故障元数据。external_log 路径、exception
  // message、stack 和正文内容都不进入归档。
  private safeCrashParams(params: Record<string, Object>): Record<string, Object> {
    const safe: Record<string, Object> = {}
    this.copyNumber(params, safe, 'time')
    this.copyNumber(params, safe, 'process_id')
    this.copyNumber(params, safe, 'thread_id')
    this.copyEnum(params, safe, 'crash_type')
    this.copyEnum(params, safe, 'freeze_type')
    this.copyEnum(params, safe, 'reason')
    this.copyEnum(params, safe, 'process_name')
    this.copyEnum(params, safe, 'thread_name')
    const foreground = params['foreground']
    if (typeof foreground === 'boolean') {
      safe['foreground'] = foreground
    }
    return safe
  }

  private copyNumber(source: Record<string, Object>, target: Record<string, Object>, key: string): void {
    const value = source[key]
    if (typeof value === 'number') {
      target[key] = value
    }
  }

  private copyEnum(source: Record<string, Object>, target: Record<string, Object>, key: string): void {
    const value = source[key]
    if (typeof value === 'string' && /^[A-Za-z0-9_.:-]{1,96}$/.test(value)) {
      target[key] = value
    }
  }

  // 原子写入 — 双槽模式（崩溃安全）。
  private writeAtomic(filePath: string, content: string): void {
    const newPath = `${filePath}.new`
    const bakPath = `${filePath}.bak`

    try {
      if (!fs.accessSync(filePath) && fs.accessSync(bakPath)) {
        fs.renameSync(bakPath, filePath)
        hilog.info(0x0000, LOG_TAG, 'Restored params from backup')
      }
    } catch (_) {
    }

    const fd = fs.openSync(newPath, fs.OpenMode.CREATE | fs.OpenMode.WRITE_ONLY | fs.OpenMode.TRUNC)
    let closed = false
    try {
      fs.writeSync(fd.fd, content)
      fs.closeSync(fd)
      closed = true
    } catch (e) {
      if (!closed) {
        try {
          fs.closeSync(fd)
        } catch (_) {
        }
      }
      this.safeDelete(newPath)
      throw e instanceof Error ? e : new Error(String(e))
    }

    this.safeDelete(bakPath)
    try {
      if (fs.accessSync(filePath)) {
        fs.renameSync(filePath, bakPath)
      }
    } catch (e) {
      this.safeDelete(newPath)
      throw e instanceof Error ? e : new Error(String(e))
    }

    try {
      fs.renameSync(newPath, filePath)
    } catch (e) {
      try {
        if (fs.accessSync(bakPath)) {
          fs.renameSync(bakPath, filePath)
        }
      } catch (_) {
      }
      this.safeDelete(newPath)
      throw e instanceof Error ? e : new Error(String(e))
    }
    this.safeDelete(bakPath)
  }

  private safeDelete(filePath: string): void {
    try {
      if (fs.accessSync(filePath)) {
        fs.unlinkSync(filePath)
      }
    } catch (_) {
    }
  }

  private exists(filePath: string): boolean {
    try {
      return fs.accessSync(filePath)
    } catch (_) {
      return false
    }
  }

  // 复制 external_log 后删除系统源文件，避免故障日志目录达到上限。
  private copyFile(srcPath: string, destDir: string, fileName: string): void {
    try {
      const destPath = `${destDir}/${fileName}`
      fs.copyFileSync(srcPath, destPath)
      hilog.info(0x0000, LOG_TAG, 'Copied external fault log into diagnostics storage')
      try {
        fs.unlinkSync(srcPath)
      } catch (_) {
        hilog.warn(0x0000, LOG_TAG, 'Unable to delete copied system fault log source')
      }
    } catch (e) {
      hilog.warn(0x0000, LOG_TAG, 'copyFile failed: %{public}s', e instanceof Error ? e.name : 'UNKNOWN_ERROR')
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
