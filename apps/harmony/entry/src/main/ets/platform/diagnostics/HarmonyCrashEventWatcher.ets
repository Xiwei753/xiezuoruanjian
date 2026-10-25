// HarmonyCrashEventWatcher — 系统崩溃事件观察器。
//
// 负责：
// 1. 在应用启动时订阅 HiAppEvent 的 APP_CRASH 和 APP_KILLED 事件
// 2. 系统在下一次启动回放上一轮故障事件时，把 params 和 external_log 复制进
//    diagnostics/crashes/<timestamp>/ 目录
// 3. 额外由 ArkTS zlib 生成 sujian-diagnostics-crash-<timestamp>.zip 崩溃补全包
//    （不覆盖平时维护的 latest.zip / previous.zip）
//
// HiAppEvent 的 APP_CRASH 事件 params 中包含 time、external_log 等系统定义字段。
// external_log 是 string[] 类型，包含系统生成的 crash fault log 文件路径。
//
// 参考 HarmonyDiagnosticsExporter.ets 的附件收集和 zip 生成模式。

import { common } from '@kit.AbilityKit'
import hilog from '@ohos.hilog'
import fs from '@ohos.file.fs'
import hiAppEvent from '@ohos.hiviewdfx.hiAppEvent'
import util from '@ohos.util'
import { NativeDiagnosticsBridge } from '../../corebridge/native/NativeDiagnosticsBridge'
import { NativeCoreModule } from '../../corebridge/native/NativeCoreModule'
import { PlatformApiResolver } from '../version/PlatformApiResolver'
import { HarmonyDiagnosticEvents, DiagnosticField } from './HarmonyDiagnosticEvents'
import { HarmonyDiagnosticsArchive } from './HarmonyDiagnosticsArchive'
import { HarmonyDiagnosticsCheckpoint } from './HarmonyDiagnosticsCheckpoint'

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
  private diagnosticsBridge: NativeDiagnosticsBridge
  private watcher: hiAppEvent.Watcher | null = null
  private initialized: boolean = false
  private processedEventKeys: Set<string> = new Set()

  private constructor() {
    this.diagnosticsBridge = new NativeDiagnosticsBridge(new NativeCoreModule())
  }

  static getInstance(): HarmonyCrashEventWatcher {
    if (!HarmonyCrashEventWatcher.instance) {
      HarmonyCrashEventWatcher.instance = new HarmonyCrashEventWatcher()
    }
    return HarmonyCrashEventWatcher.instance
  }

  // 在 EntryAbility.onCreate 中调用，订阅 HiAppEvent 崩溃事件。
  // 系统会在下一次启动时回放上一轮的 crash/killed 事件。
  // Issue #773 评论 5845398115：生命周期幂等 — UIAbility 在同一进程销毁后重建时，
  // 不重复注册 HiAppEvent watcher。destroy() 后 initialized 复位，允许下次 init 重新订阅。
  init(context: common.UIAbilityContext): void {
    if (this.initialized) {
      return
    }
    this.initialized = true
    this.context = context
    this.subscribeCrashEvents()
  }

  // Issue #773 评论 5845398115：Ability 销毁时解绑 HiAppEvent watcher。
  // 进程 crash 时系统直接杀掉，不依赖 destroy；正常 Ability 销毁要解绑避免泄漏。
  destroy(): void {
    if (this.watcher !== null) {
      try {
        hiAppEvent.removeWatcher(this.watcher)
      } catch (e) {
        hilog.error(0x0000, LOG_TAG, 'removeWatcher failed: %{public}s', String(e))
      }
      this.watcher = null
    }
    this.initialized = false
    this.context = null
  }

  // 订阅 APP_CRASH 和 APP_KILLED 事件。
  // 使用 addWatcher + onReceive 实时接收系统事件回调。
  // APP_KILLED 是 API 20 新增事件，API 12-19 设备上注册会导致整个 watcher 失败（包括 APP_CRASH），
  // 因此按运行时 API Level 分流：API >= 20 才注册 APP_KILLED。
  private subscribeCrashEvents(): void {
    try {
      // Issue #773 评论 5845398115：UIAbilityContext.config 是 Configuration，
      // 官方 Configuration 字段没有 apiVersion。统一走 PlatformApiResolver 读 deviceInfo.sdkApiVersion。
      const apiVersion = PlatformApiResolver.getInstance().getApiLevel()

      const eventNames: string[] = [hiAppEvent.event.APP_CRASH]
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
      hilog.info(0x0000, LOG_TAG, 'Subscribed to crash events (API %{public}d): %{public}s',
        apiVersion, eventNames.join(', '))
    } catch (e) {
      hilog.error(0x0000, LOG_TAG, 'subscribeCrashEvents failed: %{public}s', String(e))
    }
  }

  // onReceive 回调 — 遍历事件组，对每个 AppEventInfo 调用 handleCrashEvent。
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

  // 处理单个崩溃事件：
  // 1. 创建 crashes/<eventKey>/ 目录
  // 2. 写 params.json
  // 3. 复制 external_log 文件
  // 4. 生成崩溃补全包 zip
  private handleCrashEvent(event: hiAppEvent.AppEventInfo): void {
    try {
      // 从 params 中提取 time 和 external_log（系统事件字段在 params 对象中）
      const params = event.params as Record<string, Object>
      const time = this.extractTime(params)
      const externalLog = this.extractExternalLog(params)
      const timestamp = this.formatTimestamp(time)
      const eventKey = this.sanitizeEventKey(`${timestamp}-${event.name}`)

      // 去重：同一 eventKey 的重复事件直接跳过
      if (this.processedEventKeys.has(eventKey)) {
        hilog.info(0x0000, LOG_TAG, 'Duplicate crash event key skipped: %{public}s', eventKey)
        return
      }
      this.processedEventKeys.add(eventKey)

      // Issue #906：把系统崩溃/被杀事件补进结构化诊断。放在去重之后，避免同一事件重复上报。
      // 只记事件名和非敏感原因枚举，不记 exception.message/stack、external_log 路径等
      // 可能含正文/路径的内容；也不把 crash-copy/zip 逻辑搬进 facade。
      const reason = this.extractNonSensitiveReason(params)
      const eventFields: DiagnosticField[] = [{ key: 'eventName', value: event.name }]
      if (reason.length > 0) {
        eventFields.push({ key: 'reason', value: reason })
      }
      if (event.name === hiAppEvent.event.APP_CRASH) {
        HarmonyDiagnosticEvents.record('Error', 'System', 'system.app_crash', 'HarmonyCrashEventWatcher', eventFields)
      } else if (event.name === hiAppEvent.event.APP_KILLED) {
        HarmonyDiagnosticEvents.record('Warn', 'System', 'system.app_killed', 'HarmonyCrashEventWatcher', eventFields)
      }

      const crashesDir = `${this.context!.filesDir}/${DIAGNOSTICS_DIR}/crashes/${eventKey}`
      fs.mkdirSync(crashesDir, true)

      // 只持久化崩溃摘要白名单；external_log 的原始 fault 内容单独保存在
      // diagnostics/crashes/<eventKey>/fault-N.log，原始系统日志保持完整。
      const crashRecord: CrashRecord = {
        domain: event.domain === hiAppEvent.domain.OS ? event.domain : hiAppEvent.domain.OS,
        name: event.name,
        eventType: event.eventType,
        params: this.safeCrashParams(params)
      }
      const paramsJson = JSON.stringify(crashRecord, null, 2)
      this.writeAtomic(`${crashesDir}/params.json`, paramsJson)

      // 复制 external_log 文件到 crashes 目录
      // APP_KILLED 事件不保证有 external_log（官方样例只有 reason/time/foreground 等字段），
      // 没有 external_log 是正常情况，用 info 级别记录，不当失败处理。
      if (externalLog.length > 0) {
        for (let i = 0; i < externalLog.length; i++) {
          this.copyFile(externalLog[i], crashesDir, `fault-${i + 1}.log`)
        }
      } else {
        hilog.info(0x0000, LOG_TAG, 'No external_log in event %{public}s (normal for APP_KILLED)', event.name)
      }

      // 崩溃事件和持久化日志均触发一轮合并 checkpoint。
      HarmonyDiagnosticsCheckpoint.getInstance().markDirty()

      // 生成崩溃补全包 zip
      void this.generateCrashZip(eventKey, crashRecord)
    } catch (e) {
      hilog.error(0x0000, LOG_TAG, 'handleCrashEvent failed: %{public}s', String(e))
    }
  }

  // 通过同一 ArkTS archive 服务生成崩溃补全包；native flush 失败不影响归档。
  // 崩溃包不覆盖 latest.zip / previous.zip，使用独立的 crash 前缀文件名。
  private async generateCrashZip(eventKey: string, crashRecord: CrashRecord): Promise<void> {
    let stagingDir: string = ''
    try {
      // 1. Best-effort flush of native diagnostics; no native export dependency.
      try {
        const flushResult = this.diagnosticsBridge.flushDiagnostics()
        if (flushResult !== 0) {
          hilog.warn(0x0000, LOG_TAG, 'flushDiagnostics returned %{public}d; continuing crash archive', flushResult)
        }
      } catch (_) {
        hilog.info(0x0000, LOG_TAG, 'native diagnostics flush unavailable; continuing crash archive')
      }

      // 2. 准备输出目录
      const outputDir = `${this.context!.filesDir}/${DIAGNOSTICS_DIR}/snapshots`
      this.ensureOutputDir(outputDir)
      stagingDir = `${this.context!.filesDir}/${DIAGNOSTICS_DIR}/.staging/crash-${eventKey}-${Date.now()}`
      this.ensureOutputDir(stagingDir)

      // 3. 仅写入安全参数摘要和已持久化的 fault 日志副本。
      const relativeCrashDir = `crashes/${eventKey}`
      HarmonyDiagnosticsArchive.writeTextFile(
        stagingDir, `${relativeCrashDir}/params.json`, JSON.stringify(crashRecord, null, 2)
      )
      const crashesDir = `${this.context!.filesDir}/${DIAGNOSTICS_DIR}/crashes/${eventKey}`
      for (const fileName of fs.listFileSync(crashesDir)) {
        if (!/^fault-[0-9]+\.log$/.test(fileName)) {
          continue
        }
        const persistedPath = `${crashesDir}/${fileName}`
        const logContent = this.readFileContent(persistedPath)
        if (logContent.length > 0) {
          HarmonyDiagnosticsArchive.writeTextFile(
            stagingDir, `${relativeCrashDir}/${fileName}`, logContent
          )
        }
      }

      // 4. NativeDiagnosticsBridge.exportDiagnostics is not part of crash ZIP creation.
      await HarmonyDiagnosticsArchive.compressDirectory(
        stagingDir, outputDir, `sujian-diagnostics-crash-${eventKey}`, true
      )
      hilog.info(0x0000, LOG_TAG, 'Crash archive generated for event %{public}s', eventKey)
    } catch (e) {
      hilog.error(0x0000, LOG_TAG, 'generateCrashZip failed: %{public}s', e instanceof Error ? e.name : 'UNKNOWN_ERROR')
    } finally {
      if (stagingDir.length > 0) {
        HarmonyDiagnosticsArchive.removeTree(stagingDir)
      }
    }
  }

  // ── 工具方法 ──────────────────────────────────────────────────

  // 从 params 对象中提取 time 字段（毫秒时间戳）。
  // 系统事件的 time 字段在 params 中，不在 AppEventInfo 顶层。
  private extractTime(params: Record<string, Object>): number {
    const timeVal = params['time']
    if (typeof timeVal === 'number') {
      return timeVal
    }
    // fallback: 使用当前时间
    return Date.now()
  }

  // 从 params 对象中提取 external_log 字段（string[]）。
  private extractExternalLog(params: Record<string, Object>): string[] {
    const logVal = params['external_log']
    if (Array.isArray(logVal)) {
      return logVal as string[]
    }
    return []
  }

  // Issue #906：从 params 中提取非敏感的故障类型/原因枚举。
  // APP_CRASH 用 crash_type（NativeCrash / JsError），APP_KILLED 用 reason。
  // 只取枚举字符串，不取 exception.message、stack 等可能含正文/路径的字段。
  private extractNonSensitiveReason(params: Record<string, Object>): string {
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

  // params.json keeps only stable, non-sensitive crash metadata. In particular,
  // external_log paths, exception messages and stacks stay out of the archive.
  private safeCrashParams(params: Record<string, Object>): Record<string, Object> {
    const safe: Record<string, Object> = {}
    const time = params['time']
    if (typeof time === 'number') {
      safe['time'] = time
    }
    const crashType = params['crash_type']
    if (typeof crashType === 'string' && /^[A-Za-z0-9_.-]{1,64}$/.test(crashType)) {
      safe['crash_type'] = crashType
    }
    const reason = params['reason']
    if (typeof reason === 'string' && /^[A-Za-z0-9_.-]{1,64}$/.test(reason)) {
      safe['reason'] = reason
    }
    const foreground = params['foreground']
    if (typeof foreground === 'boolean') {
      safe['foreground'] = foreground
    }
    return safe
  }

  // 原子写入 — 双槽模式（崩溃安全）：
  // 1. 写完整 file.new
  // 2. 删旧 file.bak（如果存在）
  // 3. file → file.bak（如果 file 存在）
  // 4. file.new → file
  // 5. 成功后删 file.bak
  // 如果进程在步骤 3-4 之间崩溃，下次写入时 file 不存在但 file.bak 存在，
  // 自动恢复 file.bak → file。
  private writeAtomic(filePath: string, content: string): void {
    const newPath = `${filePath}.new`
    const bakPath = `${filePath}.bak`

    // 初始化恢复：如果目标文件不存在但 .bak 存在，说明上次写入中断，恢复 .bak → 目标
    try {
      if (!fs.accessSync(filePath) && fs.accessSync(bakPath)) {
        fs.renameSync(bakPath, filePath)
        hilog.info(0x0000, LOG_TAG, 'Restored from backup: %{public}s → %{public}s', bakPath, filePath)
      }
    } catch (_) { /* 恢复失败不影响后续写入 */ }

    // 1. 写完整 .new 文件
    const fd = fs.openSync(newPath, fs.OpenMode.CREATE | fs.OpenMode.WRITE_ONLY | fs.OpenMode.TRUNC)
    let closed = false
    try {
      fs.writeSync(fd.fd, content)
      fs.closeSync(fd)
      closed = true
    } catch (e) {
      if (!closed) {
        try { fs.closeSync(fd) } catch (_) { /* fd 可能已关闭 */ }
      }
      this.safeDelete(newPath)
      throw e instanceof Error ? e : new Error(String(e))
    }

    // 2. 删旧 .bak（如果存在）
    this.safeDelete(bakPath)

    // 3. 目标存在 → rename 为 .bak
    try {
      if (fs.accessSync(filePath)) {
        fs.renameSync(filePath, bakPath)
      }
    } catch (e) {
      this.safeDelete(newPath)
      throw e instanceof Error ? e : new Error(String(e))
    }

    // 4. .new → 目标
    try {
      fs.renameSync(newPath, filePath)
    } catch (e) {
      // 步骤 4 失败，尝试恢复 .bak → 目标
      try {
        if (fs.accessSync(bakPath)) {
          fs.renameSync(bakPath, filePath)
        }
      } catch (_) { /* 恢复失败，尽力而为 */ }
      this.safeDelete(newPath)
      throw e instanceof Error ? e : new Error(String(e))
    }

    // 5. 成功后删 .bak
    this.safeDelete(bakPath)
  }

  // 安全删除文件 — 如果文件不存在，不抛异常。
  // 先 accessSync 检查文件是否存在，避免 native file_api 反复打印 ret -2。
  private safeDelete(filePath: string): void {
    try {
      if (fs.accessSync(filePath)) {
        fs.unlinkSync(filePath)
      }
    } catch (_) {
      // 文件不存在是正常情况
    }
  }

  // 复制 external_log 文件到 crashes 目录，复制成功后删除系统源文件。
  // 华为官方要求处理完 external_log 后及时删除，否则故障日志目录达到上限后
  // 后续新日志可能写不出来。删除失败只记 warn，不影响后续流程。
  private copyFile(srcPath: string, destDir: string, fileName: string): void {
    try {
      const destPath = `${destDir}/${fileName}`
      fs.copyFileSync(srcPath, destPath)
      hilog.info(0x0000, LOG_TAG, 'Copied external crash fault log into diagnostics storage')
      // 复制成功后删除系统源文件
      try {
        fs.unlinkSync(srcPath)
      } catch (_) {
        hilog.warn(0x0000, LOG_TAG, 'Unable to delete copied system crash fault log source')
      }
    } catch (e) {
      hilog.warn(0x0000, LOG_TAG, 'copyFile failed: %{public}s', e instanceof Error ? e.name : 'UNKNOWN_ERROR')
    }
  }

  // 读取文件内容为字符串（用于把 external_log 内容打包进 zip 附件）。
  private readFileContent(filePath: string): string {
    try {
      const fd = fs.openSync(filePath, fs.OpenMode.READ_ONLY)
      try {
        const stat = fs.statSync(filePath)
        const buf = new ArrayBuffer(stat.size)
        fs.readSync(fd.fd, buf)
        const textDecoder = new util.TextDecoder('utf-8')
        return textDecoder.decodeWithStream(new Uint8Array(buf))
      } finally {
        fs.closeSync(fd)
      }
    } catch (e) {
      hilog.warn(0x0000, LOG_TAG, 'readFileContent failed: %{public}s', e instanceof Error ? e.name : 'UNKNOWN_ERROR')
      return ''
    }
  }

  // 清理 eventKey 中的非法字符，确保可作为目录名/文件名使用。
  private sanitizeEventKey(key: string): string {
    return key.replace(/[^a-zA-Z0-9_-]/g, '_')
  }

  // 格式化时间戳为目录名格式：YYYYMMDD-HHMMSS。
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

  // 确保输出目录存在。
  private ensureOutputDir(outputDir: string): void {
    try {
      if (!fs.accessSync(outputDir)) {
        fs.mkdirSync(outputDir, true)
      }
    } catch (_) {
      // 目录创建失败不影响导出（Rust 层会自己创建）
    }
  }
}
