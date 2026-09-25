// SettingsViewModel — 设置页 ViewModel。
// Issue #760 评论 5822350374：
// - 问题9：改为 @ObservedV2 + @Trace，页面直接观察 ViewModel 状态字段
// - 问题4：折叠状态用 AppStorageV2 持久化，跨页面实例保留
// - 问题5：复制设备信息接 @ohos.pasteboard，真正写入系统剪贴板
// - 问题6：clearLogs 同时清 Rust diagnostics 和 HiLog buffer
// 页面只调 ViewModel 暴露的接口，不直接接触 bridge/native。

import hilog from '@ohos.hilog'
import pasteboard from '@ohos.pasteboard'
import { AppStorageV2 } from '@kit.ArkUI'
import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import { LocalSettings, ThemePaletteRecordDto, BuiltinThemeDto } from '../../../corebridge/dto/SettingsDtos'
import { SyncConfig, SyncDiagnosticsResult } from '../../../corebridge/dto/SyncDtos'
import { HarmonyThemeAdapter, ThemeMode, ThemeUpdateParams } from '../../../ui/theme/HarmonyThemeAdapter'
import { getAppContext } from '../../../app/AppContext'
import { MessageKeyMapper } from '../../../ui/i18n/MessageKeyMapper'
import { HarmonyDiagnosticsExporter } from '../../../platform/diagnostics/HarmonyDiagnosticsExporter'
import { NativeDiagnosticsBridge } from '../../../corebridge/native/NativeDiagnosticsBridge'
import { NativeCoreModule } from '../../../corebridge/native/NativeCoreModule'
import { getDeviceInfoSnapshot } from '../../../platform/device/DeviceInfoSnapshot'

const TAG = 'SettingsViewModel'
const DOMAIN = 0x0000

// 设置分类标识 — 供折叠状态管理使用。
export type SettingsSectionId =
  | 'appearance'
  | 'editor'
  | 'save'
  | 'sync'
  | 'recovery'
  | 'ai'
  | 'diagnostics'
  | 'laboratory'
  | 'about'

// 问题4：折叠状态持久化 — 使用 AppStorageV2 跨页面实例保留展开状态。
// AppStorageV2 连接的 key，所有 SettingsScreen 实例共享同一份展开状态。
const EXPANDED_SECTIONS_KEY = 'settings_expanded_sections'

/**
 * SettingsPresentationStore — 设置页展示状态持久化存储。
 * 使用 AppStorageV2 在 AppStorage 层面持久化 expandedSections，
 * 页面 pop 后再进入时恢复之前的展开/收起状态。
 */
@ObservedV2
class SettingsPresentationStore {
  // 每个分类的展开状态，默认全部收起。
  @Trace expandedSections: Map<SettingsSectionId, boolean> = new Map()

  constructor() {
    const allSections: SettingsSectionId[] = [
      'appearance', 'editor', 'save', 'sync', 'recovery',
      'ai', 'diagnostics', 'laboratory', 'about'
    ]
    for (const section of allSections) {
      this.expandedSections.set(section, false)
    }
  }

  toggleSection(section: SettingsSectionId): void {
    const current = this.expandedSections.get(section) ?? false
    // 创建新 Map 引用以触发 V2 观察更新
    const newMap = new Map<SettingsSectionId, boolean>(this.expandedSections)
    newMap.set(section, !current)
    this.expandedSections = newMap
  }

  isSectionExpanded(section: SettingsSectionId): boolean {
    return this.expandedSections.get(section) ?? false
  }
}

@ObservedV2
export class SettingsViewModel {
  private bridge: IWriterCoreBridge = getAppContext().getBridge()
  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private diagnosticsExporter: HarmonyDiagnosticsExporter | null = null
  // 问题6：持有 NativeDiagnosticsBridge 实例，用于 clearHilogSnapshot
  private diagnosticsBridge: NativeDiagnosticsBridge = new NativeDiagnosticsBridge(new NativeCoreModule())

  // 问题4：折叠展开状态 — 通过 AppStorageV2 持久化，跨页面实例保留
  private presentationStore: SettingsPresentationStore = AppStorageV2.connect(
    SettingsPresentationStore, EXPANDED_SECTIONS_KEY, () => new SettingsPresentationStore()
  )!

