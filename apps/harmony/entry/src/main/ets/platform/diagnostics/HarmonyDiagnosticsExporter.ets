// HarmonyDiagnosticsExporter — HarmonyOS 诊断导出器。
//
// 负责：
// 1. 收集 HarmonyOS 平台附件（hilog / app_settings / sync_state / device_info / build_identity）
// 2. 调用共享 Rust exporter（通过 NativeDiagnosticsBridge.exportDiagnostics）生成 zip
// 3. 拉起 HarmonyOS 系统分享面板
//
// 附件内容在传给 NAPI 前必须 base64 编码；Rust 层解码后交给 writer_diagnostics::export。
// 脱敏由 Rust 层 redact 模块统一处理，ArkTS 层不做脱敏。
//
// 参考 Android 端 DiagnosticsExporterInterop.kt。

import { common } from '@kit.AbilityKit'
import { fileUri } from '@kit.CoreFileKit'
import hilog from '@ohos.hilog'
import util from '@ohos.util'
import fs from '@ohos.file.fs'
import bundleManager from '@ohos.bundle.bundleManager'

import { IWriterCoreBridge } from '../../corebridge/IWriterCoreBridge'
import { NativeCoreModule } from '../../corebridge/native/NativeCoreModule'
import { NativeDiagnosticsBridge } from '../../corebridge/native/NativeDiagnosticsBridge'
import { NativeWriterCoreBridge } from '../../corebridge/NativeWriterCoreBridge'
import { HarmonyShareService } from '../share/HarmonyShareService'
import { SharePayload } from '../share/SharePayload'
import { DeviceInfoSnapshot, getDeviceInfoSnapshot } from '../device/DeviceInfoSnapshot'
import { HarmonyDiagnosticsRuntimeSnapshot } from './HarmonyDiagnosticsRuntimeSnapshot'

// 附件 JSON 项格式 — 与 Rust AttachmentDto { relative_path: String, content: String } 对齐。
// content 是 base64 编码的二进制数据。
interface AttachmentItem {
  relative_path: string
  content: string
}

// app_settings_sanitized.json 附件 DTO。
interface AppSettingsAttachment {
  themeMode: string
  appearanceMode: string | null
  editorFontSize: number
  editorLineSpacingMultiplier: number
  autoSaveEnabled: boolean
  autoSaveDelayMs: number
  autoIndentEnabled: boolean
  autoIndentWidth: number
  editorTypingAnimationEnabled: boolean
  editorTypingAnimationDurationMs: number
  aiEnabled: boolean
  diagnosticsEnabled: boolean
  diagnosticsVerbose: boolean
}

// sync_state_sanitized.json 附件 DTO。
interface SyncStateAttachment {
  status: string | null
  lastSyncTime: number | null
  lastError: string | null
  conflictCount: number
}

// build_identity.json 附件 DTO。
interface BuildIdentityAttachment {
  appVersion: string
  buildKey: string
}

// Issue #773 评论 5845033983：ThemeRuntimeAttachment / UiRuntimeAttachment / EditorRuntimeAttachment
// 三个 interface 已删除，序列化逻辑统一移到 HarmonyDiagnosticsRuntimeSnapshot。
// 附件名常量保留，用于 makeAttachment 调用。

const LOG_TAG = 'HarmonyDiagnosticsExporter'
// 诊断目录名 — 位于 filesDir 下（非 cacheDir），因为 snapshots 子目录需要持久保留 latest.zip/previous.zip
const DIAGNOSTICS_DIR = 'diagnostics'

// 附件名常量 — 与 Android 端 DiagnosticsExporterInterop 保持一致。
const ATTACHMENT_HILOG = 'hilog.txt'
const ATTACHMENT_APP_SETTINGS = 'app_settings_sanitized.json'
const ATTACHMENT_SYNC_STATE = 'sync_state_sanitized.json'
const ATTACHMENT_CURRENT_DEVICE = 'current_device.json'
const ATTACHMENT_BUILD_IDENTITY = 'build_identity.json'
const ATTACHMENT_THEME_RUNTIME = 'theme_runtime_sanitized.json'
const ATTACHMENT_UI_RUNTIME = 'ui_runtime_sanitized.json'
const ATTACHMENT_EDITOR_RUNTIME = 'editor_runtime_sanitized.json'

