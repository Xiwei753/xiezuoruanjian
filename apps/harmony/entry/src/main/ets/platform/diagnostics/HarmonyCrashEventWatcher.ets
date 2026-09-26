// HarmonyCrashEventWatcher — 系统崩溃事件观察器。
//
// 负责：
// 1. 在应用启动时订阅 HiAppEvent 的 APP_CRASH 和 APP_KILLED 事件
// 2. 系统在下一次启动回放上一轮故障事件时，把 params 和 external_log 复制进
//    diagnostics/crashes/<timestamp>/ 目录
// 3. 额外生成 sujian-diagnostics-crash-<timestamp>.zip 崩溃补全包到 snapshots 目录
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

const LOG_TAG = 'HarmonyCrashEventWatcher'
const DIAGNOSTICS_DIR = 'diagnostics'

// 附件 JSON 项格式 — 与 Rust AttachmentDto { relative_path: String, content: String } 对齐。
// content 是 base64 编码的二进制数据。
interface AttachmentItem {
  relative_path: string
  content: string
}

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
  // 1. 创建 crashes/<timestamp>/ 目录
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

      const crashesDir = `${this.context!.filesDir}/${DIAGNOSTICS_DIR}/crashes/${timestamp}`
      fs.mkdirSync(crashesDir, true)

      // 写 params.json — 把整个 event 信息序列化保存
      // ArkTS 严格模式要求对象字面量有显式 interface 类型（arkts-no-untyped-obj-literals）。
      const crashRecord: CrashRecord = {
        domain: event.domain,
        name: event.name,
        eventType: event.eventType,
        params: event.params as Record<string, Object>
      }
      const paramsJson = JSON.stringify(crashRecord, null, 2)
      this.writeAtomic(`${crashesDir}/params.json`, paramsJson)

      // 复制 external_log 文件到 crashes 目录
      // APP_KILLED 事件不保证有 external_log（官方样例只有 reason/time/foreground 等字段），
      // 没有 external_log 是正常情况，用 info 级别记录，不当失败处理。
      if (externalLog.length > 0) {
        for (let i = 0; i < externalLog.length; i++) {
          this.copyFile(externalLog[i], crashesDir)
        }
      } else {
        hilog.info(0x0000, LOG_TAG, 'No external_log in event %{public}s (normal for APP_KILLED)', event.name)
      }

      // 生成崩溃补全包 zip
      this.generateCrashZip(timestamp, crashRecord, externalLog)
    } catch (e) {
      hilog.error(0x0000, LOG_TAG, 'handleCrashEvent failed: %{public}s', String(e))
    }
  }

  // 生成崩溃补全包 zip — flush → export → 重命名为 crash 前缀。
  // 崩溃补全包不覆盖 latest.zip / previous.zip，使用独立的 crash 前缀文件名。
  private generateCrashZip(
    timestamp: string,
    crashRecord: CrashRecord,
    externalLog: string[]
  ): void {
    try {
      // 1. flush 日志落盘
      const flushResult = this.diagnosticsBridge.flushDiagnostics()
      if (flushResult !== 0) {
        hilog.warn(0x0000, LOG_TAG, 'flushDiagnostics failed with code %{public}d; continuing crash zip', flushResult)
      }

      // 2. 准备输出目录
      const outputDir = `${this.context!.filesDir}/${DIAGNOSTICS_DIR}/snapshots`
      this.ensureOutputDir(outputDir)

      // 3. 把 crash 事件信息作为附件传给 Rust exporter
      const attachments: AttachmentItem[] = []
      // params.json 附件
      const paramsJson = JSON.stringify(crashRecord, null, 2)
      attachments.push(this.makeAttachment('crashes/' + timestamp + '/params.json', paramsJson))

      // external_log 内容附件 — 从已复制到 diagnostics 的持久副本读取
      // （原始系统路径的 external_log 在 copyFile 中已被删除，不能再依赖）
      const crashesDir = `${this.context!.filesDir}/${DIAGNOSTICS_DIR}/crashes/${timestamp}`
      for (let i = 0; i < externalLog.length; i++) {
        const fileName = this.extractFileName(externalLog[i])
        const persistedPath = `${crashesDir}/${fileName}`
        const logContent = this.readFileContent(persistedPath)
        if (logContent.length > 0) {
          attachments.push(this.makeAttachment('crashes/' + timestamp + '/' + fileName, logContent))
        }
      }

      // 4. 调用 Rust exporter 生成 zip
      const attachmentsJson = JSON.stringify(attachments)
      const zipPath = this.diagnosticsBridge.exportDiagnostics(outputDir, attachmentsJson)

      if (zipPath === null || zipPath === undefined || zipPath.length === 0) {
        hilog.error(0x0000, LOG_TAG, 'exportDiagnostics returned empty path for crash zip')
        return
      }
      // 失败时 NAPI 返回 JSON 错误 envelope（以 { 开头）
      if (zipPath.startsWith('{')) {
        hilog.error(0x0000, LOG_TAG, 'exportDiagnostics returned error for crash zip: %{public}s', zipPath)
        return
      }

      // 5. 重命名为 crash 前缀文件名
      // Rust 层生成的文件名格式是 sujian-diagnostics-{rust_timestamp}.zip
      // 我们需要重命名为 sujian-diagnostics-crash-{our_timestamp}.zip
      const crashZipName = `sujian-diagnostics-crash-${timestamp}.zip`
      const crashZipPath = `${outputDir}/${crashZipName}`
      try {
        // 如果目标文件已存在（同一时间戳的崩溃事件），先删除
        if (fs.accessSync(crashZipPath)) {
          fs.unlinkSync(crashZipPath)
        }
        fs.renameSync(zipPath, crashZipPath)
        hilog.info(0x0000, LOG_TAG, 'Crash zip generated: %{public}s', crashZipPath)
      } catch (renameErr) {
        // rename 失败时尝试 copy + delete
        try {
          fs.copyFileSync(zipPath, crashZipPath)
          fs.unlinkSync(zipPath)
          hilog.info(0x0000, LOG_TAG, 'Crash zip generated (via copy): %{public}s', crashZipPath)
        } catch (copyErr) {
          hilog.error(0x0000, LOG_TAG, 'Failed to rename/copy crash zip: %{public}s', String(copyErr))
        }
      }
    } catch (e) {
      hilog.error(0x0000, LOG_TAG, 'generateCrashZip failed: %{public}s', String(e))
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
  private safeDelete(filePath: string): void {
    try {
      fs.unlinkSync(filePath)
    } catch (_) {
      // 文件不存在是正常情况
    }
  }

  // 复制 external_log 文件到 crashes 目录，复制成功后删除系统源文件。
  // 华为官方要求处理完 external_log 后及时删除，否则故障日志目录达到上限后
  // 后续新日志可能写不出来。删除失败只记 warn，不影响后续流程。
  private copyFile(srcPath: string, destDir: string): void {
    try {
      const fileName = this.extractFileName(srcPath)
      const destPath = `${destDir}/${fileName}`
      fs.copyFileSync(srcPath, destPath)
      hilog.info(0x0000, LOG_TAG, 'Copied external log: %{public}s → %{public}s', srcPath, destPath)
      // 复制成功后删除系统源文件
      try {
        fs.unlinkSync(srcPath)
        hilog.info(0x0000, LOG_TAG, 'Deleted system external log source: %{public}s', srcPath)
      } catch (unlinkErr) {
        hilog.warn(0x0000, LOG_TAG, 'Failed to delete system external log source %{public}s: %{public}s',
          srcPath, String(unlinkErr))
      }
    } catch (e) {
      hilog.warn(0x0000, LOG_TAG, 'copyFile failed for %{public}s: %{public}s', srcPath, String(e))
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
      hilog.warn(0x0000, LOG_TAG, 'readFileContent failed for %{public}s: %{public}s', filePath, String(e))
      return ''
    }
  }

  // 从文件路径中提取文件名。
  private extractFileName(filePath: string): string {
    const lastSlash = filePath.lastIndexOf('/')
    if (lastSlash >= 0 && lastSlash < filePath.length - 1) {
      return filePath.substring(lastSlash + 1)
    }
    return filePath
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

  // 构造附件项 — content 为 base64 编码的 UTF-8 字符串。
  private makeAttachment(relativePath: string, textContent: string): AttachmentItem {
    const textEncoder = new util.TextEncoder('utf-8')
    const encoded: Uint8Array = textEncoder.encodeInto(textContent)
    const base64Helper = new util.Base64Helper()
    const base64 = base64Helper.encodeToStringSync(encoded)
    return {
      relative_path: relativePath,
      content: base64
    }
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
