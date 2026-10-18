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
import { SyncConfig, SyncDiagnosticsResult, SyncSecrets, ProviderConfig } from '../../../corebridge/dto/SyncDtos'
import { HarmonyThemeAdapter, ThemeUpdateParams } from '../../../ui/theme/HarmonyThemeAdapter'
import { ThemeMode } from '../../../ui/theme/HarmonyThemeTypes'
import { getAppContext } from '../../../app/AppContext'
import { MessageKeyMapper } from '../../../ui/i18n/MessageKeyMapper'
import { LocalSettingsStore } from './LocalSettingsStore'
import { HarmonyDiagnosticsExporter, DiagnosticsSaveResult } from '../../../platform/diagnostics/HarmonyDiagnosticsExporter'
import { NativeDiagnosticsBridge } from '../../../corebridge/native/NativeDiagnosticsBridge'
import { NativeCoreModule } from '../../../corebridge/native/NativeCoreModule'
import { SyncOperationController } from './SyncOperationController'
import { HarmonyAutoSyncScheduler } from './HarmonyAutoSyncScheduler'
import { getDeviceInfoSnapshot } from '../../../platform/device/DeviceInfoSnapshot'

const TAG = 'SettingsViewModel'
const DOMAIN = 0x0000

// Issue #924：Token 输入的落盘防抖窗口——停笔这么久后合并为一次有序持久化。
const SYNC_TOKEN_SAVE_DEBOUNCE_MS = 500

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
    autoSaveDelayMs: 30000,
    autoIndentEnabled: true,
    autoIndentWidth: 2,
    editorTypingAnimationEnabled: false,
    editorSmoothCursorEnabled: false,
    editorTypingAnimationDurationMs: 100,
    editorSmoothCursorDurationMs: 80,
    aiEnabled: false,
    statsDeviceId: null,
    editorCoordinatedTextCursorAnimationEnabled: false,
    diagnosticsEnabled: false,
    diagnosticsVerbose: false
  }
  @Trace paletteRecords: ThemePaletteRecordDto[] = []
  @Trace builtinThemes: BuiltinThemeDto[] = []
  @Trace syncConfig: SyncConfig | null = null
  @Trace syncToken: string = ''
  @Trace syncRemoteUrl: string = ''
  @Trace isLoading: boolean = true
  @Trace isSaving: boolean = false
  @Trace isDiagnosing: boolean = false
  @Trace saveMessage: string = ''
  @Trace syncDiagnostics: SyncDiagnosticsResult | null = null

  // 诊断配置同步追踪 — 记录上一次同步到 runtime 的值，用于判断是否有变化。
  private lastSyncedDiagnosticsEnabled: boolean = false
  private lastSyncedDiagnosticsVerbose: boolean = false

  // ── 同步配置有序串行保存（Issue #925） ──
  // updateSyncConfig / updateSyncRemoteUrl 只更新内存并请求保存；请求进单飞队列，
  // 保存期间的新请求合并为一次后续保存，最终落盘的一定是最后一次提交的配置；
  // 只有 Core 确认保存成功的配置才交给 HarmonyAutoSyncScheduler。
  private syncConfigSaveQueued: boolean = false
  private syncConfigSaveRunning: Promise<void> | null = null

  // ── Token 落盘合并（Issue #924） ──
  // TextInput.onChange 逐字触发 updateSyncToken：只更新内存并重置防抖计时器；
  // 输入停顿后由一次串行保存落盘，不再逐字同步 NAPI I/O。
  private syncTokenSaveTimerId: number = -1
  private syncSecretsSaveQueued: boolean = false
  private syncSecretsSaveRunning: Promise<void> | null = null

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
        // Issue #911：启动时 EntryAbility.initDiagnostics 已按这份 Core 设置启动了诊断后端，
        // 所以这里把 lastSynced 记为这份真实值，作为后续 saveSettings 的变化基线。
        // settingsResult 失败时（读取失败）绝不更新 lastSynced，也不把 UI 占位 DTO 下发底层。
        this.lastSyncedDiagnosticsEnabled = settingsResult.data.diagnosticsEnabled
        this.lastSyncedDiagnosticsVerbose = settingsResult.data.diagnosticsVerbose
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
        // 回填 UI 可观察字段
        if (syncConfigResult.data.providerConfig) {
          this.syncRemoteUrl = syncConfigResult.data.providerConfig.remote_url
        }
        // Issue #925：把 Core 已提交的配置交给进程唯一自动同步调度器。
        // reconfigure 只记录配置、启停前台唤醒 timer；是否真正同步由 runIfDue 决定，
        // 打开设置页本身不会额外产生一笔同步。
        HarmonyAutoSyncScheduler.getInstance().reconfigure(syncConfigResult.data)
      }

      // 加载同步密钥，回填 token
      await this.loadSyncSecrets()
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
        // Issue #825 复核8第 3 点：把刚落盘的那份设置广播出去。
        // 宽屏下设置浮层盖在写作页上，两边各持一份 settings 会长期互不认知：
        // 用户在这里改了字号/主题，底下活着的 WritingScreen 仍按旧排版渲染。
        // 广播时机是"保存成功之后"，和 Linux 的 settingsBackend.onSettings_changed 同语义。
        //
        // Issue #825 复核9 第3项：不能把 this.settings 直接 publish 出去。
        // 它是 SettingsContent 还在原地 edit 的对象（updateSettings 基本都是
        // `s.editorFontSize = value; return s`），用户下一次拖 Slider 会在下一次
        // save 成功之前就把这份"所谓最近一次落盘值"改掉。重新从 Core 读一次
        // 拿到独立 decode 的 DTO，Store 才真的代表磁盘真相。
        const persisted = await this.bridge.getLocalSettings()
        if (persisted.success && persisted.data) {
          LocalSettingsStore.getInstance().publish(persisted.data)
          // Issue #911：诊断开关只在 Core 真正落盘、且重新读回成功后，才比较读回的持久化值
          // 与上次同步值；this.settings 是 UI 还在原地编辑的对象，不能当"已落盘真相"。
          const persistedEnabled: boolean = persisted.data.diagnosticsEnabled
          const persistedVerbose: boolean = persisted.data.diagnosticsVerbose
          if (persistedEnabled !== this.lastSyncedDiagnosticsEnabled ||
            persistedVerbose !== this.lastSyncedDiagnosticsVerbose) {
            // setDiagnosticsConfig 返回 ResultEnvelope；success:false 不等于抛异常，
            // 不能照旧宣称已同步，只有 success 才更新 lastSynced。
            const syncResult = await this.bridge.setDiagnosticsConfig(persistedEnabled, persistedVerbose)
            if (syncResult.success) {
              this.lastSyncedDiagnosticsEnabled = persistedEnabled
              this.lastSyncedDiagnosticsVerbose = persistedVerbose
            } else {
              hilog.warn(DOMAIN, TAG, `setDiagnosticsConfig failed on save: ${syncResult.errorCode || 'unknown'}`)
            }
          }
        }
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
  // 同步运行状态和结果由 SyncRunPresentationState 统一持有；ViewModel 只准备凭据并触发 Controller。
  async performSync(): Promise<void> {
    // Issue #924：若 Token 还在防抖等待/队列中，先落盘再同步，避免用旧凭据发起同步。
    if (this.isSyncSecretsSavePending()) {
      await this.saveSyncSecrets()
    }
    await SyncOperationController.getInstance().performSync()
  }

  // ── 运行同步诊断 ──

  async runSyncDiagnostics(): Promise<void> {
    this.isDiagnosing = true
    this.syncDiagnostics = null

    try {
      const controller = SyncOperationController.getInstance()
      const result = await controller.runDiagnostics()
      if (result.success && result.data && result.data.diagnostics) {
        this.syncDiagnostics = result.data.diagnostics
      } else {
        // 保留真实 errorCode/messageKey/rawError，不用固定 "unknown"/"diagnostics_error" 替换
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
          errorCategory: result.errorCode ?? 'unknown',
          rawError: result.rawError ?? result.userMessage ?? '',
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
        errorCategory: 'NATIVE_ERROR',
        rawError: String(err),
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

  // ── 同步配置更新与保存 ──

  /**
   * 更新 syncConfig 并排队保存到 Core（Issue #925：有序串行保存，保存期间合并重复变更）。
   * 保证 activeProvider='github_api'，providerConfig.type='github'，transport='https_token'。
   * 只有真正落盘成功的配置才会 reconfigure 自动同步调度器；不 await（UI 回调不阻塞），
   * 保存结果由队列内部感知。
   */
  updateSyncConfig(config: SyncConfig): void {
    this.syncConfig = config
    this.saveSyncConfig()
  }

  /**
   * 更新远程仓库地址，修改 providerConfig.remote_url，然后保存。
   */
  updateSyncRemoteUrl(value: string): void {
    this.syncRemoteUrl = value
    if (this.syncConfig) {
      // providerConfig 缺失时新建，branch 沿用已有值（拿不到时回落 'main'），
      // 不再由用户在设置页手输；已有 providerConfig 只更新 remote_url。
      // 注意：必须在创建新对象前先读取旧 branch，避免在 ?? 表达式右侧访问已被收窄的类型。
      const existingProviderConfig: ProviderConfig | null = this.syncConfig.providerConfig
      const previousBranch: string = existingProviderConfig ? existingProviderConfig.branch : 'main'
      const providerConfig: ProviderConfig = existingProviderConfig ?? {
        type: 'github',
        remote_url: value,
        branch: previousBranch,
        username: '',
        transport: 'https_token'
      }
      providerConfig.remote_url = value
      const newConfig: SyncConfig = {
        enabled: this.syncConfig.enabled,
        activeProvider: 'github_api',
        providerConfig: providerConfig,
        autoSync: this.syncConfig.autoSync,
        syncIntervalSeconds: this.syncConfig.syncIntervalSeconds,
        hasNetworkPermission: this.syncConfig.hasNetworkPermission,
        hasNetworkStatePermission: this.syncConfig.hasNetworkStatePermission
      }
      this.syncConfig = newConfig
      this.saveSyncConfig()
    }
  }

  /**
   * 更新 Token（仅更新内存状态）。
   * Issue #924：不再逐字同步落盘；由防抖合并 + 串行队列保证输入完成后落盘一次、
   * 最后一次输入必达。需要立即落盘（onSubmit/onBlur）时调用 commitSyncToken()。
   */
  updateSyncToken(value: string): void {
    this.syncToken = value
    this.scheduleSyncSecretsSave()
  }

  /**
   * Issue #924：Token 输入完成/提交入口。
   * 取消尚未触发的防抖等待，立即把当前 Token 并入串行队列落盘；
   * 供 UI 在 onSubmit/onBlur 时调用（未接线时由防抖路径兜底）。
   */
  async commitSyncToken(): Promise<void> {
    await this.saveSyncSecrets()
  }

  /**
   * 保存同步配置到 Core —— 单飞串行队列入口（Issue #925）。
   * 保存进行中再次调用：合并为"完成后保存最新 this.syncConfig"；
   * await 本方法的调用方在队列（含本次并入的更新）排空后返回。
   * 保存成功才 reconfigure 自动同步调度器；失败保持现有语义（saveMessage 带 errorCode）。
   */
  async saveSyncConfig(): Promise<void> {
    if (!this.syncConfig) {
      return
    }
    this.syncConfigSaveQueued = true
    if (this.syncConfigSaveRunning === null) {
      this.syncConfigSaveRunning = this.drainSyncConfigSaves()
    }
    await this.syncConfigSaveRunning
  }

  /** 队列排空循环：每轮取最新 this.syncConfig 保存；期间的新请求只置标记，合并进下一轮。 */
  private async drainSyncConfigSaves(): Promise<void> {
    try {
      while (this.syncConfigSaveQueued) {
        this.syncConfigSaveQueued = false
        const config: SyncConfig | null = this.syncConfig
        if (config === null) {
          continue
        }
        const persisted: boolean = await this.persistSyncConfig(config)
        if (persisted) {
          // 只有 Core 真正落盘的配置才更新调度器；未提交的 UI 编辑不触发自动同步。
          HarmonyAutoSyncScheduler.getInstance().reconfigure(config)
        }
      }
    } finally {
      this.syncConfigSaveRunning = null
    }
  }

  /** 实际保存一份配置；返回是否已真正持久化。 */
  private async persistSyncConfig(config: SyncConfig): Promise<boolean> {
    try {
      const result = await this.bridge.saveSyncConfig(config)
      if (!result.success) {
        this.saveMessage = `error:${result.errorCode || 'unknown'}`
        return false
      }
      return true
    } catch (err) {
      hilog.error(DOMAIN, TAG, `Failed to save sync config: ${err}`)
      this.saveMessage = 'error:save_failed'
      return false
    }
  }

  /**
   * 加载同步密钥，从结果中提取 token 回填到 syncToken。
   */
  async loadSyncSecrets(): Promise<void> {
    try {
      const result = await this.bridge.loadSyncSecrets()
      if (result.success && result.data) {
        if (result.data.providerSecrets) {
          this.syncToken = result.data.providerSecrets.token
        } else {
          this.syncToken = ''
        }
      }
    } catch (err) {
      hilog.error(DOMAIN, TAG, `Failed to load sync secrets: ${err}`)
    }
  }

  /**
   * 保存同步密钥（Token）—— 单飞串行队列入口（Issue #924）。
   * 取消防抖等待，把最新 Token 并入队列：多次变更合并、串行执行、最后一次必达。
   * 只有 Core 返回 success=true 才报告"已保存"；失败立即写结构化 errorCode
   * （saveMessage），不会被之后的同步结果提示掩盖。
   */
  async saveSyncSecrets(): Promise<void> {
    this.clearSyncTokenSaveTimer()
    this.syncSecretsSaveQueued = true
    if (this.syncSecretsSaveRunning === null) {
      this.syncSecretsSaveRunning = this.drainSyncSecretsSaves()
    }
    await this.syncSecretsSaveRunning
  }

  /** 防抖：每次输入变更重置计时器，停笔后合并触发一次落盘。 */
  private scheduleSyncSecretsSave(): void {
    this.clearSyncTokenSaveTimer()
    this.syncTokenSaveTimerId = setTimeout(() => {
      this.syncTokenSaveTimerId = -1
      this.saveSyncSecrets()
    }, SYNC_TOKEN_SAVE_DEBOUNCE_MS)
  }

  private clearSyncTokenSaveTimer(): void {
    if (this.syncTokenSaveTimerId !== -1) {
      clearTimeout(this.syncTokenSaveTimerId)
      this.syncTokenSaveTimerId = -1
    }
  }

  /** 是否有尚未交给 Core 保存的 Token 变更（防抖等待中或已排队）。 */
  private isSyncSecretsSavePending(): boolean {
    return this.syncTokenSaveTimerId !== -1 || this.syncSecretsSaveQueued
  }

  /** 队列排空循环：每轮取最新 this.syncToken 保存；期间的新变更合并进下一轮。 */
  private async drainSyncSecretsSaves(): Promise<void> {
    try {
      while (this.syncSecretsSaveQueued) {
        this.syncSecretsSaveQueued = false
        await this.persistSyncSecrets(this.syncToken)
      }
    } finally {
      this.syncSecretsSaveRunning = null
    }
  }

  /** 实际保存一份 Token；成功才报告"已保存"，失败写结构化 errorCode。 */
  private async persistSyncSecrets(token: string): Promise<boolean> {
    try {
      const secrets: SyncSecrets = {
        providerSecrets: token.length === 0 ? null : {
          type: 'github',
          token: token
        }
      }
      const result = await this.bridge.saveSyncSecrets(secrets)
      if (result.success) {
        // Issue #924：只有 Core 真正落盘成功才报告"已保存"。
        this.saveMessage = 'saved'
        return true
      }
      // 失败：立即显示结构化 errorCode；共享同步状态不会改写这条保存提示。
      this.saveMessage = `error:${result.errorCode || 'unknown'}`
      return false
    } catch (err) {
      hilog.error(DOMAIN, TAG, `Failed to save sync secrets: ${err}`)
      this.saveMessage = 'error:save_failed'
      return false
    }
  }

  // ── 诊断导出 ──
  // Issue #784：设置页按钮是用户主动导出，使用 DocumentViewPicker.save() 让用户选择保存位置，
  // 将诊断 zip 文件保存到用户可见的本地目录，而非拉起分享面板。
  // export() 继续给内部快照使用；exportAndShare() 保留给以后可能的"分享诊断包"功能。
  // Issue #804：返回 DiagnosticsSaveResult 区分成功 / 用户取消 / 真失败，让 UI 能给出明确反馈。
  async exportDiagnostics(): Promise<DiagnosticsSaveResult> {
    try {
      if (this.diagnosticsExporter === null) {
        this.diagnosticsExporter = new HarmonyDiagnosticsExporter()
        const context = getAppContext().getAbilityContext()
        if (context !== null) {
          this.diagnosticsExporter.setAbilityContext(context)
        }
      }
      return await this.diagnosticsExporter.exportAndSaveToUserFile()
    } catch (err) {
      hilog.error(DOMAIN, TAG, `exportDiagnostics failed: ${err}`)
      return { status: 'failed', error: String(err) }
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