export class HarmonyDiagnosticsExporter {
  private bridge: IWriterCoreBridge
  private diagnosticsBridge: NativeDiagnosticsBridge
  private abilityContext: common.UIAbilityContext | null = null

  constructor() {
    this.bridge = NativeWriterCoreBridge.getInstance()
    // NativeCoreModule 只是 NAPI 入口的薄封装，创建多个实例安全（都指向同一个 .so）。
    this.diagnosticsBridge = new NativeDiagnosticsBridge(new NativeCoreModule())
  }

  // 注入 UIAbilityContext — 由 AppContext 或 SettingsViewModel 调用。
  // 没有 context 时 export 仍可执行（只是无法拉起分享面板）。
  setAbilityContext(context: common.UIAbilityContext): void {
    this.abilityContext = context
  }

  // 导出诊断包 — 完整流程：flush → 收集附件 → export → 分享。
  // 返回 zip 文件路径；失败返回 null。
  // Issue #773 评论 5845033983：内部调用 exportTo(defaultDir)，复用统一逻辑。
  async export(): Promise<string | null> {
    if (this.abilityContext === null) {
      hilog.error(0x0000, LOG_TAG, 'abilityContext is null; cannot export')
      return null
    }
    const filesDir = this.abilityContext.filesDir
    const outputDir = `${filesDir}/${DIAGNOSTICS_DIR}/snapshots`
    return this.exportTo(outputDir)
  }

  // Issue #773 评论 5845033983：导出诊断包到指定目录。
  // 与 export() 类似，但接受 outputDir 参数，不使用默认的 snapshots 目录。
  // Checkpoint 调用此方法导出到 staging 目录，确认完整成功后再移动到 snapshots。
  async exportTo(outputDir: string): Promise<string | null> {
    try {
      // 1. flush 日志落盘
      const flushResult = this.diagnosticsBridge.flushDiagnostics()
      if (flushResult !== 0) {
        hilog.warn(0x0000, LOG_TAG, 'flushDiagnostics failed with code %{public}d; continuing export', flushResult)
      }

      // 2. 准备输出目录
      if (this.abilityContext === null) {
        hilog.error(0x0000, LOG_TAG, 'abilityContext is null; cannot export')
        return null
      }
      this.ensureOutputDir(outputDir)

      // 3. 收集平台附件
      const attachments = await this.collectAttachments()

      // 4. 调用 Rust exporter 生成 zip
      const attachmentsJson = JSON.stringify(attachments)
      const zipPath = this.diagnosticsBridge.exportDiagnostics(outputDir, attachmentsJson)

      // 检查返回值：成功是 zip 文件路径，失败是 JSON 错误 envelope
      if (zipPath === null || zipPath === undefined || zipPath.length === 0) {
        hilog.error(0x0000, LOG_TAG, 'exportDiagnostics returned empty path')
        return null
      }
      // 失败时 NAPI 返回 JSON 错误 envelope（以 { 开头）
      if (zipPath.startsWith('{')) {
        hilog.error(0x0000, LOG_TAG, 'exportDiagnostics returned error: %{public}s', zipPath)
        return null
      }

      hilog.info(0x0000, LOG_TAG, 'exportDiagnostics succeeded: %{public}s', zipPath)
      return zipPath
    } catch (e) {
      hilog.error(0x0000, LOG_TAG, 'exportTo failed: %{public}s', String(e))
      return null
    }
  }

