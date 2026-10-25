// HarmonyDiagnosticsExporter — HarmonyOS 诊断导出器。
//
// 负责：
// 1. 收集 HarmonyOS 平台附件（hilog / app_settings / sync_state / device_info / build_identity）
// 2. 将白名单附件和脱敏后的持久日志写入 staging 树，再由 ArkTS zlib 生成 zip
// 3. 拉起 HarmonyOS 系统分享面板
//
// 导出不能依赖 writer_core NAPI；Native 可用时仅尽力 flush / 读取 HiLog 快照。
// 导入 staging 的文本与 JSON 均由 HarmonyDiagnosticsArchive 做脱敏。
//
// 参考 Android 端 DiagnosticsExporterInterop.kt。

import { common } from '@kit.AbilityKit'
import { fileUri, picker } from '@kit.CoreFileKit'
import hilog from '@ohos.hilog'
import util from '@ohos.util'
import fs from '@ohos.file.fs'
import bundleManager from '@ohos.bundle.bundleManager'

import { IWriterCoreBridge } from '../../corebridge/IWriterCoreBridge'
import { FullSyncReport, FullSyncReportFailure, FullSyncState } from '../../corebridge/dto/SyncDtos'
import { NativeCoreModule } from '../../corebridge/native/NativeCoreModule'
import { NativeDiagnosticsBridge } from '../../corebridge/native/NativeDiagnosticsBridge'
import { NativeSyncBridge } from '../../corebridge/native/NativeSyncBridge'
import { NativeWriterCoreBridge } from '../../corebridge/NativeWriterCoreBridge'
import { SyncDisplayPhase, SyncEntrySource, SyncRunPresentationState } from '../../feature/settings/presentation/SyncRunPresentationState'
import { HarmonyShareService } from '../share/HarmonyShareService'
import { SharePayload } from '../share/SharePayload'
import { DeviceInfoSnapshot, getDeviceInfoSnapshot } from '../device/DeviceInfoSnapshot'
import { HarmonyDiagnosticsRuntimeSnapshot } from './HarmonyDiagnosticsRuntimeSnapshot'
import { HarmonyDiagnosticsArchive } from './HarmonyDiagnosticsArchive'

// 已整理的 UTF-8 诊断附件。relative_path 必须命中本文件的显式白名单。
interface AttachmentItem {
  relative_path: string
  content: string
}

interface LogCollectionSummary {
  status: string
  file_count: number
  files: string[]
}

interface DiagnosticsCollectionSummary {
  logs: LogCollectionSummary
  attachments: string[]
}

interface DiagnosticsManifest {
  schema_version: number
  platform: string
  build_identity: string
  exported_at: string
  collection: DiagnosticsCollectionSummary
}

// Issue #804：诊断导出保存结果 — 区分成功 / 用户取消 / 真失败，让 UI 能给出明确反馈。
export interface DiagnosticsSaveResult {
  status: 'success' | 'cancelled' | 'failed'
  fileName?: string
  uri?: string
  error?: string
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

// sync_state_sanitized.json — App target 局部状态，不代表全量同步。
interface SyncStateAttachment {
  scope: 'app_target_local'
  status: string | null
  lastSyncTime: number | null
  lastError: string | null
  conflictCount: number
}

// full_sync_state_sanitized.json 附件 DTO。Core 从未持久化时，各值为空。
interface FullSyncStateAttachment {
  overall_status: string | null
  last_attempt_time: number | null
  last_success_time: number | null
  failed_targets: string[]
}

// last_sync_report_sanitized.json — project only the safe Core report fields.
interface FullSyncReportAttachment {
  attempted_at: number
  completed_groups: number
  failed_groups: number
  pending_groups: number
  failures: FullSyncReportFailure[]
}

// sync_operation_runtime.json — 进程内同步运行快照的安全展示字段。
interface SyncOperationRuntimeAttachment {
  phase: SyncDisplayPhase
  source: SyncEntrySource
  startedAtMs: number | null
  finishedAtMs: number | null
  errorCode: string | null
  overallStatus: string | null
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
const ATTACHMENT_HILOG_CAPTURE_STATS = 'hilog_capture_stats.json'
const ATTACHMENT_APP_SETTINGS = 'app_settings_sanitized.json'
const ATTACHMENT_SYNC_STATE = 'sync_state_sanitized.json'
const ATTACHMENT_FULL_SYNC_STATE = 'full_sync_state_sanitized.json'
const ATTACHMENT_LATEST_FULL_SYNC_REPORT = 'last_sync_report_sanitized.json'
const ATTACHMENT_SYNC_OPERATION_RUNTIME = 'sync_operation_runtime.json'
const ATTACHMENT_CURRENT_DEVICE = 'current_device.json'
const ATTACHMENT_BUILD_IDENTITY = 'build_identity.json'
const ATTACHMENT_THEME_RUNTIME = 'theme_runtime_sanitized.json'
const ATTACHMENT_UI_RUNTIME = 'ui_runtime_sanitized.json'
const ATTACHMENT_EDITOR_RUNTIME = 'editor_runtime_sanitized.json'

export class HarmonyDiagnosticsExporter {
  private bridge: IWriterCoreBridge
  private diagnosticsBridge: NativeDiagnosticsBridge
  private syncBridge: NativeSyncBridge
  private abilityContext: common.UIAbilityContext | null = null

