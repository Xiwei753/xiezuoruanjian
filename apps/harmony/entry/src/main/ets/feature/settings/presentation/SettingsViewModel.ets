// SettingsViewModel — 设置页 ViewModel。
// 从 SettingsScreen.ets 移出的所有 Core/NAPI 调用、async 加载/保存、sync/diagnostics 操作。
// 页面只调 ViewModel 暴露的接口，不直接接触 bridge/native。

import hilog from '@ohos.hilog'
import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import { LocalSettings, ThemePaletteRecordDto, BuiltinThemeDto } from '../../../corebridge/dto/SettingsDtos'
import { SyncConfig, SyncDiagnosticsResult } from '../../../corebridge/dto/SyncDtos'
import { HarmonyThemeAdapter, ThemeMode, ThemeUpdateParams } from '../../../ui/theme/HarmonyThemeAdapter'
import { getBridge } from '../../../app/AppContext'
import { MessageKeyMapper } from '../../../ui/i18n/MessageKeyMapper'

const TAG = 'SettingsViewModel'
const DOMAIN = 0x0000

export class SettingsViewModel {
  private bridge: IWriterCoreBridge = getBridge()
  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()

  // 状态字段
  private settings: LocalSettings = {
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
  private paletteRecords: ThemePaletteRecordDto[] = []
  private builtinThemes: BuiltinThemeDto[] = []
  private syncConfig: SyncConfig | null = null
  private isLoading: boolean = true
  private isSaving: boolean = false
  private isSyncing: boolean = false
  private isDiagnosing: boolean = false
  private saveMessage: string = ''
  private syncMessage: string = ''
  private syncDiagnostics: SyncDiagnosticsResult | null = null

  // ── 公共查询接口 ──

  getSettings(): LocalSettings {
    return this.settings
  }

  getSyncConfig(): SyncConfig | null {
    return this.syncConfig
  }

  getPaletteRecords(): ThemePaletteRecordDto[] {
    return this.paletteRecords
  }

  getBuiltinThemes(): BuiltinThemeDto[] {
    return this.builtinThemes
  }

  isLoadingData(): boolean {
    return this.isLoading
  }

  isSavingData(): boolean {
    return this.isSaving
  }

  isSyncingData(): boolean {
    return this.isSyncing
  }

  isDiagnosingData(): boolean {
    return this.isDiagnosing
  }

  getSaveMessage(): string {
    return this.saveMessage
  }

  getSyncMessage(): string {
    return this.syncMessage
  }

  getSyncDiagnostics(): SyncDiagnosticsResult | null {
    return this.syncDiagnostics
  }

  // ── 加载数据 ──

  async loadData(): Promise<void> {
    this.isLoading = true

    try {
      const settingsResult = await this.bridge.getLocalSettings()
      // 全量同步（Issue #630）：一个全局 SyncConfig，不依赖选中作品。
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
      } else {
        // envelope 只有 errorCode / userMessage（Core 不发明 messageKey / messageArgs）。
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
          // 失败/未完成：需要用户处理或可重试，展示错误分类文案
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
        // envelope 只有 errorCode / userMessage：先按 errorCategory 分类映射，
        // 其余落到按 errorCode 的本地化兜底文案。
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
        // 字段与 SyncDiagnosticsResult DTO 一字不差对齐（Issue #753）。
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
      // 字段与 SyncDiagnosticsResult DTO 一字不差对齐（Issue #753）。
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

  // ── 诊断导出占位方法（Task #7 会补齐具体 bridge 实现） ──

  async exportDiagnostics(): Promise<boolean> {
    // 占位：Task #7 会在此处调用 bridge 的 diagnostics 导出相关方法。
    hilog.info(DOMAIN, TAG, 'exportDiagnostics placeholder — Task #7 will implement')
    return false
  }

  async clearLogs(): Promise<boolean> {
    // 占位：Task #7 会在此处调用 bridge 的 diagnostics 清空日志相关方法。
    hilog.info(DOMAIN, TAG, 'clearLogs placeholder — Task #7 will implement')
    return false
  }

  getDeviceInfoJson(): string {
    // 占位：Task #7 会在此处调用 bridge 的 diagnostics 设备信息相关方法。
    hilog.info(DOMAIN, TAG, 'getDeviceInfoJson placeholder — Task #7 will implement')
    return '{}'
  }
}