  // 导出并分享 — 导出成功后拉起系统分享面板。
  // 返回 true 表示分享面板成功拉起；false 表示导出失败或分享失败。
  // Issue #774：把 zip copy 到 share 目录后再交给 ShareKit，避免 PERMISSION_DENIED。
  async exportAndShare(): Promise<boolean> {
    const zipPath = await this.export()
    if (zipPath === null) {
      return false
    }

    try {
      // 把 zip copy 到 share 目录
      const filesDir = this.abilityContext!.filesDir
      const shareDir = `${filesDir}/${DIAGNOSTICS_DIR}/share`
      this.ensureOutputDir(shareDir)

      const zipFileName = this.extractFileName(zipPath)
      const shareZipPath = `${shareDir}/${zipFileName}`

      // copy zip 到 share 目录
      fs.copyFileSync(zipPath, shareZipPath)

      // 验证 share copy 的 size
      const stat = fs.statSync(shareZipPath)
      hilog.info(0x0000, LOG_TAG, 'Share zip size: %{public}d bytes', stat.size)
      if (stat.size <= 0) {
        hilog.error(0x0000, LOG_TAG, 'Share zip size is 0 or negative; aborting share')
        return false
      }

      // 只把 share copy 的 URI 交给 ShareKit
      const shareService = HarmonyShareService.getInstance()
      const payload: SharePayload = {
        kind: 'file',
        title: '素笺诊断包',
        utd: 'general.zip-archive',
        uris: [fileUri.getUriFromPath(shareZipPath)]
      }
      const ok = await shareService.share(payload, 'system')
      if (!ok) {
        hilog.warn(0x0000, LOG_TAG, 'system share returned false; zip is still at %{public}s', shareZipPath)
      }
      return ok
    } catch (e) {
      hilog.error(0x0000, LOG_TAG, 'share failed: %{public}s', String(e))
      return false
    }
  }

  // ── 附件收集 ──────────────────────────────────────────────────

  // 收集所有平台附件，返回 base64 编码的 AttachmentItem 列表。
  // 每个附件收集失败时写入错误 JSON 附件，不让整个导出失败。
  private async collectAttachments(): Promise<AttachmentItem[]> {
    const attachments: AttachmentItem[] = []

    // hilog — 优先使用真实 HiLog snapshot（C++ 层 OH_LOG_SetCallback 环形缓冲），
    // fallback 到 writer_diagnostics 已落盘的日志文件。
    this.addHilogAttachment(attachments)

    // app_settings_sanitized.json
    await this.addAppSettingsAttachment(attachments)

    // sync_state_sanitized.json
    await this.addSyncStateAttachment(attachments)

    // current_device.json
    this.addCurrentDeviceAttachment(attachments)

    // build_identity.json
    this.addBuildIdentityAttachment(attachments)

    // theme_runtime_sanitized.json — Issue #767 主题运行时快照
    this.addThemeRuntimeAttachment(attachments)

    // ui_runtime_sanitized.json — Issue #767 UI 运行时快照
    this.addUiRuntimeAttachment(attachments)

    // editor_runtime_sanitized.json — Issue #768 编辑器运行时快照
    this.addEditorRuntimeAttachment(attachments)

    return attachments
  }

  // hilog.txt — 优先使用真实 HiLog snapshot（通过 NAPI 获取 C++ 层环形缓冲内容）。
  // 如果 snapshot 为空（callback 未注册或无日志），fallback 到 writer_diagnostics 日志文件。
  private addHilogAttachment(attachments: AttachmentItem[]): void {
    try {
      // 优先使用真实 HiLog snapshot
      const hilogContent = this.diagnosticsBridge.getHilogSnapshot()
      if (hilogContent && hilogContent.length > 0) {
        attachments.push(this.makeAttachment(ATTACHMENT_HILOG, hilogContent))
        return
      }
      // Fallback: writer_diagnostics 日志文件
      if (this.abilityContext === null) {
        attachments.push(this.errorAttachment(ATTACHMENT_HILOG, 'abilityContext is null'))
        return
      }
      const filesDir = this.abilityContext.filesDir
      const logDir = `${filesDir}/diagnostics/live/log`
      const logContent = this.readLogFiles(logDir)
      if (logContent.length > 0) {
        attachments.push(this.makeAttachment(ATTACHMENT_HILOG, logContent))
      } else {
        attachments.push(this.errorAttachment(ATTACHMENT_HILOG, 'no log files found'))
      }
    } catch (e) {
      attachments.push(this.errorAttachment(ATTACHMENT_HILOG, String(e)))
    }
  }

