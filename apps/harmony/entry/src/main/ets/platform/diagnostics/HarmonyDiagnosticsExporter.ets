// HarmonyDiagnosticsExporter — HarmonyOS 诊断导出器。
//
// 负责：
// 1. 在 ArkUI 线程收集轻量、已脱敏的平台附件（hilog / app_settings / sync_state /
//    device_info / build_identity / 运行态 JSON）
// 2. 把附件清单、沙箱路径和归档用途投递给 DiagnosticsArchiveWorker；日志枚举、
//    故障附件复制、manifest、ZIP 压缩和 latest/previous 发布全部在 Worker 执行
// 3. 拉起 HarmonyOS 系统分享面板或保存到用户文件
//
// 导出不能依赖 writer_core NAPI；Native 可用时仅尽力 flush / 读取 HiLog 快照。
// 本类不再扫描/读取持久日志或 crash 目录，也不在 ArkUI 线程做整包压缩。
//
// 参考 Android 端 DiagnosticsExporterInterop.kt。

import { common } from '@kit.AbilityKit'
import { fileUri, picker } from '@kit.CoreFileKit'
import hilog from '@ohos.hilog'
import bundleManager from '@ohos.bundle.bundleManager'
import { BUILD_GIT_SHA } from '../../generated/BuildRevision'

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
import { HarmonyDiagnosticsArchiveWorkerClient } from './HarmonyDiagnosticsArchiveWorkerClient'
import {
  DiagnosticsArchiveAttachment,
  DiagnosticsArchiveKind,
  DiagnosticsArchiveManifestMeta,
  DiagnosticsArchiveRequest,
  DIAGNOSTICS_ARCHIVE_DATA_VERSION
} from './DiagnosticsArchiveProtocol'

// 已整理的 UTF-8 诊断附件。relative_path 必须命中本文件的显式白名单。
interface AttachmentItem {
  relative_path: string
  content: string
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

// sync_config_sanitized.json — only stable provider metadata, never remote URL or secrets.
interface SyncConfigOverviewAttachment {
  enabled: boolean
  providerType: string | null
  hasRemoteUrl: boolean
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
  operationId: string | null
  failureStage: string | null
  errorCategory: string | null
}

// build_identity.json 附件 DTO。
interface BuildIdentityAttachment {
  appVersion: string
  buildKey: string
  gitSha: string
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
const ATTACHMENT_SYNC_CONFIG = 'sync_config_sanitized.json'
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
  // 构建身份只读且低频，缓存后避免故障事件回调和导出入口重复走 bundleManager 同步 IPC。
  private cachedBuildIdentity: BuildIdentityAttachment | null = null

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

  // 用户手动导出完整诊断包 — 当前 live 事件 + 最近故障附件。
  // 返回 zip 文件路径；失败返回 null。
  async export(): Promise<string | null> {
    if (this.abilityContext === null) {
      hilog.error(0x0000, LOG_TAG, 'abilityContext is null; cannot export')
      return null
    }
    const filesDir = this.abilityContext.filesDir
    const outputDir = `${filesDir}/${DIAGNOSTICS_DIR}/snapshots`
    return this.exportTo(outputDir, 'full', '')
  }

  // 后台收尾归档 — 由 Checkpoint 在后台保存全部完成后调用，发布 latest/previous 双槽。
  async exportCheckpoint(): Promise<string | null> {
    if (this.abilityContext === null) {
      return null
    }
    const outputDir = `${this.abilityContext.filesDir}/${DIAGNOSTICS_DIR}/snapshots`
    return this.exportTo(outputDir, 'checkpoint', '')
  }

  // 启动时查询已知故障 eventKey，供 CrashWatcher 在订阅系统事件前完成持久去重。
  async listKnownFaultEventKeys(): Promise<string[]> {
    if (this.abilityContext === null) {
      return []
    }
    const result = await HarmonyDiagnosticsArchiveWorkerClient.getInstance().requestFaultKeys(
      `${this.abilityContext.filesDir}/${DIAGNOSTICS_DIR}/crashes`
    )
    if (!result.success || result.eventKeys === undefined) {
      return []
    }
    return result.eventKeys
  }

