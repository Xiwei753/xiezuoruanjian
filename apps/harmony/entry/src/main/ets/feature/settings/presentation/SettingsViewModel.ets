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
import { LocalSettings, BuiltinThemeDto } from '../../../corebridge/dto/SettingsDtos'
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
import { SyncOutcome } from './SyncRunTypes'
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

// Issue #1026：设置浮层只提交本次实际改动的字段，避免把整份旧 DTO 写回磁盘，
// 覆盖写作页已经落盘的字号/行距等字段。
type LocalSettingsField =
  | 'themeMode'
  | 'appearanceMode'
  | 'colorSource'
  | 'dynamicColorEnabled'
  | 'selectedBuiltinThemeId'
  | 'selectedPaletteId'
  | 'locale'
  | 'autoSaveEnabled'
  | 'editorFontSize'
  | 'editorLineSpacingMultiplier'
  | 'autoSaveDelayMs'
  | 'autoIndentEnabled'
  | 'autoIndentWidth'
  | 'editorTypingAnimationEnabled'
  | 'editorSmoothCursorEnabled'
  | 'editorTypingAnimationDurationMs'
  | 'editorSmoothCursorDurationMs'
  | 'aiEnabled'
  | 'statsDeviceId'
  | 'editorCoordinatedTextCursorAnimationEnabled'
  | 'diagnosticsEnabled'
  | 'diagnosticsVerbose'