  // app_settings_sanitized.json — 应用设置（脱敏由 Rust 层处理）。
  private async addAppSettingsAttachment(attachments: AttachmentItem[]): Promise<void> {
    try {
      const result = await this.bridge.getLocalSettings()
      if (result.success && result.data !== undefined && result.data !== null) {
        const settings = result.data
        // 只采集非敏感字段（与 Android 端一致）
        const sanitized: AppSettingsAttachment = {
          themeMode: settings.themeMode ?? 'system',
          appearanceMode: settings.appearanceMode ?? null,
          editorFontSize: settings.editorFontSize,
          editorLineSpacingMultiplier: settings.editorLineSpacingMultiplier,
          autoSaveEnabled: settings.autoSaveEnabled,
          autoSaveDelayMs: settings.autoSaveDelayMs,
          autoIndentEnabled: settings.autoIndentEnabled,
          autoIndentWidth: settings.autoIndentWidth,
          editorTypingAnimationEnabled: settings.editorTypingAnimationEnabled,
          editorTypingAnimationDurationMs: settings.editorTypingAnimationDurationMs,
          aiEnabled: settings.aiEnabled,
          diagnosticsEnabled: settings.diagnosticsEnabled,
          diagnosticsVerbose: settings.diagnosticsVerbose
        }
        const json = JSON.stringify(sanitized, null, 2)
        attachments.push(this.makeAttachment(ATTACHMENT_APP_SETTINGS, json))
      } else {
        attachments.push(this.errorAttachment(ATTACHMENT_APP_SETTINGS,
          result.errorCode ?? 'getLocalSettings failed'))
      }
    } catch (e) {
      attachments.push(this.errorAttachment(ATTACHMENT_APP_SETTINGS, String(e)))
    }
  }

  // sync_state_sanitized.json — 同步状态（脱敏由 Rust 层处理）。
  private async addSyncStateAttachment(attachments: AttachmentItem[]): Promise<void> {
    try {
      const result = await this.bridge.loadAppSyncState()
      if (result.success && result.data !== undefined && result.data !== null) {
        const state = result.data
        const sanitized: SyncStateAttachment = {
          status: state.status ?? null,
          lastSyncTime: state.lastSyncTime ?? null,
          lastError: state.lastError ?? null,
          conflictCount: state.conflicts?.length ?? 0
        }
        const json = JSON.stringify(sanitized, null, 2)
        attachments.push(this.makeAttachment(ATTACHMENT_SYNC_STATE, json))
      } else {
        attachments.push(this.errorAttachment(ATTACHMENT_SYNC_STATE,
          result.errorCode ?? 'loadAppSyncState failed'))
      }
    } catch (e) {
      attachments.push(this.errorAttachment(ATTACHMENT_SYNC_STATE, String(e)))
    }
  }

  // current_device.json — 当前设备信息。
  private addCurrentDeviceAttachment(attachments: AttachmentItem[]): void {
    try {
      const info: DeviceInfoSnapshot = getDeviceInfoSnapshot()
      const json = JSON.stringify(info, null, 2)
      attachments.push(this.makeAttachment(ATTACHMENT_CURRENT_DEVICE, json))
    } catch (e) {
      attachments.push(this.errorAttachment(ATTACHMENT_CURRENT_DEVICE, String(e)))
    }
  }

  // build_identity.json — 构建标识。
  private addBuildIdentityAttachment(attachments: AttachmentItem[]): void {
    try {
      const identity = this.getBuildIdentity()
      const json = JSON.stringify(identity, null, 2)
      attachments.push(this.makeAttachment(ATTACHMENT_BUILD_IDENTITY, json))
    } catch (e) {
      attachments.push(this.errorAttachment(ATTACHMENT_BUILD_IDENTITY, String(e)))
    }
  }

  // theme_runtime_sanitized.json — 主题运行时快照（Issue #767）。
  // Issue #773 评论 5845033983：从 HarmonyDiagnosticsRuntimeSnapshot 获取 JSON，不再保留自己的序列化逻辑。
  private addThemeRuntimeAttachment(attachments: AttachmentItem[]): void {
    try {
      const json = HarmonyDiagnosticsRuntimeSnapshot.getInstance().getThemeRuntimeJson()
      attachments.push(this.makeAttachment(ATTACHMENT_THEME_RUNTIME, json))
    } catch (e) {
      attachments.push(this.errorAttachment(ATTACHMENT_THEME_RUNTIME, String(e)))
    }
  }