  // 故障事件落盘 + fault ZIP。事件回调只传白名单元数据和 external_log 路径，
  // 目录创建、params 原子写入、大文件复制/删除、压缩和 complete.json 全在 Worker。
  async captureFault(eventKey: string, paramsJson: string,
    externalLogPaths: string[]): Promise<string | null> {
    if (this.abilityContext === null) {
      return null
    }
    const filesDir = this.abilityContext.filesDir
    const outputDir = `${filesDir}/${DIAGNOSTICS_DIR}/snapshots`
    const safeEventKey = eventKey.replace(/[^A-Za-z0-9_-]/g, '_')
    const result = await HarmonyDiagnosticsArchiveWorkerClient.getInstance().requestFaultCapture(
      outputDir,
      `${filesDir}/${DIAGNOSTICS_DIR}/live/log`,
      `${filesDir}/${DIAGNOSTICS_DIR}/crashes`,
      safeEventKey,
      paramsJson,
      externalLogPaths,
      this.buildManifestMeta()
    )
    if (!result.success) {
      hilog.error(0x0000, LOG_TAG,
        'fault capture failed: stage=%{public}s durationMs=%{public}d',
        result.stage, result.durationMs)
      return null
    }
    hilog.info(0x0000, LOG_TAG,
      'fault capture completed: stage=%{public}s durationMs=%{public}d',
      result.stage, result.durationMs)
    return result.archivePath.length > 0 ? result.archivePath : null
  }

  // 启动补全上次进程中断的故障事件；只补已经落盘的 params/fault 日志，不阻塞启动。
  async recoverIncompleteFaults(): Promise<void> {
    if (this.abilityContext === null) {
      return
    }
    const filesDir = this.abilityContext.filesDir
    const result = await HarmonyDiagnosticsArchiveWorkerClient.getInstance().requestFaultRecovery(
      `${filesDir}/${DIAGNOSTICS_DIR}/snapshots`,
      `${filesDir}/${DIAGNOSTICS_DIR}/live/log`,
      `${filesDir}/${DIAGNOSTICS_DIR}/crashes`,
      this.buildManifestMeta()
    )
    if (!result.success) {
      hilog.warn(0x0000, LOG_TAG,
        'fault recovery incomplete: stage=%{public}s durationMs=%{public}d',
        result.stage, result.durationMs)
    }
  }