  // ── 可观察状态字段（@Trace，UI 直接观察） ──
  // 问题9：所有页面需要观察的状态字段加 @Trace，页面不再维护副本
  @Trace settings: LocalSettings = {
    themeMode: 'system',
    appearanceMode: 'system',
    colorSource: 'built_in',
    dynamicColorEnabled: false,
    selectedBuiltinThemeId: '',
    selectedPaletteId: '',
    locale: null,
    autoSaveEnabled: true,
    editorFontSize: 16,
    editorLineSpacingMultiplier: 1.8,
    windowWidth: 0,
    windowHeight: 0,
    autoSaveDelayMs: 30000,
    autoIndentEnabled: true,
    autoIndentWidth: 2,
    editorTypingAnimationEnabled: false,
    editorSmoothCursorEnabled: false,
    editorTypingAnimationDurationMs: 100,
    editorSmoothCursorDurationMs: 80,
    aiEnabled: false,
    statsDeviceId: null,
    desktopSidebarWidth: 0,
    desktopEditorWidth: 0,
    editorCoordinatedTextCursorAnimationEnabled: false,
    diagnosticsEnabled: false,
    diagnosticsVerbose: false
  }
  @Trace paletteRecords: ThemePaletteRecordDto[] = []
  @Trace builtinThemes: BuiltinThemeDto[] = []
  @Trace syncConfig: SyncConfig | null = null
  @Trace isLoading: boolean = true
  @Trace isSaving: boolean = false
  @Trace isSyncing: boolean = false
  @Trace isDiagnosing: boolean = false
  @Trace saveMessage: string = ''
  @Trace syncMessage: string = ''
  @Trace syncDiagnostics: SyncDiagnosticsResult | null = null

  // 诊断配置同步追踪 — 记录上一次同步到 runtime 的值，用于判断是否有变化。
  private lastSyncedDiagnosticsEnabled: boolean = false
  private lastSyncedDiagnosticsVerbose: boolean = false

  // ── 折叠展开状态管理（问题4：通过 AppStorageV2 持久化） ──

  toggleSection(section: SettingsSectionId): void {
    this.presentationStore.toggleSection(section)
  }

  isSectionExpanded(section: SettingsSectionId): boolean {
    return this.presentationStore.isSectionExpanded(section)
  }

  // ── 加载数据 ──

  async loadData(): Promise<void> {
    this.isLoading = true

    try {
      const settingsResult = await this.bridge.getLocalSettings()
      const syncConfigResult = await this.bridge.loadSyncConfig()

      try {
        const paletteResult = await this.bridge.listPaletteRecords()
        if (paletteResult.success && paletteResult.data) {
          this.paletteRecords = paletteResult.data
          this.themeAdapter.setPaletteRecords(paletteResult.data)
        }
      } catch (_e) {
        hilog.warn(DOMAIN, TAG, 'listPaletteRecords failed')
      }

      try {
        const builtinResult = await this.bridge.listBuiltinThemes()
        if (builtinResult.success && builtinResult.data) {
          this.builtinThemes = builtinResult.data
          this.themeAdapter.setBuiltinThemes(builtinResult.data)
        }
      } catch (_e) {
        hilog.warn(DOMAIN, TAG, 'listBuiltinThemes failed')
      }

      if (settingsResult.success && settingsResult.data) {
        this.settings = settingsResult.data
        const appearanceMode = settingsResult.data.appearanceMode || settingsResult.data.themeMode || 'system'
        this.themeAdapter.updateFromLocalSettings(
          appearanceMode,
          settingsResult.data.colorSource || 'built_in',
          settingsResult.data.selectedPaletteId || '',
          settingsResult.data.selectedBuiltinThemeId || ''
        )
        this.themeAdapter.updateFromSettings({
          theme: appearanceMode as ThemeMode,
          fontSize: settingsResult.data.editorFontSize,
          fontFamily: 'HarmonyOS Sans',
          lineHeight: settingsResult.data.editorLineSpacingMultiplier
        } as ThemeUpdateParams)
      }

      if (syncConfigResult.success && syncConfigResult.data) {
        this.syncConfig = syncConfigResult.data
      }

      // 加载设置后，把当前诊断配置同步一次到底层 runtime。
      try {
        await this.bridge.setDiagnosticsConfig(
          this.settings.diagnosticsEnabled,
          this.settings.diagnosticsVerbose
        )
        this.lastSyncedDiagnosticsEnabled = this.settings.diagnosticsEnabled
        this.lastSyncedDiagnosticsVerbose = this.settings.diagnosticsVerbose
      } catch (_e) {
        hilog.warn(DOMAIN, TAG, 'setDiagnosticsConfig failed on load')
      }
    } catch (err) {
      hilog.error(DOMAIN, TAG, `Failed to load settings: ${err}`)
    } finally {
      this.isLoading = false
    }
  }