  constructor() {
    this.bridge = NativeWriterCoreBridge.getInstance()
    // NativeCoreModule 只是 NAPI 入口的薄封装，可供不同领域 Bridge 复用。
    const coreModule = new NativeCoreModule()
    this.diagnosticsBridge = new NativeDiagnosticsBridge(coreModule)
    this.syncBridge = new NativeSyncBridge(coreModule)
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
    let stagingDir: string = ''
    try {
      // 1. 准备输出目录
      if (this.abilityContext === null) {
        hilog.error(0x0000, LOG_TAG, 'abilityContext is null; cannot export')
        return null
      }
      this.ensureOutputDir(outputDir)

      // Flush is only a best-effort log barrier. Its failure cannot block the
      // ArkTS archive path when the writer native module is unavailable.
      try {
        const flushResult = this.diagnosticsBridge.flushDiagnostics()
        if (flushResult !== 0) {
          hilog.warn(0x0000, LOG_TAG, 'flushDiagnostics returned %{public}d; archiving persisted files', flushResult)
        }
      } catch (_) {
        hilog.info(0x0000, LOG_TAG, 'native diagnostics flush unavailable; archiving persisted files')
      }

      // 2. 收集平台附件
      const attachments = await this.collectAttachments()

      // 3. 只把固定附件白名单和 writer 持久日志放进沙箱 staging 树。
      stagingDir = `${outputDir}/.archive-${Date.now()}-${Math.floor(Math.random() * 1000000)}`
      this.ensureOutputDir(stagingDir)
      const logSummary = this.copyPersistedLogs(stagingDir)
      const attachmentNames: string[] = []
      for (const attachment of attachments) {
        if (!this.isWhitelistedAttachmentPath(attachment.relative_path)) {
          hilog.warn(0x0000, LOG_TAG, 'Skipped non-whitelisted attachment')
          continue
        }
        HarmonyDiagnosticsArchive.writeTextFile(stagingDir, attachment.relative_path, attachment.content)
        attachmentNames.push(attachment.relative_path)
      }

      const manifest: DiagnosticsManifest = {
        schema_version: 1,
        platform: 'harmony',
        build_identity: this.getBuildIdentity().buildKey,
        exported_at: new Date().toISOString(),
        collection: {
          logs: logSummary,
          attachments: attachmentNames
        }
      }
      HarmonyDiagnosticsArchive.writeTextFile(
        stagingDir, 'diagnostics_manifest.json', JSON.stringify(manifest, null, 2)
      )

      // 4. NativeDiagnosticsBridge.exportDiagnostics is deliberately not involved.
      const zipPath = await HarmonyDiagnosticsArchive.compressDirectory(
        stagingDir, outputDir, 'sujian-diagnostics'
      )
      hilog.info(0x0000, LOG_TAG, 'ArkTS diagnostics archive generated')
      return zipPath
    } catch (e) {
      hilog.error(0x0000, LOG_TAG, 'exportTo failed: %{public}s', e instanceof Error ? e.name : 'UNKNOWN_ERROR')
      return null
    } finally {
      if (stagingDir.length > 0) {
        HarmonyDiagnosticsArchive.removeTree(stagingDir)
      }
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

      // 固定分享文件名：share 目录只保留当前待分享的一份，不做历史仓库。
      // snapshots/ 负责内部历史，share/ 永远只有一份。
      const shareZipPath = `${shareDir}/sujian-diagnostics-share.zip`

      // 旧文件存在先删，避免堆积重复 zip
      if (fs.accessSync(shareZipPath)) {
        fs.unlinkSync(shareZipPath)
      }
      fs.copyFileSync(zipPath, shareZipPath)

      // 记录 share copy 的 size（仅日志，不拦截）
      const stat = fs.statSync(shareZipPath)
      hilog.info(0x0000, LOG_TAG, 'Share zip size: %{public}d bytes', stat.size)

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

  // 导出并保存到用户文件 — 使用 DocumentViewPicker.save() 让用户选择保存位置。
  // Issue #804：返回 DiagnosticsSaveResult 区分成功 / 用户取消 / 真失败，让 UI 能给出明确反馈。
  // 与 exportAndShare() 的区别：exportAndShare 拉起系统分享面板，exportAndSaveToUserFile 让用户选择本地保存位置。
  async exportAndSaveToUserFile(): Promise<DiagnosticsSaveResult> {
    const zipPath = await this.export()
    if (zipPath === null || this.abilityContext === null) {
      return { status: 'failed', error: '导出诊断包失败' }
    }

    try {
      const options = new picker.DocumentSaveOptions()
      const fileName = this.buildExportFileName()
      options.newFileNames = [fileName]

      const documentPicker = new picker.DocumentViewPicker(this.abilityContext)
      const uris: string[] = await documentPicker.save(options)
      if (uris.length === 0) {
        return { status: 'cancelled' }
      }

      const src = fs.openSync(zipPath, fs.OpenMode.READ_ONLY)
      const dst = fs.openSync(uris[0], fs.OpenMode.READ_WRITE)
      try {
        const sourceSize = fs.statSync(src.fd).size
        if (sourceSize <= 0) {
          throw new Error('diagnostics source zip is empty')
        }

        const data = new ArrayBuffer(sourceSize)
        const readSize = fs.readSync(src.fd, data)
        if (readSize !== sourceSize) {
          throw new Error(`diagnostics read incomplete: ${readSize}/${sourceSize}`)
        }

        const writeSize = fs.writeSync(dst.fd, data)
        if (writeSize !== readSize) {
          throw new Error(`diagnostics write incomplete: ${writeSize}/${readSize}`)
        }
      } finally {
        fs.closeSync(src)
        fs.closeSync(dst)
      }
      return { status: 'success', fileName: fileName, uri: uris[0] }
    } catch (e) {
      hilog.error(0x0000, LOG_TAG, 'save diagnostics failed: %{public}s', String(e))
      return { status: 'failed', error: String(e) }
    }
  }

  // 用真实时间生成诊断包文件名，格式：sujian-diagnostics-YYYYMMDD-HHmm.zip
  private buildExportFileName(): string {
    const now = new Date()
    const year = now.getFullYear()
    const month = (now.getMonth() + 1).toString().padStart(2, '0')
    const day = now.getDate().toString().padStart(2, '0')
    const hours = now.getHours().toString().padStart(2, '0')
    const minutes = now.getMinutes().toString().padStart(2, '0')
    return `sujian-diagnostics-${year}${month}${day}-${hours}${minutes}.zip`
  }

  // ── 附件收集 ──────────────────────────────────────────────────

  // 收集所有已整理的 UTF-8 平台附件。
  // 每个附件收集失败时写入错误 JSON 附件，不让整个导出失败。
  private async collectAttachments(): Promise<AttachmentItem[]> {
    const attachments: AttachmentItem[] = []

    // hilog — 真实 HiLog snapshot（C++ 层 OH_LOG_SetCallback 环形缓冲）。
    // 快照为空时不伪造内容，原因见 hilog_capture_stats.json。
    this.addHilogAttachment(attachments)

    // hilog_capture_stats.json — HiLog 采集统计，解释 hilog.txt 是否为空。
    this.addHilogCaptureStatsAttachment(attachments)

    // Capture the process-local run state before any asynchronous attachment reads.
    this.addSyncOperationRuntimeAttachment(attachments)

    // app_settings_sanitized.json
    await this.addAppSettingsAttachment(attachments)

    // sync_state_sanitized.json
    await this.addSyncStateAttachment(attachments)

    // full_sync_state_sanitized.json — the last persisted whole-sync result.
    await this.addFullSyncStateAttachment(attachments)

    // last_sync_report_sanitized.json — per-group failures from the last whole-sync run.
    await this.addLatestFullSyncReportAttachment(attachments)

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

    // crash fault stack — Issue #802
    await this.addLatestCrashAttachments(attachments)

    return attachments
  }

  // hilog.txt — 仅使用 NAPI 层 HiLog callback 的环形缓冲快照（OH_LOG_SetCallback）。
  // 快照为空时不写 hilog.txt、也不伪造内容：hilog_capture_stats.json 会说明原因
  // （registered / capturedRecords）。不再回退到 writer_diagnostics 的 Rust 日志文件。
  private addHilogAttachment(attachments: AttachmentItem[]): void {
    try {
      const hilogContent = this.diagnosticsBridge.getHilogSnapshot()
      if (hilogContent && hilogContent.length > 0) {
        attachments.push(this.makeAttachment(ATTACHMENT_HILOG, hilogContent))
      }
    } catch (e) {
      attachments.push(this.errorAttachment(ATTACHMENT_HILOG, String(e)))
    }
  }

  // hilog_capture_stats.json — HiLog 采集统计（registered/capturedRecords/
  // overwrittenRecords/truncatedRecords/capturedStartMs/capturedEndMs）。
  // 与 hilog.txt 成对出现：说明快照为空是 callback 未注册、确实没有日志，还是已被 clear。
  private addHilogCaptureStatsAttachment(attachments: AttachmentItem[]): void {
    try {
      const statsJson = this.diagnosticsBridge.getHilogCaptureStats()
      if (statsJson && statsJson.length > 0) {
        attachments.push(this.makeAttachment(ATTACHMENT_HILOG_CAPTURE_STATS, statsJson))
      } else {
        attachments.push(this.errorAttachment(ATTACHMENT_HILOG_CAPTURE_STATS,
          'empty hilog capture stats'))
      }
    } catch (e) {
      attachments.push(this.errorAttachment(ATTACHMENT_HILOG_CAPTURE_STATS, String(e)))
    }
  }

  // app_settings_sanitized.json — 应用设置仅采集非敏感字段，写入 staging 时再统一脱敏。
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

  // sync_state_sanitized.json — App target 局部状态，写入 staging 时再统一脱敏。
  private async addSyncStateAttachment(attachments: AttachmentItem[]): Promise<void> {
    try {
      const result = await this.bridge.loadAppSyncState()
      if (result.success && result.data !== undefined && result.data !== null) {
        const state = result.data
        const sanitized: SyncStateAttachment = {
          scope: 'app_target_local',
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

  // full_sync_state_sanitized.json — 仅导出 Core 持久化的整轮同步终态字段。
  private async addFullSyncStateAttachment(attachments: AttachmentItem[]): Promise<void> {
    try {
      const result = await this.syncBridge.loadFullSyncState()
      if (!result.success) {
        attachments.push(this.errorAttachment(ATTACHMENT_FULL_SYNC_STATE,
          result.errorCode ?? 'loadFullSyncState failed'))
        return
      }

      const state: FullSyncState | null = result.data ?? null
      const sanitized: FullSyncStateAttachment = {
        overall_status: state?.overall_status ?? null,
        last_attempt_time: state?.last_attempt_time ?? null,
        last_success_time: state?.last_success_time ?? null,
        failed_targets: state?.failed_targets ?? []
      }
      attachments.push(this.makeAttachment(
        ATTACHMENT_FULL_SYNC_STATE,
        JSON.stringify(sanitized, null, 2)
      ))
    } catch (e) {
      attachments.push(this.errorAttachment(ATTACHMENT_FULL_SYNC_STATE,
        e instanceof Error ? e.name : 'UNKNOWN_ERROR'))
    }
  }

  // Core has already sanitized report paths/details. Project only the report DTO fields,
  // excluding any future raw error or credential fields from the diagnostic archive.
  private async addLatestFullSyncReportAttachment(attachments: AttachmentItem[]): Promise<void> {
    try {
      const result = await this.syncBridge.loadLatestFullSyncReport()
      if (!result.success) {
        attachments.push(this.errorAttachment(ATTACHMENT_LATEST_FULL_SYNC_REPORT,
          result.errorCode ?? 'loadLatestFullSyncReport failed'))
        return
      }

      const report: FullSyncReport | null = result.data ?? null
      const sanitized: FullSyncReportAttachment | null = report === null ? null : {
        attempted_at: report.attempted_at,
        completed_groups: report.completed_groups,
        failed_groups: report.failed_groups,
        pending_groups: report.pending_groups,
        failures: report.failures.map((failure: FullSyncReportFailure): FullSyncReportFailure => {
          const item: FullSyncReportFailure = {
            target_kind: failure.target_kind,
            group_id: failure.group_id,
            path: failure.path,
            phase: failure.phase,
            error_category: failure.error_category,
            message_key: failure.message_key,
            detail: failure.detail,
            occurred_at: failure.occurred_at
          }
          return item
        })
      }
      attachments.push(this.makeAttachment(
        ATTACHMENT_LATEST_FULL_SYNC_REPORT,
        JSON.stringify(sanitized, null, 2)
      ))
    } catch (e) {
      attachments.push(this.errorAttachment(ATTACHMENT_LATEST_FULL_SYNC_REPORT,
        e instanceof Error ? e.name : 'UNKNOWN_ERROR'))
    }
  }

  // sync_operation_runtime.json — export only safe display state, never rawError or credentials.
  private addSyncOperationRuntimeAttachment(attachments: AttachmentItem[]): void {
    try {
      const snapshot = SyncRunPresentationState.getInstance().snapshot()
      const runtime: SyncOperationRuntimeAttachment = {
        phase: snapshot.phase,
        source: snapshot.source,
        startedAtMs: snapshot.startedAtMs,
        finishedAtMs: snapshot.finishedAtMs,
        errorCode: snapshot.outcome?.errorCode ?? null,
        overallStatus: snapshot.outcome?.overallStatus ?? null
      }
      attachments.push(this.makeAttachment(
        ATTACHMENT_SYNC_OPERATION_RUNTIME,
        JSON.stringify(runtime, null, 2)
      ))
    } catch (e) {
      attachments.push(this.errorAttachment(ATTACHMENT_SYNC_OPERATION_RUNTIME,
        e instanceof Error ? e.name : 'UNKNOWN_ERROR'))
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

  // crash fault stack — Issue #802
  // 从 filesDir/diagnostics/crashes/ 目录找最近一次 crash 事件目录，
  // 把 params.json 和已保存的 external crash log 装入诊断包。
  // 如果 crashes 目录不存在或没有事件目录，静默跳过（不报错）。
  private async addLatestCrashAttachments(attachments: AttachmentItem[]): Promise<void> {
    try {
      if (this.abilityContext === null) {
        return
      }
      const filesDir = this.abilityContext.filesDir
      const crashesDir = `${filesDir}/${DIAGNOSTICS_DIR}/crashes`

      // crashes 目录不存在时静默跳过
      if (!fs.accessSync(crashesDir)) {
        return
      }

      // 遍历 crashes 目录，找到最近修改的事件子目录
      const entries = fs.listFileSync(crashesDir)
      let latestEventKey: string | null = null
      let latestMtime: number = 0

      for (const entry of entries) {
        const eventDirPath = `${crashesDir}/${entry}`
        try {
          const stat = fs.statSync(eventDirPath)
          if (stat.isDirectory() && stat.mtime > latestMtime) {
            latestMtime = stat.mtime
            latestEventKey = entry
          }
        } catch (_) {
          // 单个目录 stat 失败不影响其他目录
        }
      }

      if (latestEventKey === null) {
        return
      }

      if (!/^[A-Za-z0-9_-]+$/.test(latestEventKey)) {
        return
      }

      // 只复制 crash 参数摘要和本应用持久化的 fault-N.log；不把原始
      // external_log 路径或目录中的任意文件纳入 ZIP。
      const eventDirPath = `${crashesDir}/${latestEventKey}`
      const eventFiles = fs.listFileSync(eventDirPath)

      for (const fileName of eventFiles) {
        if (fileName !== 'params.json' && !/^fault-[0-9]+\.log$/.test(fileName)) {
          continue
        }
        const filePath = `${eventDirPath}/${fileName}`
        try {
          const content = this.readFileContent(filePath)
          if (content.length > 0) {
            attachments.push(this.makeAttachment(`crashes/${latestEventKey}/${fileName}`, content))
          }
        } catch (_) {
          // 单个文件读取失败不影响其他文件
        }
      }

      hilog.info(0x0000, LOG_TAG, 'Added crash attachments from event: %{public}s', latestEventKey)
    } catch (e) {
      hilog.error(0x0000, LOG_TAG, 'addLatestCrashAttachments failed: %{public}s', String(e))
    }
  }

  // ── 工具方法 ──────────────────────────────────────────────────

  // 读取单个文件内容为字符串 — 用于 crash 附件收集。
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

  // 构造尚未写入 staging 的 UTF-8 附件项。
  private makeAttachment(relativePath: string, textContent: string): AttachmentItem {
    return {
      relative_path: relativePath,
      content: textContent
    }
  }

  private isWhitelistedAttachmentPath(relativePath: string): boolean {
    const allowedPaths: string[] = [
      ATTACHMENT_HILOG,
      ATTACHMENT_HILOG_CAPTURE_STATS,
      ATTACHMENT_APP_SETTINGS,
      ATTACHMENT_SYNC_STATE,
      ATTACHMENT_FULL_SYNC_STATE,
      ATTACHMENT_LATEST_FULL_SYNC_REPORT,
      ATTACHMENT_SYNC_OPERATION_RUNTIME,
      ATTACHMENT_CURRENT_DEVICE,
      ATTACHMENT_BUILD_IDENTITY,
      ATTACHMENT_THEME_RUNTIME,
      ATTACHMENT_UI_RUNTIME,
      ATTACHMENT_EDITOR_RUNTIME
    ]
    if (allowedPaths.indexOf(relativePath) >= 0) {
      return true
    }
    return /^crashes\/[A-Za-z0-9_-]+\/(?:params\.json|fault-[0-9]+\.log)$/.test(relativePath)
  }

  // Rust's writer stores up to five build logs in this sandbox directory. Include
  // only its deterministic file names; the staging writer redacts their contents.
  private copyPersistedLogs(stagingDir: string): LogCollectionSummary {
    const files: string[] = []
    const logDir = `${this.abilityContext!.filesDir}/${DIAGNOSTICS_DIR}/live/log`
    try {
      if (!fs.accessSync(logDir)) {
        return { status: 'missing', file_count: 0, files: files }
      }
      for (const fileName of fs.listFileSync(logDir)) {
        if (!/^sujian-current-[A-Za-z0-9._-]+\.log$/.test(fileName)) {
          continue
        }
        const filePath = `${logDir}/${fileName}`
        try {
          if (!fs.statSync(filePath).isFile()) {
            continue
          }
          const content = this.readFileContent(filePath)
          HarmonyDiagnosticsArchive.writeTextFile(stagingDir, `logs/${fileName}`, content)
          files.push(fileName)
        } catch (_) {
          // A single unreadable log should not prevent the remaining archive.
        }
      }
    } catch (_) {
      return { status: 'error', file_count: 0, files: files }
    }
    return {
      status: files.length > 0 ? 'ok' : 'missing',
      file_count: files.length,
      files: files
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