const LOCAL_SETTINGS_FIELDS: LocalSettingsField[] = [
  'themeMode', 'appearanceMode', 'colorSource', 'dynamicColorEnabled',
  'selectedBuiltinThemeId', 'selectedPaletteId', 'locale', 'autoSaveEnabled',
  'editorFontSize', 'editorLineSpacingMultiplier', 'autoSaveDelayMs',
  'autoIndentEnabled', 'autoIndentWidth', 'editorTypingAnimationEnabled',
  'editorSmoothCursorEnabled', 'editorTypingAnimationDurationMs',
  'editorSmoothCursorDurationMs', 'aiEnabled', 'statsDeviceId',
  'editorCoordinatedTextCursorAnimationEnabled', 'diagnosticsEnabled',
  'diagnosticsVerbose'
]

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
  // 设置页显示生命周期：旧页面的异步加载结果不得覆盖新页面/新请求。
  private loadGeneration: number = 0
  private displaySessionActive: boolean = false

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
  @Trace builtinThemes: BuiltinThemeDto[] = []
  @Trace syncConfig: SyncConfig | null = null
  @Trace syncToken: string = ''
  @Trace syncRemoteUrl: string = ''
  @Trace syncBranch: string = 'main'
  @Trace syncUsername: string = ''
  @Trace syncAdvancedExpanded: boolean = false
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

  // ── LocalSettings 有序串行保存（Issue #1026） ──
  // 快速拖动字号/行距或连续切换主题时，saveSettings 只登记“还有最新请求”；
  // drain 每轮取走待保存字段和对应快照，在统一写队列内做字段级合并，
  // 避免多份异步写入乱序，也避免整份旧 DTO 覆盖其它页面刚写入的字段。
  private localSettingsSaveQueued: boolean = false
  private localSettingsSaveRunning: Promise<void> | null = null
  private localSettingsDirtyFields: Set<LocalSettingsField> = new Set<LocalSettingsField>()
  // 正在落盘的一轮字段。外部广播必须同时避开 dirty 和 inFlight，
  // 避免设置页等在途保存返回时，自己刚提交的字段被别的页面广播反向覆盖。
  private localSettingsInFlightFields: Set<LocalSettingsField> = new Set<LocalSettingsField>()

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

  /** 搜索结果定位到设置分类：只展开，不翻转用户已有状态。 */
  expandSection(section: SettingsSectionId): void {
    if (this.presentationStore.isSectionExpanded(section)) {
      return
    }
    this.presentationStore.toggleSection(section)
  }

  // ── 加载数据 ──

  /** 进入设置页：开启新的显示生命周期，之前未完成的加载全部失效。 */
  beginDisplaySession(): void {
    this.loadGeneration += 1
    this.displaySessionActive = true
  }

  /** 离开设置页：让在途请求失效，避免旧结果继续写 ViewModel 或全局主题/调度器。 */
  endDisplaySession(): void {
    this.loadGeneration += 1
    this.displaySessionActive = false
  }

  private isLoadCurrent(generation: number): boolean {
    return this.displaySessionActive && generation === this.loadGeneration
  }

  async loadData(): Promise<void> {
    const loadGeneration: number = ++this.loadGeneration
    this.isLoading = true

    try {
      const settingsResult = await this.bridge.getLocalSettings()
      const syncConfigResult = await this.bridge.loadSyncConfig()
      if (!this.isLoadCurrent(loadGeneration)) {
        return
      }

      try {
        const builtinResult = await this.bridge.listBuiltinThemes()
        if (!this.isLoadCurrent(loadGeneration)) {
          return
        }
        if (builtinResult.success && builtinResult.data) {
          this.builtinThemes = builtinResult.data
          this.themeAdapter.setBuiltinThemes(builtinResult.data)
        }
      } catch (_e) {
        if (!this.isLoadCurrent(loadGeneration)) {
          return
        }
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
          this.syncBranch = syncConfigResult.data.providerConfig.branch || 'main'
          this.syncUsername = syncConfigResult.data.providerConfig.username || ''
        }
        // Issue #925：把 Core 已提交的配置交给进程唯一自动同步调度器。
        // reconfigure 只记录配置、启停前台唤醒 timer；是否真正同步由 runIfDue 决定，
        // 打开设置页本身不会额外产生一笔同步。
        HarmonyAutoSyncScheduler.getInstance().reconfigure(syncConfigResult.data)
      }

      // 加载同步密钥，回填 token
      await this.loadSyncSecrets(loadGeneration)
    } catch (err) {
      if (this.isLoadCurrent(loadGeneration)) {
        hilog.error(DOMAIN, TAG, `Failed to load settings: ${err}`)
      }
    } finally {
      if (this.isLoadCurrent(loadGeneration)) {
        this.isLoading = false
      }
    }
  }

  // ── 保存设置 ──

  async saveSettings(): Promise<void> {
    // 没有字段变化时不需要保存；updateSettings 已记录本轮实际改动集合。
    if (this.localSettingsDirtyFields.size === 0) {
      return
    }
    this.localSettingsSaveQueued = true
    this.isSaving = true
    if (this.localSettingsSaveRunning === null) {
      this.localSettingsSaveRunning = this.drainLocalSettingsSaves()
    }
    const running: Promise<void> | null = this.localSettingsSaveRunning
    if (running !== null) {
      await running
    }
  }

  /**
   * 队列排空循环：每轮取走当前待保存字段和快照；保存期间的新改动进入下一轮。
   * 先取走字段再清空，保证在途请求不会吞掉后续滑块/开关产生的新改动。
   */
  private async drainLocalSettingsSaves(): Promise<void> {
    try {
      while (this.localSettingsSaveQueued) {
        this.localSettingsSaveQueued = false
        if (this.localSettingsDirtyFields.size === 0) {
          continue
        }
        const fields: Set<LocalSettingsField> =
          new Set<LocalSettingsField>(this.localSettingsDirtyFields)
        this.localSettingsDirtyFields.clear()
        this.localSettingsInFlightFields = new Set<LocalSettingsField>(fields)
        const snapshot: LocalSettings = this.snapshotLocalSettings()
        try {
          await this.persistLocalSettings(snapshot, fields)
        } finally {
          this.localSettingsInFlightFields.clear()
        }
      }
    } finally {
      this.localSettingsSaveRunning = null
      this.isSaving = false
    }
  }

  /** 独立快照：updateSettings 会原地修改 this.settings，落盘必须使用提交时冻结的值。 */
  private snapshotLocalSettings(): LocalSettings {
    const s: LocalSettings = this.settings
    return {
      themeMode: s.themeMode,
      appearanceMode: s.appearanceMode,
      colorSource: s.colorSource,
      dynamicColorEnabled: s.dynamicColorEnabled,
      selectedBuiltinThemeId: s.selectedBuiltinThemeId,
      selectedPaletteId: s.selectedPaletteId,
      locale: s.locale,
      autoSaveEnabled: s.autoSaveEnabled,
      editorFontSize: s.editorFontSize,
      editorLineSpacingMultiplier: s.editorLineSpacingMultiplier,
      autoSaveDelayMs: s.autoSaveDelayMs,
      autoIndentEnabled: s.autoIndentEnabled,
      autoIndentWidth: s.autoIndentWidth,
      editorTypingAnimationEnabled: s.editorTypingAnimationEnabled,
      editorSmoothCursorEnabled: s.editorSmoothCursorEnabled,
      editorTypingAnimationDurationMs: s.editorTypingAnimationDurationMs,
      editorSmoothCursorDurationMs: s.editorSmoothCursorDurationMs,
      aiEnabled: s.aiEnabled,
      statsDeviceId: s.statsDeviceId,
      editorCoordinatedTextCursorAnimationEnabled: s.editorCoordinatedTextCursorAnimationEnabled,
      diagnosticsEnabled: s.diagnosticsEnabled,
      diagnosticsVerbose: s.diagnosticsVerbose
    }
  }

  /** 显式比较字段；null 与空字符串不同，作为“未修改/改为空”的边界。 */
  private localSettingsFieldEquals(
    field: LocalSettingsField,
    left: LocalSettings,
    right: LocalSettings,
  ): boolean {
    switch (field) {
      case 'themeMode': return left.themeMode === right.themeMode
      case 'appearanceMode': return left.appearanceMode === right.appearanceMode
      case 'colorSource': return left.colorSource === right.colorSource
      case 'dynamicColorEnabled': return left.dynamicColorEnabled === right.dynamicColorEnabled
      case 'selectedBuiltinThemeId': return left.selectedBuiltinThemeId === right.selectedBuiltinThemeId
      case 'selectedPaletteId': return left.selectedPaletteId === right.selectedPaletteId
      case 'locale': return left.locale === right.locale
      case 'autoSaveEnabled': return left.autoSaveEnabled === right.autoSaveEnabled
      case 'editorFontSize': return left.editorFontSize === right.editorFontSize
      case 'editorLineSpacingMultiplier':
        return left.editorLineSpacingMultiplier === right.editorLineSpacingMultiplier
      case 'autoSaveDelayMs': return left.autoSaveDelayMs === right.autoSaveDelayMs
      case 'autoIndentEnabled': return left.autoIndentEnabled === right.autoIndentEnabled
      case 'autoIndentWidth': return left.autoIndentWidth === right.autoIndentWidth
      case 'editorTypingAnimationEnabled':
        return left.editorTypingAnimationEnabled === right.editorTypingAnimationEnabled
      case 'editorSmoothCursorEnabled':
        return left.editorSmoothCursorEnabled === right.editorSmoothCursorEnabled
      case 'editorTypingAnimationDurationMs':
        return left.editorTypingAnimationDurationMs === right.editorTypingAnimationDurationMs
      case 'editorSmoothCursorDurationMs':
        return left.editorSmoothCursorDurationMs === right.editorSmoothCursorDurationMs
      case 'aiEnabled': return left.aiEnabled === right.aiEnabled
      case 'statsDeviceId': return left.statsDeviceId === right.statsDeviceId
      case 'editorCoordinatedTextCursorAnimationEnabled':
        return left.editorCoordinatedTextCursorAnimationEnabled ===
          right.editorCoordinatedTextCursorAnimationEnabled
      case 'diagnosticsEnabled': return left.diagnosticsEnabled === right.diagnosticsEnabled
      case 'diagnosticsVerbose': return left.diagnosticsVerbose === right.diagnosticsVerbose
      default: return true
    }
  }

  /** 只把指定字段从 source 写入 target，保留 target 中其它页面/入口刚落盘的最新值。 */
  private applyLocalSettingsFields(
    target: LocalSettings,
    source: LocalSettings,
    fields: Set<LocalSettingsField>,
  ): LocalSettings {
    if (fields.has('themeMode')) target.themeMode = source.themeMode
    if (fields.has('appearanceMode')) target.appearanceMode = source.appearanceMode
    if (fields.has('colorSource')) target.colorSource = source.colorSource
    if (fields.has('dynamicColorEnabled')) target.dynamicColorEnabled = source.dynamicColorEnabled
    if (fields.has('selectedBuiltinThemeId')) target.selectedBuiltinThemeId = source.selectedBuiltinThemeId
    if (fields.has('selectedPaletteId')) target.selectedPaletteId = source.selectedPaletteId
    if (fields.has('locale')) target.locale = source.locale
    if (fields.has('autoSaveEnabled')) target.autoSaveEnabled = source.autoSaveEnabled
    if (fields.has('editorFontSize')) target.editorFontSize = source.editorFontSize
    if (fields.has('editorLineSpacingMultiplier')) {
      target.editorLineSpacingMultiplier = source.editorLineSpacingMultiplier
    }
    if (fields.has('autoSaveDelayMs')) target.autoSaveDelayMs = source.autoSaveDelayMs
    if (fields.has('autoIndentEnabled')) target.autoIndentEnabled = source.autoIndentEnabled
    if (fields.has('autoIndentWidth')) target.autoIndentWidth = source.autoIndentWidth
    if (fields.has('editorTypingAnimationEnabled')) {
      target.editorTypingAnimationEnabled = source.editorTypingAnimationEnabled
    }
    if (fields.has('editorSmoothCursorEnabled')) {
      target.editorSmoothCursorEnabled = source.editorSmoothCursorEnabled
    }
    if (fields.has('editorTypingAnimationDurationMs')) {
      target.editorTypingAnimationDurationMs = source.editorTypingAnimationDurationMs
    }
    if (fields.has('editorSmoothCursorDurationMs')) {
      target.editorSmoothCursorDurationMs = source.editorSmoothCursorDurationMs
    }
    if (fields.has('aiEnabled')) target.aiEnabled = source.aiEnabled
    if (fields.has('statsDeviceId')) target.statsDeviceId = source.statsDeviceId
    if (fields.has('editorCoordinatedTextCursorAnimationEnabled')) {
      target.editorCoordinatedTextCursorAnimationEnabled = source.editorCoordinatedTextCursorAnimationEnabled
    }
    if (fields.has('diagnosticsEnabled')) target.diagnosticsEnabled = source.diagnosticsEnabled
    if (fields.has('diagnosticsVerbose')) target.diagnosticsVerbose = source.diagnosticsVerbose
    return target
  }

  /**
   * 把一份落盘结果按字段合并回 UI，跳过 protectedFields。
   * 合并前重新冻结当前内存值，保证调用期间用户刚改的草稿不被旧 DTO 覆盖。
   */
  private mergeLocalSettings(
    persisted: LocalSettings,
    protectedFields: Set<LocalSettingsField>,
  ): void {
    const safeFields: Set<LocalSettingsField> = new Set<LocalSettingsField>()
    for (const field of LOCAL_SETTINGS_FIELDS) {
      if (!protectedFields.has(field)) {
        safeFields.add(field)
      }
    }
    const next: LocalSettings = this.snapshotLocalSettings()
    this.applyLocalSettingsFields(next, persisted, safeFields)
    this.settings = next
  }

  /**
   * 把本页自己保存返回的落盘结果合并回 UI：只保护尚待保存的 dirty 字段，
   * 本轮已提交字段允许回填，避免已落盘值被内存旧草稿长期占住。
   */
  private mergePersistedLocalSettings(persisted: LocalSettings): void {
    this.mergeLocalSettings(persisted, this.localSettingsDirtyFields)
  }

  /**
   * 外部页面（如 WritingScreen）落盘后的广播入口。
   * 保护 dirty ∪ inFlight：用户正在编辑或正在等待保存的字段不被反向覆盖，
   * 其余字段立即同步到设置页，保持两个入口显示同一份最新持久化设置。
   */
  applyPublishedLocalSettings(persisted: LocalSettings): void {
    const protectedFields: Set<LocalSettingsField> =
      new Set<LocalSettingsField>(this.localSettingsDirtyFields)
    for (const field of this.localSettingsInFlightFields) {
      protectedFields.add(field)
    }
    this.mergeLocalSettings(persisted, protectedFields)
  }

  /** 只持久化本轮字段和快照；广播、提示、主题都使用同一份落盘结果。 */
  private async persistLocalSettings(
    snapshot: LocalSettings,
    fields: Set<LocalSettingsField>,
  ): Promise<void> {
    try {
      const result = await this.bridge.updateLocalSettings(
        (latest: LocalSettings): LocalSettings => this.applyLocalSettingsFields(latest, snapshot, fields)
      )
      if (!result.success || !result.data) {
        this.saveMessage = `error:${result.errorCode || 'unknown'}`
        return
      }
      const persisted: LocalSettings = result.data
      this.saveMessage = 'saved'
      // Issue #825 复核8第 3 点：把刚落盘的那份设置广播出去。
      // updateLocalSettings 在统一写队列内读回真实持久化 DTO，Store 才代表磁盘真相。
      LocalSettingsStore.getInstance().publish(persisted)
      this.mergePersistedLocalSettings(persisted)
      // Issue #911：诊断开关只在 Core 真正落盘、且重新读回成功后，才比较读回的持久化值
      // 与上次同步值；只有 setDiagnosticsConfig 成功才更新 lastSynced。
      const persistedEnabled: boolean = persisted.diagnosticsEnabled
      const persistedVerbose: boolean = persisted.diagnosticsVerbose
      if (persistedEnabled !== this.lastSyncedDiagnosticsEnabled ||
        persistedVerbose !== this.lastSyncedDiagnosticsVerbose) {
        const syncResult = await this.bridge.setDiagnosticsConfig(persistedEnabled, persistedVerbose)
        if (syncResult.success) {
          this.lastSyncedDiagnosticsEnabled = persistedEnabled
          this.lastSyncedDiagnosticsVerbose = persistedVerbose
        } else {
          hilog.warn(DOMAIN, TAG, `setDiagnosticsConfig failed on save: ${syncResult.errorCode || 'unknown'}`)
        }
      }
      // 主题与排版只应用这份落盘结果，不读可能已被后续操作改写的 this.settings。
      const appearanceMode = persisted.appearanceMode || persisted.themeMode || 'system'
      this.themeAdapter.updateFromLocalSettings(
        appearanceMode,
        persisted.selectedBuiltinThemeId || ''
      )
      this.themeAdapter.updateFromSettings({
        theme: appearanceMode as ThemeMode,
        fontSize: persisted.editorFontSize,
        fontFamily: 'HarmonyOS Sans',
        lineHeight: persisted.editorLineSpacingMultiplier
      } as ThemeUpdateParams)
    } catch (err) {
      hilog.error(DOMAIN, TAG, `Failed to save settings: ${err}`)
      this.saveMessage = 'error:save_failed'
    }
  }

  // ── 执行同步 ──
  // 同步运行状态和结果由 SyncRunPresentationState 统一持有；ViewModel 只准备凭据并触发 Controller。
  // Issue #952：返回本次请求收到的 SyncOutcome，供设置页入口在与后台自动同步重叠时自行反馈结果。
  async performSync(): Promise<SyncOutcome> {
    // Issue #924：若 Token 还在防抖等待/队列中，先落盘再同步，避免用旧凭据发起同步。
    if (this.isSyncSecretsSavePending()) {
      await this.saveSyncSecrets()
    }
    return await SyncOperationController.getInstance().performSync()
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
    // Issue #1026：按字段记录本次实际改动，保存时只提交这些字段；
    // 否则设置浮层会把内存里的旧整份 DTO 写回，覆盖写作页刚保存的字号/行距。
    const before: LocalSettings = this.snapshotLocalSettings()
    this.settings = updater(this.settings)
    const after: LocalSettings = this.snapshotLocalSettings()
    for (const field of LOCAL_SETTINGS_FIELDS) {
      if (!this.localSettingsFieldEquals(field, before, after)) {
        this.localSettingsDirtyFields.add(field)
      }
    }
  }

  // ── 同步配置更新与保存 ──

  /**
   * 更新 syncConfig 并排队保存到 Core（Issue #925：有序串行保存，保存期间合并重复变更）。
   * 新配置统一使用通用 `git_https`；旧 `github` 配置原样保留，由 Core 统一选路。
   * 只有真正落盘成功的配置才会 reconfigure 自动同步调度器；不 await（UI 回调不阻塞），
   * 保存结果由队列内部感知。
   */
  updateSyncConfig(config: SyncConfig): void {
    this.syncConfig = config
    this.saveSyncConfig()
  }

  /** 新表单只展示仓库地址与可选令牌；分支/用户名属于高级设置。 */
  private makeGitHttpsProviderConfig(remoteUrl: string): ProviderConfig {
    return {
      type: 'git_https',
      remote_url: remoteUrl,
      branch: this.syncBranch.trim().length > 0 ? this.syncBranch.trim() : 'main',
      username: this.syncUsername.trim(),
      transport: null
    }
  }

  private saveProviderConfig(providerConfig: ProviderConfig): void {
    if (!this.syncConfig) {
      return
    }
    const next: SyncConfig = {
      enabled: this.syncConfig.enabled,
      activeProvider: 'git_https',
      providerConfig: providerConfig,
      autoSync: this.syncConfig.autoSync,
      syncIntervalSeconds: this.syncConfig.syncIntervalSeconds,
      hasNetworkPermission: this.syncConfig.hasNetworkPermission,
      hasNetworkStatePermission: this.syncConfig.hasNetworkStatePermission
    }
    this.syncConfig = next
    this.saveSyncConfig()
  }

  /**
   * 更新远程仓库 HTTPS 地址。旧 GitHub 配置在首次编辑后迁移为通用 Git HTTPS 配置，
   * Core 仍会对 github.com 仓库自动探测并在不可达时转 API。
   */
  updateSyncRemoteUrl(value: string): void {
    this.syncRemoteUrl = value
    if (this.syncConfig) {
      const existingProviderConfig: ProviderConfig | null = this.syncConfig.providerConfig
      if (existingProviderConfig && this.syncBranch.trim().length === 0) {
        this.syncBranch = existingProviderConfig.branch || 'main'
      }
      if (existingProviderConfig && this.syncUsername.length === 0) {
        this.syncUsername = existingProviderConfig.username || ''
      }
      this.saveProviderConfig(this.makeGitHttpsProviderConfig(value))
    }
  }

  /** 高级设置：远端分支，默认 main。 */
  updateSyncBranch(value: string): void {
    this.syncBranch = value
    if (this.syncConfig) {
      this.saveProviderConfig(this.makeGitHttpsProviderConfig(this.syncRemoteUrl))
    }
  }

  /** 高级设置：HTTPS Basic 用户名；留空时由 Core 按服务器要求自动选择。 */
  updateSyncUsername(value: string): void {
    this.syncUsername = value
    if (this.syncConfig) {
      this.saveProviderConfig(this.makeGitHttpsProviderConfig(this.syncRemoteUrl))
    }
  }

  toggleSyncAdvanced(): void {
    this.syncAdvancedExpanded = !this.syncAdvancedExpanded
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
  async loadSyncSecrets(loadGeneration?: number): Promise<void> {
    try {
      const result = await this.bridge.loadSyncSecrets()
      if (loadGeneration !== undefined && !this.isLoadCurrent(loadGeneration)) {
        return
      }
      if (result.success && result.data) {
        if (result.data.providerSecrets) {
          const secrets = result.data.providerSecrets
          if (secrets.type === 'git_https') {
            this.syncToken = secrets.secret ?? ''
          } else {
            this.syncToken = secrets.token ?? ''
          }
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
          type: 'git_https',
          token: null,
          secret: token
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