  // ── 保存设置 ──

  async saveSettings(): Promise<void> {
    this.isSaving = true
    this.saveMessage = ''

    try {
      const result = await this.bridge.saveLocalSettings(this.settings)
      if (result.success) {
        this.saveMessage = 'saved'
        const appearanceMode = this.settings.appearanceMode || this.settings.themeMode || 'system'
        this.themeAdapter.updateFromLocalSettings(
          appearanceMode,
          this.settings.colorSource || 'built_in',
          this.settings.selectedPaletteId || '',
          this.settings.selectedBuiltinThemeId || ''
        )
        this.themeAdapter.updateFromSettings({
          theme: appearanceMode as ThemeMode,
          fontSize: this.settings.editorFontSize,
          fontFamily: 'HarmonyOS Sans',
          lineHeight: this.settings.editorLineSpacingMultiplier
        } as ThemeUpdateParams)

        // 如果诊断配置有变化，同步到 runtime。
        if (this.settings.diagnosticsEnabled !== this.lastSyncedDiagnosticsEnabled ||
          this.settings.diagnosticsVerbose !== this.lastSyncedDiagnosticsVerbose) {
          try {
            await this.bridge.setDiagnosticsConfig(
              this.settings.diagnosticsEnabled,
              this.settings.diagnosticsVerbose
            )
            this.lastSyncedDiagnosticsEnabled = this.settings.diagnosticsEnabled
            this.lastSyncedDiagnosticsVerbose = this.settings.diagnosticsVerbose
          } catch (_e) {
            hilog.warn(DOMAIN, TAG, 'setDiagnosticsConfig failed on save')
          }
        }
      } else {
        this.saveMessage = `error:${result.errorCode || 'unknown'}`
      }
    } catch (err) {
      hilog.error(DOMAIN, TAG, `Failed to save settings: ${err}`)
      this.saveMessage = 'error:save_failed'
    } finally {
      this.isSaving = false
    }
  }

  // ── 执行同步 ──

  async performSync(): Promise<void> {
    this.isSyncing = true
    this.syncMessage = ''

    try {
      const result = await this.bridge.performSync()
      if (result.success && result.data) {
        const syncResult = result.data
        if (syncResult.overallStatus === 'conflict' || syncResult.overallStatus === 'partial_conflict') {
          this.syncMessage = 'error:sync_conflict'
        } else if (syncResult.overallStatus === 'dirty_repo_blocked') {
          this.syncMessage = 'error:dirty_repo_blocked'
        } else if (syncResult.overallStatus === 'no_changes') {
          this.syncMessage = `sync_complete:0`
        } else if (syncResult.overallStatus === 'recoverable_error' ||
          syncResult.overallStatus === 'fatal_error' ||
          syncResult.overallStatus === 'error' ||
          syncResult.overallStatus === 'syncing') {
          const errorCategory = syncResult.errorCategory
          if (errorCategory && errorCategory.length > 0) {
            this.syncMessage = `error:${errorCategory}`
          } else {
            this.syncMessage = 'error:sync_error'
          }
        } else {
          this.syncMessage = `sync_complete:${syncResult.totalDownloaded}`
        }
      } else {
        const errorCategory = result.data?.errorCategory
        if (errorCategory && errorCategory.length > 0) {
          this.syncMessage = `error:${errorCategory}`
        } else {
          this.syncMessage = `error:${result.errorCode || 'sync_error'}`
        }
      }
    } catch (err) {
      hilog.error(DOMAIN, TAG, `Failed to perform sync: ${err}`)
      this.syncMessage = 'error:sync_error'
    } finally {
      this.isSyncing = false
    }
  }