  // ui_runtime_sanitized.json — UI 运行时快照（Issue #767）。
  // Issue #773 评论 5845033983：从 HarmonyDiagnosticsRuntimeSnapshot 获取 JSON，不再保留自己的序列化逻辑。
  private addUiRuntimeAttachment(attachments: AttachmentItem[]): void {
    try {
      const json = HarmonyDiagnosticsRuntimeSnapshot.getInstance().getUiRuntimeJson()
      attachments.push(this.makeAttachment(ATTACHMENT_UI_RUNTIME, json))
    } catch (e) {
      attachments.push(this.errorAttachment(ATTACHMENT_UI_RUNTIME, String(e)))
    }
  }

  // editor_runtime_sanitized.json — 编辑器运行时快照（Issue #768）。
  // Issue #773 评论 5845033983：从 HarmonyDiagnosticsRuntimeSnapshot 获取 JSON，不再保留自己的序列化逻辑。
  private addEditorRuntimeAttachment(attachments: AttachmentItem[]): void {
    try {
      const json = HarmonyDiagnosticsRuntimeSnapshot.getInstance().getEditorRuntimeJson()
      attachments.push(this.makeAttachment(ATTACHMENT_EDITOR_RUNTIME, json))
    } catch (e) {
      attachments.push(this.errorAttachment(ATTACHMENT_EDITOR_RUNTIME, String(e)))
    }
  }

  // ── 工具方法 ──────────────────────────────────────────────────

  // 读取 logDir 下所有 .log 文件内容，合并为一个字符串。
  private readLogFiles(logDir: string): string {
    try {
      if (!fs.accessSync(logDir)) {
        return ''
      }
      const entries = fs.listFileSync(logDir)
      let combined = ''
      for (const entry of entries) {
        if (entry.endsWith('.log')) {
          const filePath = `${logDir}/${entry}`
          try {
            const fd = fs.openSync(filePath, fs.OpenMode.READ_ONLY)
            try {
              const stat = fs.statSync(filePath)
              const buf = new ArrayBuffer(stat.size)
              fs.readSync(fd.fd, buf)
              const textDecoder = new util.TextDecoder('utf-8')
              const content = textDecoder.decodeWithStream(new Uint8Array(buf))
              combined += `=== ${entry} ===\n${content}\n`
            } finally {
              fs.closeSync(fd)
            }
          } catch (_) {
            // 单个文件读取失败不影响其他文件
          }
        }
      }
      return combined
    } catch (_) {
      return ''
    }
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

  // 构造错误附件 — 收集失败时写入合法 JSON 附件，不让整个导出失败。
  private errorAttachment(relativePath: string, errorMsg: string): AttachmentItem {
    const errorJson = JSON.stringify({ 'error': errorMsg }, null, 2)
    return this.makeAttachment(relativePath, errorJson)
  }

  // 获取构建身份 — 与 EntryAbility.getBuildIdentity() 逻辑一致。
  private getBuildIdentity(): BuildIdentityAttachment {
    try {
      const info = bundleManager.getBundleInfoForSelfSync(
        bundleManager.BundleFlag.GET_BUNDLE_INFO_DEFAULT
      )
      return {
        appVersion: info.versionName,
        buildKey: `harmony-v${info.versionCode}-${info.versionName}`
      }
    } catch (_) {
      return {
        appVersion: 'unknown',
        buildKey: 'harmony-unknown'
      }
    }
  }

  // 确保输出目录存在。
  // 不清空旧 zip 文件，因为 snapshots 目录需要保留 latest.zip/previous.zip。
  private ensureOutputDir(outputDir: string): void {
    try {
      if (!fs.accessSync(outputDir)) {
        fs.mkdirSync(outputDir, true)
      }
    } catch (_) {
      // 目录创建失败不影响导出（Rust 层会自己创建）
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
}