  // 归档统一入口：ArkUI 线程只收集轻量附件并投递 Worker 请求。
  // archiveKind 决定输出命名/发布策略，eventKey 只用于 fault 包。
  async exportTo(outputDir: string, archiveKind: DiagnosticsArchiveKind = 'full',
    eventKey: string = ''): Promise<string | null> {
    if (this.abilityContext === null) {
      hilog.error(0x0000, LOG_TAG, 'abilityContext is null; cannot export')
      return null
    }

    // Flush is only a best-effort log barrier. Its failure cannot block the
    // archive request when the writer native module is unavailable.
    try {
      const flushResult = this.diagnosticsBridge.flushDiagnostics()
      if (flushResult !== 0) {
        hilog.warn(0x0000, LOG_TAG, 'flushDiagnostics returned %{public}d; archiving persisted files', flushResult)
      }
    } catch (_) {
      hilog.info(0x0000, LOG_TAG, 'native diagnostics flush unavailable; archiving persisted files')
    }

    const attachments = await this.collectAttachments()
    const filtered: DiagnosticsArchiveAttachment[] = []
    for (const attachment of attachments) {
      if (!this.isWhitelistedAttachmentPath(attachment.relative_path)) {
        hilog.warn(0x0000, LOG_TAG, 'Skipped non-whitelisted attachment')
        continue
      }
      const item: DiagnosticsArchiveAttachment = {
        relativePath: attachment.relative_path,
        content: attachment.content
      }
      filtered.push(item)
    }

    const filesDir = this.abilityContext.filesDir
    const safeEventKey = eventKey.replace(/[^A-Za-z0-9_-]/g, '_')
    const manifest: DiagnosticsArchiveManifestMeta = this.buildManifestMeta()
    const request: DiagnosticsArchiveRequest = {
      requestKind: 'archive',
      requestId: '',
      archiveKind: archiveKind,
      filesDir: filesDir,
      outputDir: outputDir,
      logDir: `${filesDir}/${DIAGNOSTICS_DIR}/live/log`,
      crashesDir: `${filesDir}/${DIAGNOSTICS_DIR}/crashes`,
      fileStem: archiveKind === 'checkpoint' ? 'sujian-diagnostics-latest' :
        (archiveKind === 'fault' ? `sujian-diagnostics-fault-${safeEventKey}` : 'sujian-diagnostics'),
      eventKey: safeEventKey,
      attachments: filtered,
      recentCrashLimit: archiveKind === 'full' ? 3 : 0,
      publishLatest: archiveKind === 'checkpoint',
      manifest: manifest
    }

    const result = await HarmonyDiagnosticsArchiveWorkerClient.getInstance().requestArchive(request)
    if (!result.success) {
      hilog.error(0x0000, LOG_TAG,
        'diagnostics archive failed: kind=%{public}s stage=%{public}s durationMs=%{public}d',
        archiveKind, result.stage, result.durationMs)
      return null
    }
    hilog.info(0x0000, LOG_TAG,
      'diagnostics archive generated: kind=%{public}s durationMs=%{public}d',
      archiveKind, result.durationMs)
    return result.archivePath.length > 0 ? result.archivePath : null
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
      // 固定分享文件名：share 目录只保留当前待分享的一份，不做历史仓库。
      // 整包复制由 Worker 分块完成，ArkUI 线程不按 ZIP 大小申请内存。
      const filesDir = this.abilityContext!.filesDir
      const shareZipPath = `${filesDir}/${DIAGNOSTICS_DIR}/share/sujian-diagnostics-share.zip`
      const copyResult = await HarmonyDiagnosticsArchiveWorkerClient.getInstance().requestFileCopy(
        zipPath, shareZipPath
      )
      if (!copyResult.success) {
        hilog.error(0x0000, LOG_TAG,
          'share zip copy failed: stage=%{public}s durationMs=%{public}d',
          copyResult.stage, copyResult.durationMs)
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

      const copyResult = await HarmonyDiagnosticsArchiveWorkerClient.getInstance().requestFileCopy(
        zipPath, uris[0]
      )
      if (!copyResult.success) {
        hilog.error(0x0000, LOG_TAG,
          'save diagnostics copy failed: stage=%{public}s durationMs=%{public}d',
          copyResult.stage, copyResult.durationMs)
        return { status: 'failed', error: `保存诊断包失败（${copyResult.stage}）` }
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
    await this.addSyncOperationRuntimeAttachment(attachments)

    // app_settings_sanitized.json
    await this.addAppSettingsAttachment(attachments)

    // sync_state_sanitized.json
    await this.addSyncStateAttachment(attachments)

    // sync_config_sanitized.json — provider identity and remote URL presence only.
    await this.addSyncConfigOverviewAttachment(attachments)

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

    // 故障附件由 Worker 按 archiveKind/eventKey 从 crashes/ 复制，避免主线程扫描目录。
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

  private async addSyncConfigOverviewAttachment(attachments: AttachmentItem[]): Promise<void> {
    try {
      const result = await this.syncBridge.loadSyncConfig()
      if (result.success && result.data !== undefined && result.data !== null) {
        const config = result.data
        const configuredProvider = config.providerConfig?.type ?? null
        const providerType = configuredProvider === 'git_https' || configuredProvider === 'github'
          ? configuredProvider : null
        const sanitized: SyncConfigOverviewAttachment = {
          enabled: config.enabled,
          providerType: providerType,
          hasRemoteUrl: (config.providerConfig?.remote_url ?? '').trim().length > 0
        }
        attachments.push(this.makeAttachment(ATTACHMENT_SYNC_CONFIG,
          JSON.stringify(sanitized, null, 2)))
      } else {
        attachments.push(this.errorAttachment(ATTACHMENT_SYNC_CONFIG,
          result.errorCode ?? 'loadSyncConfig failed'))
      }
    } catch (e) {
      attachments.push(this.errorAttachment(ATTACHMENT_SYNC_CONFIG,
        e instanceof Error ? e.name : 'UNKNOWN_ERROR'))
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
  private async addSyncOperationRuntimeAttachment(attachments: AttachmentItem[]): Promise<void> {
    try {
      const snapshot = SyncRunPresentationState.getInstance().snapshot()
      let failureStage = snapshot.outcome?.failureStage ?? null
      if (snapshot.phase === 'failed' && (failureStage === null || failureStage === 'core_sync')) {
        failureStage = await this.loadMostRecentFailureStage() ?? failureStage
      }
      const runtime: SyncOperationRuntimeAttachment = {
        phase: snapshot.phase,
        source: snapshot.source,
        startedAtMs: snapshot.startedAtMs,
        finishedAtMs: snapshot.finishedAtMs,
        errorCode: snapshot.outcome?.errorCode ?? null,
        overallStatus: snapshot.outcome?.overallStatus ?? null,
        operationId: snapshot.outcome?.operationId ?? null,
        failureStage: failureStage,
        errorCategory: snapshot.outcome?.errorCategory ?? null
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

  private async loadMostRecentFailureStage(): Promise<string | null> {
    try {
      const stateResult = await this.syncBridge.loadFullSyncState()
      if (stateResult.success && stateResult.data !== undefined && stateResult.data !== null) {
        const allowedStages: string[] = [
          'config_load', 'secrets_load', 'core_enter', 'provider_init', 'starmap_store_flush',
          'remote_revision', 'prepare', 'seed', 'transfer', 'commit', 'preflight'
        ]
        const failedTargets: string[] = stateResult.data.failed_targets
        for (let i = failedTargets.length - 1; i >= 0; i--) {
          if (allowedStages.indexOf(failedTargets[i]) >= 0) {
            return failedTargets[i]
          }
        }
      }
      const result = await this.syncBridge.loadLatestFullSyncReport()
      if (!result.success || result.data === undefined || result.data === null || result.data.failures.length === 0) {
        return null
      }
      let latest = result.data.failures[0]
      for (const failure of result.data.failures) {
        if (failure.occurred_at >= latest.occurred_at) {
          latest = failure
        }
      }
      const allowedStages: string[] = [
        'config_load', 'secrets_load', 'core_enter', 'provider_init', 'starmap_store_flush',
        'remote_revision', 'prepare', 'seed', 'transfer', 'commit', 'preflight'
      ]
      return allowedStages.indexOf(latest.phase) >= 0 ? latest.phase : null
    } catch (_) {
      return null
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
      ATTACHMENT_SYNC_CONFIG,
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
    return allowedPaths.indexOf(relativePath) >= 0
  }

  // 构造错误附件 — 收集失败时写入合法 JSON 附件，不让整个导出失败。
  private errorAttachment(relativePath: string, errorMsg: string): AttachmentItem {
    const errorJson = JSON.stringify({ 'error': errorMsg }, null, 2)
    return this.makeAttachment(relativePath, errorJson)
  }

  private buildManifestMeta(): DiagnosticsArchiveManifestMeta {
    const identity = this.getBuildIdentity()
    return {
      dataVersion: DIAGNOSTICS_ARCHIVE_DATA_VERSION,
      appVersion: identity.appVersion,
      buildKey: identity.buildKey,
      gitSha: identity.gitSha,
      exportedAt: new Date().toISOString()
    }
  }

  // 获取构建身份 — 与 EntryAbility.getBuildIdentity() 逻辑一致。
  private getBuildIdentity(): BuildIdentityAttachment {
    if (this.cachedBuildIdentity !== null) {
      return this.cachedBuildIdentity
    }
    try {
      const info = bundleManager.getBundleInfoForSelfSync(
        bundleManager.BundleFlag.GET_BUNDLE_INFO_DEFAULT
      )
      this.cachedBuildIdentity = {
        appVersion: info.versionName,
        buildKey: `harmony-v${info.versionCode}-${info.versionName}-${BUILD_GIT_SHA}`,
        gitSha: BUILD_GIT_SHA
      }
    } catch (_) {
      this.cachedBuildIdentity = {
        appVersion: 'unknown',
        buildKey: `harmony-unknown-${BUILD_GIT_SHA}`,
        gitSha: BUILD_GIT_SHA
      }
    }
    return this.cachedBuildIdentity
  }

}