  // ── 运行同步诊断 ──

  async runSyncDiagnostics(): Promise<void> {
    this.isDiagnosing = true
    this.syncDiagnostics = null

    try {
      const result = await this.bridge.syncDiagnostics()
      if (result.success && result.data && result.data.diagnostics) {
        this.syncDiagnostics = result.data.diagnostics
      } else {
        const diagResult: SyncDiagnosticsResult = {
          success: false,
          providerType: '',
          hasNetworkPermission: false,
          hasNetworkStatePermission: false,
          networkState: '',
          networkOk: false,
          authOk: false,
          remoteOk: false,
          networkStatus: '',
          authStatus: '',
          errorCategory: 'unknown',
          rawError: 'diagnostics_error',
          providerDetails: null
        }
        this.syncDiagnostics = diagResult
      }
    } catch (err) {
      hilog.error(DOMAIN, TAG, `Failed to run sync diagnostics: ${err}`)
      const diagResult: SyncDiagnosticsResult = {
        success: false,
        providerType: '',
        hasNetworkPermission: false,
        hasNetworkStatePermission: false,
        networkState: '',
        networkOk: false,
        authOk: false,
        remoteOk: false,
        networkStatus: '',
        authStatus: '',
        errorCategory: 'unknown',
        rawError: 'diagnostics_error_detail',
        providerDetails: null
      }
      this.syncDiagnostics = diagResult
    } finally {
      this.isDiagnosing = false
    }
  }

  // ── 更新设置（供UI修改设置项后调用） ──

  updateSettings(updater: (settings: LocalSettings) => LocalSettings): void {
    this.settings = updater(this.settings)
  }

  // ── 诊断导出 ──

  async exportDiagnostics(): Promise<boolean> {
    try {
      if (this.diagnosticsExporter === null) {
        this.diagnosticsExporter = new HarmonyDiagnosticsExporter()
        const context = getAppContext().getAbilityContext()
        if (context !== null) {
          this.diagnosticsExporter.setAbilityContext(context)
        }
      }
      return await this.diagnosticsExporter.exportAndShare()
    } catch (err) {
      hilog.error(DOMAIN, TAG, `exportDiagnostics failed: ${err}`)
      return false
    }
  }

  // ── 清空诊断日志（问题6：同时清 Rust diagnostics 和 HiLog buffer） ──

  async clearLogs(): Promise<boolean> {
    try {
      // 1. 清空 Rust writer_diagnostics 日志
      const result = await this.bridge.clearDiagnostics()
      if (!result.success) {
        hilog.warn(DOMAIN, TAG, 'clearDiagnostics returned failure')
      }

      // 2. 清空 HiLog 环形缓冲区
      const hilogResult = this.diagnosticsBridge.clearHilogSnapshot()
      if (hilogResult !== 0) {
        hilog.warn(DOMAIN, TAG, `clearHilogSnapshot returned ${hilogResult}`)
      }

      return result.success && hilogResult === 0
    } catch (err) {
      hilog.error(DOMAIN, TAG, `clearLogs failed: ${err}`)
      return false
    }
  }

  // ── 复制设备信息到剪贴板（问题5：真正写入系统剪贴板） ──

  async copyDeviceInfoToClipboard(): Promise<boolean> {
    try {
      const json = this.getDeviceInfoJson()
      const pasteData = pasteboard.createData(pasteboard.MIMETYPE_TEXT_PLAIN, json)
      const systemPasteboard = pasteboard.getSystemPasteboard()
      await systemPasteboard.setData(pasteData)
      hilog.info(DOMAIN, TAG, 'Device info copied to clipboard')
      return true
    } catch (err) {
      hilog.error(DOMAIN, TAG, `copyDeviceInfoToClipboard failed: ${err}`)
      return false
    }
  }

  // ── 获取设备信息 JSON ──
  // 使用统一的 DeviceInfoSnapshot 收集设备信息，避免字段名错误和重复代码。

  getDeviceInfoJson(): string {
    try {
      const snapshot = getDeviceInfoSnapshot()
      return JSON.stringify(snapshot, null, 2)
    } catch (err) {
      hilog.error(DOMAIN, TAG, `getDeviceInfoJson failed: ${err}`)
      return '{}'
    }
  }
}
