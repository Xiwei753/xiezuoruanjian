// SettingsContent — 设置内容（宽屏浮层与窄屏路由页共用）。
// Issue #825：把设置内容、搜索、分组折叠布局从 SettingsScreen 抽出来，
// 让宽屏设置可以直接悬浮在打开前的页面之上，而不是再压一层路由页。
// Issue #972：宽屏浮层的外框、标题、返回与搜索由 WideSettingsOverlay 拥有；
// 本组件在 overlayLayout=true 时只绘制两列 WideSettingsSection 分类卡片，
// 搜索词由浮层通过 overlaySearchText 传入，不再自己画宽屏标题栏。
// - 本组件只负责内容与折叠状态，不持有导航栈；
// - 唯一关闭入口是 onRequestClose 回调，由外壳注入
//   （窄屏路由外壳注入 safePopWithStack，宽屏浮层注入 SettingsOverlayController.close()）。
// - 窄屏 / 中屏路由页（overlayLayout=false）保持 CompactSettingsHeader + CompactScreenLayout。
// 设置内容、保存逻辑、主题逻辑仍然全部走现有 SettingsViewModel 和 backend。

import { promptAction, ComponentContent } from '@kit.ArkUI'
import hilog from '@ohos.hilog'
import { LocalSettings, BuiltinThemeDto } from '../../../corebridge/dto/SettingsDtos'
import { FullSyncState, FullSyncReport, FullSyncReportFailure, SyncConfig, SyncDiagnosticsResult } from '../../../corebridge/dto/SyncDtos'
import { NativeSyncBridge } from '../../../corebridge/native/NativeSyncBridge'
import { NativeCoreModule } from '../../../corebridge/native/NativeCoreModule'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeObserverImpl } from '../../../ui/theme/HarmonyThemeAdapter'
import { ThemeMode } from '../../../ui/theme/HarmonyThemeTypes'
import { DesignTokens } from '../../../ui/theme/DesignTokens'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../../../ui/adaptive/AdaptiveContext'
import { MessageKeyMapper } from '../../../ui/i18n/MessageKeyMapper'
import { SyncOutcomePresenter } from '../presentation/SyncOutcomePresenter'
import { SyncOutcome } from '../presentation/SyncRunTypes'
import { SyncRunPresentationState, SyncRunSnapshot } from '../presentation/SyncRunPresentationState'
import { SyncOperationController } from '../presentation/SyncOperationController'
import { SettingsViewModel, SettingsSectionId } from '../presentation/SettingsViewModel'
import { DeviceSyncViewModel } from '../presentation/DeviceSyncViewModel'
import { TrustedDeviceInfo } from '../../../platform/device/TrustedDeviceService'
import { SettingsSectionContext } from '../presentation/SettingsSectionContext'
import { SyncConflictViewModel } from '../presentation/SyncConflictViewModel'
import { AppContinuationService } from '../../../platform/continuity/AppContinuationService'
import { ContinuationPayload } from '../../../platform/continuity/ContinuationPayload'
import { createSyncConflictsPanelContent, SyncConflictsPanelArgs } from './SyncConflictsPanel'
// Issue #958：内置模型第三方资源（来源声明 + 许可全文）面板工厂。
import { createThirdPartyNoticesPanelContent, ThirdPartyNoticesPanelArgs } from './ThirdPartyNoticesPanel'
// Issue #932：窄屏页面状态（加载中 / 出错重试 / 空）复用公共组件。
import { CompactPageState, CompactPageStateKind } from '../../../ui/components/CompactPageState'
import { GripAdaptiveToggleRow } from '../../../ui/components/GripAdaptiveToggleRow'
// Issue #972：宽屏浮层的两列分类卡片。
import { WideSettingsSection } from './wide/WideSettingsSection'
import { WideSettingsCategoryContent } from './wide/WideSettingsCategoryContent'
// Issue #986：智感握姿 → 设置开关所在侧。
import { GripLayoutSide, GripLayoutSideStore } from '../../../ui/adaptive/GripLayoutSide'

const TAG = 'SettingsContent'
const DOMAIN = 0x0000

// ── 设置分组标识（窄屏折叠布局用） ──
type SettingsGroupId = 'appearance' | 'writing' | 'data_sync' | 'advanced' | 'about'

// ── 分组与分类的映射关系（对齐 Android SettingsRoute.kt 信息架构） ──
interface GroupDefinition {
  id: SettingsGroupId
  sections: SettingsSectionId[]
}

const GROUP_DEFINITIONS: GroupDefinition[] = [
  { id: 'appearance', sections: ['appearance'] },
  { id: 'writing', sections: ['editor', 'save'] },
  { id: 'data_sync', sections: ['sync', 'recovery'] },
  { id: 'advanced', sections: ['ai', 'diagnostics', 'laboratory'] },
  { id: 'about', sections: ['about'] }
]

// Issue #982：Core 设置索引里的 section（save_sync）与平台分类（save / sync）不同名，
// 统一在这里折算；未知值返回空串，不再把非法字符串强转成 SettingsSectionId。
function normalizeSettingsSectionId(section: string): SettingsSectionId | null {
  if (section === 'appearance' || section === 'editor' || section === 'save' ||
    section === 'sync' || section === 'recovery' || section === 'ai' ||
    section === 'diagnostics' || section === 'laboratory' || section === 'about') {
    return section
  }
  if (section === 'save_sync') {
    return 'save'
  }
  return null
}

// Issue #982：settingKey → 实际存在的设置分类。旧同步别名（fontSize/themeMode）
// 指到当前平台真实控件，不额外造第二套设置项。
// Issue #1005：设备配色 / Monet 不再是 HarmonyOS 设置项。
function resolveSettingSectionForKey(settingKey: string): SettingsSectionId | null {
  switch (settingKey) {
    case 'appearanceMode':
    case 'themeMode':
    case 'selectedBuiltinThemeId':
    case 'locale':
    case 'editorFontSize':
    case 'fontSize':
    case 'editorLineSpacingMultiplier':
      return 'appearance'
    case 'autoIndentEnabled':
    case 'autoIndentWidth':
    case 'editorTypingAnimationEnabled':
    case 'editorSmoothCursorEnabled':
    case 'editorTypingAnimationDurationMs':
    case 'editorSmoothCursorDurationMs':
    case 'editorCoordinatedTextCursorAnimationEnabled':
      return 'editor'
    case 'autoSaveEnabled':
    case 'autoSaveDelayMs':
      return 'save'
    case 'aiEnabled':
      return 'ai'
    case 'diagnosticsEnabled':
    case 'diagnosticsVerbose':
    case 'statsDeviceId':
      return 'diagnostics'
    default:
      return null
  }
}

const SETTING_HIGHLIGHT_DURATION_MS: number = 1600

// 同一时刻可能有多份 SettingsContent（例如路由页 + 浮层/隐藏页面），
// 组件 id 加实例序号，避免 getRectangleById 命中到不可见的那一份。
let settingsContentInstanceSeq: number = 0

// 旧同步键 / Core 别名 → 平台当前真实控件 id。
// 定位和高亮必须落在真实控件上，不能拿别名去找一个不存在的设置项。
function resolveSettingItemAnchorKey(settingKey: string): string {
  if (settingKey === 'themeMode') {
    return 'appearanceMode'
  }
  if (settingKey === 'fontSize') {
    return 'editorFontSize'
  }
  return settingKey
}

// ── 下拉选择选项类型 ──
interface SettingsSelectOption {
  value: string
  icon: string
}

@ComponentV2
export struct SettingsContent {
  // Issue #825：唯一的关闭入口回调。内容组件自己不碰导航栈。
  @Param onRequestClose: () => void = () => {}
  // Issue #825：是否由宽屏浮层承载。
  // Issue #972：浮层外框、标题和搜索由 WideSettingsOverlay 拥有；本组件只画两列卡片主体。
  @Param overlayLayout: boolean = false
  // Issue #972：宽屏浮层的搜索词由浮层持有并传入；窄屏 overlayLayout=false 时仍用本地 searchText。
  @Param overlaySearchText: string = ''
  // Issue #982：设置页顶部搜索统一打开全局搜索，只传 originScreen=settings + 当前分类。
  @Param onOpenGlobalSearch: (section: string) => void = (_section: string): void => {}
  // 搜索结果跳到具体设置分类时由路由/浮层传入。
  @Param initialSection: string = ''
  // 设置页可能从星图/统计 tab 的搜索进入，快照必须记录真实 tab，不能硬编码 Home。
  @Param screenTabIndex: number = 0
  // Issue #982：搜索结果跳到具体设置项；与 initialSection 一起构成一次性定位意图。
  @Param initialSettingKey: string = ''

  // 问题9：ViewModel — 所有 Core/NAPI 调用和状态管理都在这里（@ObservedV2 + @Trace）
  @Local viewModel: SettingsViewModel = new SettingsViewModel()
  // Issue #988：附近设备同步与云备份是两个独立入口，各自维护状态；附近设备对象由 ETS 对象库合并。
  @Local deviceSync: DeviceSyncViewModel = new DeviceSyncViewModel()

  // 页面 UI 状态（V2 @Local）
  @Local themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()
  @Local adaptiveState: AdaptiveState = getAdaptiveState()
  @Local searchText: string = ''
  // 问题5：复制设备信息结果提示
  @Local copyMessage: string = ''
  // Issue #926：冲突面板回传的剩余冲突数；-1 表示本次会话还没从 Core 拉取过。
  @Local conflictRemaining: number = -1
  // 同步运行态由 Controller 的共享 Store 提供，不再由设置页 ViewModel 保存副本。
  @Local syncRunSnapshot: SyncRunSnapshot = SyncRunPresentationState.getInstance().snapshot()
  // Core 持久化的最近一次全量同步事务状态；null 表示 Core 尚无记录。
  @Local fullSyncState: FullSyncState | null = null
  @Local fullSyncStateReadError: string = ''
  // Core 最近一次持久化的整轮报告；与当前进程运行快照分开显示。
  @Local latestFullSyncReport: FullSyncReport | null = null
  @Local latestFullSyncReportReadError: string = ''
  // Issue #952：同步失败明细是否展开；默认收起，避免报告挤占设置列表。
  @Local syncDetailsExpanded: boolean = false
  // Issue #982：精确定位后短暂高亮的设置项 key。
  @Local highlightedSettingKey: string = ''
  // Issue #986：智感握姿决定 Switch 所在侧；默认右侧，双手/放下设备保持上一次明确握持侧。
  @Local gripSide: GripLayoutSide = 'right'

  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private themeObserver: ThemeObserver | null = null
  private adaptiveObserver: AdaptiveObserver | null = null
  private sectionContextListener: ((section: string) => void) | null = null
  private locateContextListener: ((section: string, settingKey: string) => void) | null = null
  private pendingLocateSection: string = ''
  private pendingLocateKey: string = ''
  private pendingRequestedSettingKey: string = ''
  private locateTimerId: number = -1
  private locateAttempt: number = 0
  private highlightTimerId: number = -1
  private narrowScroller: Scroller = new Scroller()
  private wideColumnScroller0: Scroller = new Scroller()
  private wideColumnScroller1: Scroller = new Scroller()
  private instanceToken: string = ''
  private syncRunPresentation: SyncRunPresentationState = SyncRunPresentationState.getInstance()
  private unsubscribeSyncRun: (() => void) | null = null
  // Issue #986：握姿侧订阅句柄。
  private unsubscribeGrip: (() => void) | null = null
  private syncStateBridge: NativeSyncBridge = new NativeSyncBridge(new NativeCoreModule())
  // Issue #926：冲突面板（窗口层级对话框）句柄。
  private conflictsPanelContent: ComponentContent<SyncConflictsPanelArgs> | null = null
  // Issue #958：内置模型第三方资源面板（窗口层级对话框）句柄。
  private thirdPartyNoticesPanelContent: ComponentContent<ThirdPartyNoticesPanelArgs> | null = null
  // Issue #926：入口计数探针——同步结果带冲突时从 Core 全局列表取剩余冲突数，
  // 与面板同一数据源，保证"N 处"和面板列表一致。
  private conflictProbe: SyncConflictViewModel = new SyncConflictViewModel()

  aboutToAppear() {
    settingsContentInstanceSeq++
    this.instanceToken = String(settingsContentInstanceSeq)
    // Issue #982：搜索结果跳转到具体设置分类时展开该分类；普通进入不改变用户折叠状态。
    // 有精确 settingKey 时以真实控件所在分类为准（Core 索引的 section 可能仍是旧分类名）。
    const keySection: SettingsSectionId | null = this.initialSettingKey.length > 0
      ? resolveSettingSectionForKey(this.initialSettingKey)
      : null
    const normalizedInitialSection: SettingsSectionId | null =
      keySection !== null ? keySection : normalizeSettingsSectionId(this.initialSection)
    if (normalizedInitialSection !== null) {
      SettingsSectionContext.setCurrentSection(normalizedInitialSection)
      this.viewModel.expandSection(normalizedInitialSection)
    }
    // 搜索从设置页发起后点击设置结果：不重建设置页，只展开目标分类。
    this.sectionContextListener = (section: string): void => {
      const normalized: SettingsSectionId | null = normalizeSettingsSectionId(section)
      if (normalized !== null) {
        this.viewModel.expandSection(normalized)
      }
    }
    SettingsSectionContext.subscribe(this.sectionContextListener)
    // Issue #982：精确设置项定位；列表数据加载完成前只记下意图，加载后再滚动/高亮。
    this.locateContextListener = (section: string, settingKey: string): void => {
      this.acceptLocateRequest(section, settingKey)
    }
    SettingsSectionContext.subscribeLocate(this.locateContextListener)
    const pendingLocate = SettingsSectionContext.peekPendingLocate()
    if (pendingLocate !== null) {
      this.acceptLocateRequest(pendingLocate.section, pendingLocate.settingKey)
    } else if (this.initialSettingKey.length > 0) {
      this.acceptLocateRequest(this.initialSection, this.initialSettingKey)
    }
    // 注册主题观察者
    this.themeObserver = new ThemeObserverImpl((theme: ThemeConfig) => {
      this.themeConfig = theme
    })
    this.themeAdapter.addObserver(this.themeObserver!)

    // 注册响应式布局观察者
    this.adaptiveObserver = new SimpleAdaptiveObserver((state: AdaptiveState) => {
      this.adaptiveState = state
    })
    AdaptiveContext.getInstance().addObserver(this.adaptiveObserver)
    this.adaptiveState = AdaptiveContext.getInstance().getCurrentState()

    // Issue #986：握姿换手只改 Switch 排列侧，不触发设置读写。
    this.unsubscribeGrip = GripLayoutSideStore.getInstance().subscribe((side: GripLayoutSide): void => {
      this.gripSide = side
    })

    // Issue #952：设置页订阅共享运行态，保证“同步”分类展开时看到的是最新一轮结果；
    // 摘要只在分类内被动展示，不驱动任何主动弹窗。
    this.syncRunSnapshot = this.syncRunPresentation.snapshot()
    this.unsubscribeSyncRun = this.syncRunPresentation.subscribe((snapshot: SyncRunSnapshot): void => {
      const previousPhase: string = this.syncRunSnapshot.phase
      this.syncRunSnapshot = snapshot
      if (previousPhase === 'in_progress' && this.syncRunSnapshot.phase !== 'in_progress') {
        void this.refreshFullSyncState()
        void this.refreshLatestFullSyncReport()
      }
    })

    // 先让设置页沿用既有 Core 读取路径完成加载，再读取 Core 的持久化同步状态。
    this.deviceSync.refresh()
    void this.loadSettingsAndSyncState()

    // Issue #926：打开设置页时从 Core 全局冲突列表取一次真实剩余数，冲突入口据此显示。
    void this.refreshConflictProbe()
  }

  aboutToDisappear() {
    if (this.sectionContextListener !== null) {
      SettingsSectionContext.unsubscribe(this.sectionContextListener)
      this.sectionContextListener = null
    }
    if (this.locateContextListener !== null) {
      SettingsSectionContext.unsubscribeLocate(this.locateContextListener)
      this.locateContextListener = null
    }
    if (this.locateTimerId !== -1) {
      clearTimeout(this.locateTimerId)
      this.locateTimerId = -1
    }
    if (this.highlightTimerId !== -1) {
      clearTimeout(this.highlightTimerId)
      this.highlightTimerId = -1
    }
    if (this.themeObserver) {
      this.themeAdapter.removeObserver(this.themeObserver)
    }
    if (this.adaptiveObserver) {
      AdaptiveContext.getInstance().removeObserver(this.adaptiveObserver)
    }
    if (this.unsubscribeSyncRun !== null) {
      this.unsubscribeSyncRun()
      this.unsubscribeSyncRun = null
    }
    if (this.unsubscribeGrip !== null) {
      this.unsubscribeGrip()
      this.unsubscribeGrip = null
    }
    // Issue #926：页面销毁时收起还开着的冲突面板（释放由 onDidDisappear 统一回收）。
    this.closeConflictsPanel()
  }

  private async loadSettingsAndSyncState(): Promise<void> {
    await this.viewModel.loadData()
    // Issue #982：数据到位后再尝试精确设置项定位，避免在静态默认值上滚动/高亮。
    this.tryApplyPendingLocate()
    await Promise.all([this.refreshFullSyncState(), this.refreshLatestFullSyncReport()])
  }

  // ── Issue #982：搜索结果精确设置项定位 ──

  // Issue #988：设置页离开/接续时保留用户最后点击的分类和设置项。
  private recordContinuationSection(section: string, settingKey: string): void {
    const payload: ContinuationPayload = {
      screen: 'settings',
      tabIndex: this.screenTabIndex,
      settingsSection: section
    }
    if (settingKey.length > 0) {
      payload.settingKey = settingKey
    }
    AppContinuationService.getInstance().updateCurrentScreen(payload)
  }

  private acceptLocateRequest(section: string, settingKey: string): void {
    if (settingKey.length === 0) {
      // 新的“只跳分类”请求取消尚未完成的精确项定位。
      this.pendingLocateSection = ''
      this.pendingLocateKey = ''
      this.pendingRequestedSettingKey = ''
      this.locateAttempt = 0
      if (this.locateTimerId !== -1) {
        clearTimeout(this.locateTimerId)
        this.locateTimerId = -1
      }
      const normalized: SettingsSectionId | null = normalizeSettingsSectionId(section)
      if (normalized !== null) {
        SettingsSectionContext.setCurrentSection(normalized)
        this.viewModel.expandSection(normalized)
        this.recordContinuationSection(normalized, '')
      }
      return
    }
    const targetSection: SettingsSectionId | null = resolveSettingSectionForKey(settingKey)
    if (targetSection === null) {
      // 设置项已移除或 Core 索引与平台控件不一致：明确反馈，不只打开分类。
      SettingsSectionContext.consumePendingLocate(settingKey)
      this.showLocateToast(`未找到设置项：${settingKey}`)
      return
    }
    this.pendingLocateSection = targetSection
    this.pendingRequestedSettingKey = settingKey
    this.pendingLocateKey = resolveSettingItemAnchorKey(settingKey)
    this.locateAttempt = 0
    SettingsSectionContext.setCurrentSection(targetSection)
    this.viewModel.expandSection(targetSection)
    this.recordContinuationSection(targetSection, settingKey)
    if (!this.viewModel.isLoading) {
      this.scheduleLocateApply(30)
    }
  }

  private tryApplyPendingLocate(): void {
    if (this.pendingLocateKey.length === 0 || this.viewModel.isLoading) {
      return
    }
    this.scheduleLocateApply(30)
  }

  private scheduleLocateApply(delayMs: number): void {
    if (this.pendingLocateKey.length === 0) {
      return
    }
    if (this.locateTimerId !== -1) {
      clearTimeout(this.locateTimerId)
    }
    this.locateTimerId = setTimeout((): void => {
      this.locateTimerId = -1
      this.applyLocateNow()
    }, delayMs)
  }

  private applyLocateNow(): void {
    const settingKey: string = this.pendingLocateKey
    if (settingKey.length === 0 || this.viewModel.isLoading) {
      return
    }
    const targetSection: SettingsSectionId | null = resolveSettingSectionForKey(settingKey)
    if (targetSection === null) {
      this.failLocate(settingKey)
      return
    }
    SettingsSectionContext.setCurrentSection(targetSection)
    this.viewModel.expandSection(targetSection)
    if (this.scrollToSettingItem(settingKey)) {
      this.finishLocate(settingKey)
      return
    }
    // 展开动画 / 首次布局可能还没产出目标节点，允许有限次重试后再反馈失败。
    this.locateAttempt++
    if (this.locateAttempt < 4) {
      this.scheduleLocateApply(80)
    } else {
      this.failLocate(settingKey)
    }
  }

  private finishLocate(settingKey: string): void {
    const requestedKey: string = this.pendingRequestedSettingKey
    this.pendingLocateSection = ''
    this.pendingLocateKey = ''
    this.pendingRequestedSettingKey = ''
    this.locateAttempt = 0
    SettingsSectionContext.consumePendingLocate(requestedKey)
    this.highlightedSettingKey = settingKey
    if (this.highlightTimerId !== -1) {
      clearTimeout(this.highlightTimerId)
    }
    this.highlightTimerId = setTimeout((): void => {
      this.highlightTimerId = -1
      if (this.highlightedSettingKey === settingKey) {
        this.highlightedSettingKey = ''
      }
    }, SETTING_HIGHLIGHT_DURATION_MS)
  }

  private failLocate(settingKey: string): void {
    const requestedKey: string = this.pendingRequestedSettingKey.length > 0
      ? this.pendingRequestedSettingKey
      : settingKey
    this.pendingLocateSection = ''
    this.pendingLocateKey = ''
    this.pendingRequestedSettingKey = ''
    this.locateAttempt = 0
    SettingsSectionContext.consumePendingLocate(requestedKey)
    this.showLocateToast(`未找到设置项：${requestedKey}`)
  }

  private showLocateToast(message: string): void {
    try {
      promptAction.showToast({ message: message })
    } catch (_e) {
      // Toast 失败不影响分类展开本身。
    }
  }

  private settingItemId(settingKey: string): string {
    return `setting_${this.instanceToken}_${settingKey}`
  }

  private settingHighlightColor(settingKey: string): ResourceColor {
    return this.highlightedSettingKey === settingKey
      ? $r('sys.color.comp_background_gray')
      : Color.Transparent
  }

  /**
   * 滚动到当前已渲染的设置项。窄屏用唯一 Scroll；宽屏按分类所在列选择对应 Scroller。
   * 目标还没渲染（分类刚展开）时返回 false，由调用方重试。
   */
  private scrollToSettingItem(settingKey: string): boolean {
    try {
      const componentUtils = this.getUIContext().getComponentUtils()
      const targetRect = componentUtils.getRectangleById(this.settingItemId(settingKey))
      if (targetRect.size.width <= 0 || targetRect.size.height <= 0) {
        return false
      }
      let viewportId: string = `settings_${this.instanceToken}_narrow_scroll`
      let scroller: Scroller = this.narrowScroller
      if (this.overlayLayout) {
        const section: SettingsSectionId | null = resolveSettingSectionForKey(settingKey)
        const columnIndex: number = section !== null &&
          this.overlayColumnSections(0).indexOf(section) >= 0 ? 0 : 1
        viewportId = `settings_${this.instanceToken}_wide_scroll_${columnIndex}`
        scroller = columnIndex === 0 ? this.wideColumnScroller0 : this.wideColumnScroller1
      }
      const viewportRect = componentUtils.getRectangleById(viewportId)
      if (viewportRect.size.height <= 0) {
        return false
      }
      const targetCenterY: number = targetRect.windowOffset.y + targetRect.size.height / 2
      const viewportCenterY: number = viewportRect.windowOffset.y + viewportRect.size.height / 2
      const deltaVp: number = this.getUIContext().px2vp(targetCenterY - viewportCenterY)
      if (Math.abs(deltaVp) > 1) {
        scroller.scrollBy(0, deltaVp)
      }
      return true
    } catch (_e) {
      return false
    }
  }

  private async refreshFullSyncState(): Promise<void> {
    try {
      const result = await this.syncStateBridge.loadFullSyncState()
      if (!result.success) {
        this.fullSyncState = null
        this.fullSyncStateReadError = result.errorCode ?? 'UNKNOWN'
        return
      }
      this.fullSyncState = result.data ?? null
      this.fullSyncStateReadError = ''
    } catch (_e) {
      this.fullSyncState = null
      this.fullSyncStateReadError = 'NATIVE_ERROR'
    }
  }

  private async refreshLatestFullSyncReport(): Promise<void> {
    try {
      const result = await this.syncStateBridge.loadLatestFullSyncReport()
      if (!result.success) {
        this.latestFullSyncReport = null
        this.latestFullSyncReportReadError = result.errorCode ?? 'UNKNOWN'
        return
      }
      this.latestFullSyncReport = result.data ?? null
      this.latestFullSyncReportReadError = ''
    } catch (_e) {
      this.latestFullSyncReport = null
      this.latestFullSyncReportReadError = 'NATIVE_ERROR'
    }
  }

  private formatSyncStateTime(epoch: number | null): string {
    if (epoch === null) {
      return '暂无'
    }
    // Core 当前以 Unix 秒传输；阈值兼容可能直接提供毫秒的未来版本。
    const epochMs: number = epoch < 100000000000 ? epoch * 1000 : epoch
    return new Date(epochMs).toLocaleString()
  }

  private fullSyncOverallStatusText(): string {
    if (this.fullSyncStateReadError.length > 0) {
      return `读取失败（${this.fullSyncStateReadError}）`
    }
    const state: FullSyncState | null = this.fullSyncState
    if (state === null) {
      return 'Core 尚无同步记录'
    }
    return state.overall_status
  }

  private fullSyncFailedTargetsText(): string {
    if (this.fullSyncStateReadError.length > 0) {
      return '失败目标：读取失败'
    }
    const state: FullSyncState | null = this.fullSyncState
    if (state === null) {
      return '失败目标：暂无记录'
    }
    if (state.failed_targets.length > 0) {
      return `失败目标：${state.failed_targets.length} 个`
    }
    return '失败目标：无'
  }

  private fullSyncAttemptTimeText(): string {
    if (this.fullSyncStateReadError.length > 0) {
      return '最近尝试：读取失败'
    }
    return `最近尝试：${this.formatSyncStateTime(this.fullSyncState?.last_attempt_time ?? null)}`
  }

  private fullSyncSuccessTimeText(): string {
    if (this.fullSyncStateReadError.length > 0) {
      return '最近成功：读取失败'
    }
    return `最近成功：${this.formatSyncStateTime(this.fullSyncState?.last_success_time ?? null)}`
  }

  private latestFullSyncReportSummaryText(): string {
    if (this.latestFullSyncReportReadError.length > 0) {
      return `最近整轮报告读取失败（${this.latestFullSyncReportReadError}）`
    }
    const report: FullSyncReport | null = this.latestFullSyncReport
    if (report === null) {
      return '最近整轮报告：暂无记录'
    }
    return `最近整轮报告：成功 ${report.completed_groups} 组，失败 ${report.failed_groups} 组，待处理 ${report.pending_groups} 组`
  }

  private fullSyncReportFailureReason(failure: FullSyncReportFailure): string {
    const context = getContext(this)
    if (failure.message_key.length > 0) {
      return MessageKeyMapper.resolveMessage(context, failure.message_key, undefined, failure.error_category)
    }
    const categoryMessage: string | undefined = MessageKeyMapper.resolveSyncErrorCategory(
      context, failure.error_category, undefined)
    if (categoryMessage !== undefined) {
      return categoryMessage
    }
    if (failure.error_category.length > 0) {
      return MessageKeyMapper.resolveMessage(context, failure.error_category, undefined, failure.error_category)
    }
    return '未分类'
  }

  // ── Issue #952：同步摘要只在“同步”分类内被动展示 ──
  // 自动同步照常运行并记录到进程快照与 Core 持久状态；这里只做被动查询显示，从不主动弹提示。
  // 摘要本身只列关键信息，失败明细由“查看同步详情”按需展开，避免挤占设置列表。

  private syncSourceLabel(source: string): string {
    return source === 'auto' ? '自动同步' : '手动同步'
  }

  @Builder
  SyncBriefSummary() {
    Column({ space: 4 }) {
      // 常态只显示一条最近状态；整轮持久状态与报告字段由用户展开查看。
      Text(this.syncBriefStatusText())
        .fontSize(13)
        .fontColor($r('sys.color.font_secondary'))
      this.SettingsNeutralAction(this.syncDetailsExpanded ? '收起同步详情' : '查看同步详情', () => {
        this.syncDetailsExpanded = !this.syncDetailsExpanded
      })
      if (this.syncDetailsExpanded) {
        Column({ space: 4 }) {
          Text(`历史结果：${this.fullSyncOverallStatusText()}`).fontSize(12)
          Text(this.fullSyncAttemptTimeText()).fontSize(12)
          Text(this.fullSyncSuccessTimeText()).fontSize(12)
          Text(this.fullSyncFailedTargetsText()).fontSize(12)
          Text(this.latestFullSyncReportSummaryText()).fontSize(12)
        }
        .width('100%')
        .alignItems(HorizontalAlign.Start)
        this.SyncReportDetails()
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
    .margin({ bottom: 16 })
  }

  private syncBriefStatusText(): string {
    if (this.syncRunSnapshot.phase === 'in_progress') {
      return `最近状态：${this.syncSourceLabel(this.syncRunSnapshot.source)}同步进行中…`
    }
    if (this.syncRunSnapshot.phase !== 'idle' && this.syncRunSnapshot.outcome !== null) {
      return `最近状态：${this.syncSourceLabel(this.syncRunSnapshot.source)}${SyncOutcomePresenter.present(getContext(this), this.syncRunSnapshot.outcome)}`
    }
    return `最近状态：${this.fullSyncOverallStatusText()}`
  }

  // 完整失败明细只在用户展开时渲染，失败条目再多也留在设置列表自己的滚动区域内。
  @Builder
  SyncReportDetails() {
    Column({ space: 4 }) {
      if (this.latestFullSyncReportReadError.length > 0) {
        Text(`最近整轮报告读取失败（${this.latestFullSyncReportReadError}）`)
          .fontSize(12)
      } else if (this.latestFullSyncReport !== null && this.latestFullSyncReport.failures.length > 0) {
        ForEach(this.latestFullSyncReport.failures, (failure: FullSyncReportFailure) => {
          Column({ space: 2 }) {
            Text(`失败组：${failure.group_id}（${failure.target_kind}）`)
              .fontSize(12)
            Text(`路径：${failure.path} · 阶段：${failure.phase}`)
              .fontSize(12)
            Text(`类型化原因：${this.fullSyncReportFailureReason(failure)}`)
              .fontSize(12)
            if (failure.detail.length > 0) {
              Text(`详情：${failure.detail}`)
                .fontSize(12)
            }
          }
          .width('100%')
          .margin({ top: 4 })
        }, (failure: FullSyncReportFailure) =>
          `${failure.group_id}:${failure.path}:${failure.phase}:${failure.occurred_at}`)
      } else {
        Text('最近整轮报告没有失败条目')
          .fontSize(12)
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }

  // ── 搜索过滤设置项 ──
  // 纯函数：只按当前生效搜索词派生可见分组，不写任何 @Local/@Trace，也不改展开状态。
  // 展开状态只由用户点击分类标题触发的 toggleSection（见 ExpandableCategory.onClick）修改。
  // 搜索命中的分类在显示层临时展开（isSectionExpandedForDisplay），不落持久化折叠状态。
  private deriveFilteredGroups(keyword: string): GroupDefinition[] {
    const normalized = keyword.trim().toLowerCase()
    if (normalized.length === 0) {
      return GROUP_DEFINITIONS
    }

    const result: GroupDefinition[] = []
    for (const groupDef of GROUP_DEFINITIONS) {
      const groupTitle = this.getGroupTitle(groupDef.id).toLowerCase()
      const groupMatched = groupTitle.includes(normalized)

      const matchedSections: SettingsSectionId[] = []
      for (const sectionId of groupDef.sections) {
        if (this.sectionMatchesKeyword(sectionId, normalized)) {
          matchedSections.push(sectionId)
        }
      }

      // 分组标题命中：保留分组下全部分类；仅部分分类命中：只保留命中的分类。
      if (groupMatched) {
        result.push(groupDef)
      } else if (matchedSections.length > 0) {
        result.push({ id: groupDef.id, sections: matchedSections })
      }
    }
    return result
  }

  // 单个分类是否命中关键词（分类标题或分类内具体设置项标题）。keyword 需为小写。
  private sectionMatchesKeyword(sectionId: SettingsSectionId, keyword: string): boolean {
    if (this.getSectionTitle(sectionId).toLowerCase().includes(keyword)) {
      return true
    }
    for (const term of this.getSectionSearchTerms(sectionId)) {
      if (term.toLowerCase().includes(keyword)) {
        return true
      }
    }
    return false
  }

  // 当前生效的搜索词：宽屏浮层用外壳传入的 overlaySearchText，窄屏路由页用本地 searchText。
  private activeSearchText(): string {
    return this.overlayLayout ? this.overlaySearchText : this.searchText
  }

  // 显示层展开判断：搜索激活且当前分类命中时临时展开，否则读用户折叠状态。
  // 这是纯读取，不在渲染期写状态，避免 re-render 期间往更新队列塞新节点。
  private isSectionExpandedForDisplay(sectionId: SettingsSectionId): boolean {
    const keyword = this.activeSearchText().trim().toLowerCase()
    if (keyword.length > 0 && this.sectionMatchesKeyword(sectionId, keyword)) {
      return true
    }
    return this.viewModel.isSectionExpanded(sectionId)
  }

  // ── 保存设置后的消息解析（问题9：在页面层做本地化，ViewModel 只管状态） ──
  private resolveSaveMessage(): string {
    const msg = this.viewModel.saveMessage
    if (msg === 'saved') {
      return getContext(this).resourceManager.getStringSync($r('app.string.settings_saved').id)
    } else if (msg.startsWith('error:')) {
      const errorCode = msg.substring(6)
      return MessageKeyMapper.resolveMessage(getContext(this), undefined, undefined, errorCode)
    }
    return ''
  }

  // ════════════════════════════════════════════
  // Issue #926：同步冲突入口与面板
  // ════════════════════════════════════════════

  // 当前同步结果指示的冲突数；-1 表示结果里没有冲突指示。
  // 只从 Controller 共享运行态读取，不在这里触发任何 Core 调用。
  private syncOutcomeConflictCount(): number {
    const outcome: SyncOutcome | null = this.syncRunSnapshot.outcome
    if (outcome === null) {
      return -1
    }
    if (outcome.errorCode === 'SYNC_SESSION_UNALIGNED' ||
      outcome.messageKey === 'sync.result.session_unaligned') {
      return -1
    }
    const total: number = outcome.totalConflicts ?? 0
    if (outcome.errorCode === 'SYNC_CONFLICT' || outcome.overallStatus === 'conflict' ||
      outcome.overallStatus === 'partial_conflict') {
      return total
    }
    return total > 0 ? total : -1
  }

  // 冲突入口可见性：
  // - 面板已从 Core 拉取过剩余数时以 Core 为准（0 处即没有冲突，不再显示过期入口）；
  // - 否则由同步结果决定（errorCode=SYNC_CONFLICT 或 totalConflicts > 0）。
  private hasConflictEntry(): boolean {
    if (this.conflictRemaining >= 0) {
      return this.conflictRemaining > 0
    }
    return this.syncOutcomeConflictCount() >= 0
  }

  // 入口计数优先用面板从 Core 拉取的真实剩余数（解决后立即刷新），
  // 面板没拉取过时退回同步结果的 totalConflicts。
  private conflictEntryCount(): number {
    if (this.conflictRemaining >= 0) {
      return this.conflictRemaining
    }
    const outcomeCount: number = this.syncOutcomeConflictCount()
    return outcomeCount > 0 ? outcomeCount : 0
  }

  private conflictEntryLabel(): string {
    const count: number = this.conflictEntryCount()
    if (count > 0) {
      return getContext(this).resourceManager
        .getStringSync($r('app.string.sync_conflicts_view_format').id)
        .replace('%s', count.toString())
    }
    return getContext(this).resourceManager.getStringSync($r('app.string.sync_conflicts_view').id)
  }

  // 设置页内发起同步：清掉面板回传的旧计数，入口完全由新的同步结果决定。
  // Issue #952：与后台自动同步重叠时合并进同一运行任务（保持去重、不新起一轮），
  // 但本次是用户主动发起——先提示“同步正在进行”，结束时反馈本次请求收到的结果。
  private async requestSync(): Promise<void> {
    this.conflictRemaining = -1
    const mergedIntoAuto: boolean =
      SyncOperationController.getInstance().currentRunningSource() === 'auto'
    if (mergedIntoAuto) {
      this.showSyncEntryToast('同步正在进行…')
    }
    const outcome: SyncOutcome = await this.viewModel.performSync()
    if (mergedIntoAuto) {
      this.showSyncEntryToast(SyncOutcomePresenter.present(getContext(this), outcome))
    }
    // Controller 的共享运行态指示冲突时，用 Core 全局冲突列表的真实剩余数刷新入口计数。
    if (this.syncOutcomeConflictCount() >= 0) {
      await this.refreshConflictProbe()
    }
    await this.refreshFullSyncState()
    await this.refreshLatestFullSyncReport()
  }

  // Issue #952：手动同步进行中的判定——只禁手动按钮防连点，后台自动同步不锁设置页。
  private isManualSyncInProgress(): boolean {
    return this.syncRunSnapshot.phase === 'in_progress' && this.syncRunSnapshot.source === 'manual'
  }

  // Issue #952：设置页同步入口 Toast（合并进后台自动任务时的进行中/结果反馈）。
  private showSyncEntryToast(message: string): void {
    try {
      promptAction.showToast({ message: message })
    } catch (_e) {
      // Toast 失败不影响同步结果本身。
    }
  }

  // 从 Core 全局冲突列表取真实剩余数；拉取失败保持旧值（-1 时退回同步结果判断）。
  private async refreshConflictProbe(): Promise<void> {
    await this.conflictProbe.loadConflicts()
    if (this.conflictProbe.listErrorCode.length === 0) {
      this.conflictRemaining = this.conflictProbe.items.length
    }
  }

  private openConflictsPanel(): void {
    if (this.conflictsPanelContent !== null) {
      return
    }
    const uiContext = this.getUIContext()
    const args: SyncConflictsPanelArgs = {
      onRequestClose: () => { this.closeConflictsPanel() },
      onRemainingChanged: (remaining: number) => { this.conflictRemaining = remaining }
    }
    const content = createSyncConflictsPanelContent(uiContext, args)
    this.conflictsPanelContent = content
    uiContext.getPromptAction().openCustomDialog(content, {
      alignment: DialogAlignment.Center,
      maskColor: '#80000000',
      autoCancel: true,
      onDidDisappear: () => { this.releaseConflictsPanel(content) }
    }).catch((e: Error) => {
      // open 失败时对话框不会出现，onDidDisappear 不会回调，这里负责回收 content。
      this.releaseConflictsPanel(content)
      hilog.error(DOMAIN, TAG, 'openConflictsPanel failed: %{public}s', String(e))
    })
  }

  private closeConflictsPanel(): void {
    if (this.conflictsPanelContent === null) {
      return
    }
    const content = this.conflictsPanelContent
    this.getUIContext().getPromptAction().closeCustomDialog(content).catch((e: Error) => {
      // 关闭失败时对话框可能仍在，交给用户下一次交互（点遮罩 / 系统返回）走 autoCancel 路径。
      hilog.error(DOMAIN, TAG, 'closeConflictsPanel failed: %{public}s', String(e))
    })
  }

  private releaseConflictsPanel(content: ComponentContent<SyncConflictsPanelArgs>): void {
    if (this.conflictsPanelContent !== content) {
      return
    }
    this.conflictsPanelContent = null
    content.dispose()
  }

  // Issue #958：打开内置模型第三方资源面板（来源声明 + 许可全文，纯本地 rawfile 文本）。
  private openThirdPartyNoticesPanel(): void {
    if (this.thirdPartyNoticesPanelContent !== null) {
      return
    }
    const uiContext = this.getUIContext()
    const args: ThirdPartyNoticesPanelArgs = {
      onRequestClose: () => { this.closeThirdPartyNoticesPanel() }
    }
    const content = createThirdPartyNoticesPanelContent(uiContext, args)
    this.thirdPartyNoticesPanelContent = content
    uiContext.getPromptAction().openCustomDialog(content, {
      alignment: DialogAlignment.Center,
      maskColor: '#80000000',
      autoCancel: true,
      onDidDisappear: () => { this.releaseThirdPartyNoticesPanel(content) }
    }).catch((e: Error) => {
      this.releaseThirdPartyNoticesPanel(content)
      hilog.error(DOMAIN, TAG, 'openThirdPartyNoticesPanel failed: %{public}s', String(e))
    })
  }

  private closeThirdPartyNoticesPanel(): void {
    if (this.thirdPartyNoticesPanelContent === null) {
      return
    }
    const content = this.thirdPartyNoticesPanelContent
    this.getUIContext().getPromptAction().closeCustomDialog(content).catch((e: Error) => {
      hilog.error(DOMAIN, TAG, 'closeThirdPartyNoticesPanel failed: %{public}s', String(e))
    })
  }

  private releaseThirdPartyNoticesPanel(content: ComponentContent<ThirdPartyNoticesPanelArgs>): void {
    if (this.thirdPartyNoticesPanelContent !== content) {
      return
    }
    this.thirdPartyNoticesPanelContent = null
    content.dispose()
  }

  // 同步结果区域的冲突入口（宽屏 / 窄屏共用）。
  @Builder
  SyncConflictEntry() {
    Column() {
      this.SettingsNeutralAction(this.conflictEntryLabel(), () => { this.openConflictsPanel() })
    }
    .width('100%')
    .margin({ bottom: 16 })
  }

  // Issue #825：设置内容的唯一根 Builder。
  // 窄屏 / 中屏由 SettingsScreen 的 NavDestination/HdsSettingsDestination 外壳调用（overlayLayout=false），
  // 宽屏由 WideSettingsOverlay 调用（overlayLayout=true，外框 / 标题 / 搜索由浮层承载）。
  // Issue #932：overlayLayout=false 不再按 sizeClass 分叉宽屏版式，一律
  // CompactSettingsHeader + CompactScreenLayout。
  // Issue #972：overlayLayout=true 只画两列 WideSettingsSection 分类卡片。
  @Builder
  NavContent() {
    Column() {
      // ── 窄屏 Header：保存状态提示 + 搜索框；宽屏浮层的标题与搜索由 WideSettingsOverlay 承担 ──
      if (!this.overlayLayout) {
        this.CompactSettingsHeader()
      }

      // Issue #952：同步摘要移入“数据与同步 → 同步”分类内，不再固定占据设置页顶部。
      if (this.viewModel.isLoading) {
        // Issue #932：加载中状态复用公共 CompactPageState，不再自绘 LoadingProgress。
        CompactPageState({
          kind: CompactPageStateKind.Loading,
          title: getContext(this).resourceManager.getStringSync($r('app.string.loading').id)
        })
      } else if (this.overlayLayout) {
        this.WideOverlaySettingsBody()
      } else {
        this.CompactScreenLayout()
      }
    }
    .width('100%')
    .height('100%')
    // Issue #932 复核：设置页根容器补页面底色，避免 HDS NavDestination 的内部 surface 透出白底。
    // Issue #972：宽屏浮层外壳 WideSettingsOverlay 与本组件共用同一底色。
    .backgroundColor($r('sys.color.background_secondary'))
  }

  // ── 窄屏 Header：保存状态提示 + 搜索框（标题由外壳标题栏承载，Issue #768/#774） ──
  @Builder
  CompactSettingsHeader() {
    Column() {
      // 状态提示行
      Row() {
        Blank()

        // 保存状态提示
        if (this.viewModel.saveMessage) {
          Text(this.resolveSaveMessage())
            .fontSize(12)
        }

        // 复制设备信息结果提示
        if (this.copyMessage) {
          Text(this.copyMessage)
            .fontSize(12)
            .margin({ left: 8 })
        }
      }
      .width('100%')
      .padding({ top: 16, bottom: 8 })

      // 搜索入口（Issue #982）：打开全局搜索，不再在本页单独过滤设置项。
      Row() {
        Image($r('app.media.ic_search'))
          .width(20)
          .height(20)
          .fillColor($r('sys.color.icon_primary'))
          .margin({ left: 12 })

        Text('搜索设置、作品、章节与星图')
          .layoutWeight(1)
          .height(36)
          .fontSize(14)
          .fontColor($r('sys.color.font_secondary'))
          .textAlign(TextAlign.Start)
          .margin({ left: 8 })

        Image($r('sys.media.ohos_ic_public_arrow_right'))
          .width(16)
          .height(16)
          .fillColor($r('sys.color.icon_secondary'))
          .margin({ right: 12 })
      }
      .width('100%')
      .height(40)
      .margin({ bottom: 8 })
      .backgroundColor($r('sys.color.comp_background_gray'))
      // Issue #932：输入框圆角归口到共享 token。
      .borderRadius(DesignTokens.inputFieldRadius)
      .onClick((): void => {
        this.onOpenGlobalSearch(SettingsSectionContext.getCurrentSection())
      })
    }
    .width('100%')
    .padding({ left: this.adaptiveState.pagePadding, right: this.adaptiveState.pagePadding })
    .constraintSize({ maxWidth: this.adaptiveState.contentMaxWidth })
  }

  // ════════════════════════════════════════════
  // 问题3：窄屏布局 — 保留折叠分类
  // ════════════════════════════════════════════
  @Builder
  CompactScreenLayout() {
    Scroll(this.narrowScroller) {
      Column() {
        // 按分组渲染：外观→写作→数据与同步→高级→关于
        this.SettingsGroupList()
      }
      .width('100%')
      .constraintSize({ maxWidth: this.adaptiveState.contentMaxWidth })
      .padding({ left: this.adaptiveState.pagePadding, right: this.adaptiveState.pagePadding, bottom: 24 })
    }
    .id(`settings_${this.instanceToken}_narrow_scroll`)
    .layoutWeight(1)
    .scrollBar(BarState.Off)
    .align(Alignment.Top)
  }

  // ── 分组列表（窄屏折叠布局，支持搜索过滤） ──
  @Builder
  SettingsGroupList() {
    ForEach(this.deriveFilteredGroups(this.searchText), (groupDef: GroupDefinition) => {
      Column() {
        // 分组标题（单分类且组名等于分类名时不重复画）
        if (this.shouldShowGroupHeader(groupDef)) {
          this.SettingsGroupHeader(groupDef.id)
        }

        // 分组内各分类（可独立展开折叠项）
        ForEach(groupDef.sections, (sectionId: SettingsSectionId) => {
          this.ExpandableCategory(sectionId)
        }, (sectionId: SettingsSectionId) => sectionId)
      }
      .width('100%')
      .margin({ bottom: 16 })
    }, (groupDef: GroupDefinition) => groupDef.id)
  }

  // ── 分组标题 ──
  @Builder
  SettingsGroupHeader(groupId: SettingsGroupId) {
    Text(this.getGroupTitle(groupId))
      .fontSize(14)
      .fontWeight(FontWeight.Medium)
      .margin({ bottom: 8, top: 8 })
      .width('100%')
  }

  // ── 中性操作按钮（使用系统中性表面色） ──
  @Builder
  SettingsNeutralAction(label: ResourceStr, onClick: () => void, enabled: boolean = true) {
    Button(label, { type: ButtonType.Normal })
      .width('100%')
      .backgroundColor($r('sys.color.ohos_id_color_button_normal'))
      .fontColor($r('sys.color.font_primary'))
      .borderRadius(DesignTokens.radiusSm)
      .enabled(enabled)
      .onClick(onClick)
  }

  // ── 横排内联操作按钮（用于 Row 中并排按钮，layoutWeight 均分宽度） ──
  @Builder
  SettingsInlineAction(label: ResourceStr, onClick: () => void, enabled: boolean = true) {
    Button(label, { type: ButtonType.Normal })
      .layoutWeight(1)
      .backgroundColor($r('sys.color.ohos_id_color_button_normal'))
      .fontColor($r('sys.color.font_primary'))
      .borderRadius(DesignTokens.radiusSm)
      .enabled(enabled)
      .onClick(onClick)
  }

  // ── 可展开折叠分类 ──
  @Builder
  ExpandableCategory(sectionId: SettingsSectionId) {
    Column() {
      // 分类标题行（点击展开/收起）
      Row() {
        Text(this.getSectionTitle(sectionId))
          .fontSize(16)
          .fontWeight(FontWeight.Medium)
          .layoutWeight(1)

        // 当前值摘要
        Text(this.getSectionValueSummary(sectionId))
          .fontSize(14)
          .margin({ right: 8 })

        // 展开/收起箭头
        Image($r('sys.media.ohos_ic_public_arrow_down'))
          .width(20)
          .height(20)
          .rotate({ angle: this.isSectionExpandedForDisplay(sectionId) ? 180 : 0 })
          .animation({ duration: 150, curve: Curve.EaseInOut })
      }
      .width('100%')
      .height(48)
      .padding({ left: 16, right: 16 })
      .onClick(() => {
        SettingsSectionContext.setCurrentSection(sectionId)
        this.recordContinuationSection(sectionId, '')
        this.viewModel.toggleSection(sectionId)
      })

      // 分类内容（展开时显示）
      if (this.isSectionExpandedForDisplay(sectionId)) {
        Column() {
          this.SectionContent(sectionId)
        }
        .width('100%')
        .padding({ left: 16, right: 16, bottom: 12 })
        .animation({ duration: 150, curve: Curve.EaseInOut })
      }
    }
    .width('100%')
    .borderRadius(DesignTokens.radiusMd)
    .margin({ bottom: 8 })
  }

  // ── 分类内容路由 ──
  @Builder
  SectionContent(sectionId: SettingsSectionId) {
    if (sectionId === 'appearance') {
      this.AppearanceContent()
    } else if (sectionId === 'editor') {
      this.EditorContent()
    } else if (sectionId === 'save') {
      this.SaveContent()
    } else if (sectionId === 'sync') {
      this.SyncContent()
    } else if (sectionId === 'recovery') {
      this.RecoveryContent()
    } else if (sectionId === 'ai') {
      this.AiContent()
    } else if (sectionId === 'diagnostics') {
      this.DiagnosticsContent()
    } else if (sectionId === 'laboratory') {
      this.LaboratoryContent()
    } else if (sectionId === 'about') {
      this.AboutContent()
    }
  }

  // ════════════════════════════════════════════
  // 窄屏折叠分类内容 — 外观设置
  // ════════════════════════════════════════════
  @Builder
  AppearanceContent() {
    Column() {
      // 主题模式 — 顺序必须是 跟随系统→浅色→深色
      Row() {
        Text($r('app.string.theme_mode'))
          .fontSize(16)
          .layoutWeight(1)

        Select(this.getAppearanceOptions())
          .selected(this.getThemeIndex())
          .value(this.getThemeName())
          .onSelect((index: number) => {
            const themes: ThemeMode[] = ['system', 'light', 'dark']
            const selectedTheme: ThemeMode = themes[index]
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.appearanceMode = selectedTheme
              s.themeMode = selectedTheme
              return s
            })
            this.themeAdapter.setMode(selectedTheme)
            this.viewModel.saveSettings()
          })
      }
      .id(this.settingItemId('appearanceMode'))
      .width('100%')
      .padding({ left: 4, right: 4, top: 2, bottom: 2 })
      .borderRadius(DesignTokens.radiusSm)
      .backgroundColor(this.settingHighlightColor('appearanceMode'))
      .margin({ bottom: 16 })

      // 内置主题选择（实验性）。HarmonyOS 只使用 ArkUI 系统色与内置主题，
      // 不读取 Android 设备配色（Issue #1005）。
      if (this.viewModel.builtinThemes.length > 0) {
        Row() {
          Text('内置主题（实验性）')
            .fontSize(16)
            .layoutWeight(1)

          Select(this.getBuiltinThemeOptions())
            .selected(this.getSelectedBuiltinThemeIndex())
            .value(this.getSelectedBuiltinThemeName())
            .onSelect((index: number) => {
              if (index < this.viewModel.builtinThemes.length) {
                const themeId = this.viewModel.builtinThemes[index].theme_id
                this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
                  s.selectedBuiltinThemeId = themeId
                  return s
                })
                this.themeAdapter.setSelectedBuiltinThemeId(themeId)
                this.viewModel.saveSettings()
              }
            })
        }
        .id(this.settingItemId('selectedBuiltinThemeId'))
        .width('100%')
        .padding({ left: 4, right: 4, top: 2, bottom: 2 })
        .borderRadius(DesignTokens.radiusSm)
        .backgroundColor(this.settingHighlightColor('selectedBuiltinThemeId'))
        .margin({ bottom: 16 })
      }

      // 界面语言
      Row() {
        Text('界面语言')
          .fontSize(16)
          .layoutWeight(1)
        Select(this.getLocaleOptions())
          .selected(this.getLocaleIndex())
          .value(this.getLocaleName())
          .onSelect((index: number) => {
            const locales: string[] = ['', 'zh-CN', 'en-US']
            const selected: string = index >= 0 && index < locales.length ? locales[index] : ''
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.locale = selected.length > 0 ? selected : null
              return s
            })
            this.viewModel.saveSettings()
          })
      }
      .id(this.settingItemId('locale'))
      .width('100%')
      .padding({ left: 4, right: 4, top: 2, bottom: 2 })
      .borderRadius(DesignTokens.radiusSm)
      .backgroundColor(this.settingHighlightColor('locale'))
      .margin({ bottom: 16 })

      // 字体大小（旧同步键 fontSize 也定位到这里）
      Row() {
        Text($r('app.string.font_size'))
          .fontSize(16)
          .layoutWeight(1)
        Text(`${this.viewModel.settings.editorFontSize}px`)
          .fontSize(14)
          .margin({ right: 16 })
        Slider({ value: this.viewModel.settings.editorFontSize, min: 12, max: 32, step: 1 })
          .width(120)
          .onChange((value: number) => {
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.editorFontSize = value
              return s
            })
            this.viewModel.saveSettings()
          })
      }
      .id(this.settingItemId('editorFontSize'))
      .width('100%')
      .padding({ left: 4, right: 4, top: 2, bottom: 2 })
      .borderRadius(DesignTokens.radiusSm)
      .backgroundColor(this.settingHighlightColor('editorFontSize'))
      .margin({ bottom: 16 })

      // 行距倍数
      Row() {
        Text($r('app.string.line_spacing_multiplier'))
          .fontSize(16)
          .layoutWeight(1)
        Text(`${this.viewModel.settings.editorLineSpacingMultiplier.toFixed(1)}`)
          .fontSize(14)
          .margin({ right: 16 })
        Slider({ value: this.viewModel.settings.editorLineSpacingMultiplier, min: 1.0, max: 3.0, step: 0.1 })
          .width(120)
          .onChange((value: number) => {
            const rounded = Math.round(value * 10) / 10
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.editorLineSpacingMultiplier = rounded
              return s
            })
            this.viewModel.saveSettings()
          })
      }
      .id(this.settingItemId('editorLineSpacingMultiplier'))
      .width('100%')
      .padding({ left: 4, right: 4, top: 2, bottom: 2 })
      .borderRadius(DesignTokens.radiusSm)
      .backgroundColor(this.settingHighlightColor('editorLineSpacingMultiplier'))
    }
  }

  // ════════════════════════════════════════════
  // 窄屏折叠分类内容 — 编辑器设置
  // ════════════════════════════════════════════
  @Builder
  EditorContent() {
    Column() {
      // 自动缩进
      Row() {
        GripAdaptiveToggleRow({
          side: this.gripSide,
          isOn: this.viewModel.settings.autoIndentEnabled,
          onChange: (isOn: boolean): void => {
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.autoIndentEnabled = isOn
              return s
            })
            this.viewModel.saveSettings()
          }
        }) {
          Text($r('app.string.auto_indent')).fontSize(16)
        }
      }
      .id(this.settingItemId('autoIndentEnabled'))
      .width('100%')
      .padding({ left: 4, right: 4, top: 2, bottom: 2 })
      .borderRadius(DesignTokens.radiusSm)
      .backgroundColor(this.settingHighlightColor('autoIndentEnabled'))
      .margin({ bottom: 16 })

      // 缩进宽度
      Row() {
        Text($r('app.string.indent_width'))
          .fontSize(16)
          .layoutWeight(1)
        Text(`${this.viewModel.settings.autoIndentWidth}`)
          .fontSize(14)
          .margin({ right: 16 })
        Slider({ value: this.viewModel.settings.autoIndentWidth, min: 0, max: 8, step: 1 })
          .width(120)
          .onChange((value: number) => {
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.autoIndentWidth = value
              return s
            })
            this.viewModel.saveSettings()
          })
      }
      .id(this.settingItemId('autoIndentWidth'))
      .width('100%')
      .padding({ left: 4, right: 4, top: 2, bottom: 2 })
      .borderRadius(DesignTokens.radiusSm)
      .backgroundColor(this.settingHighlightColor('autoIndentWidth'))
      .margin({ bottom: 16 })

      // 打字动画
      Row() {
        GripAdaptiveToggleRow({
          side: this.gripSide,
          isOn: this.viewModel.settings.editorTypingAnimationEnabled,
          onChange: (isOn: boolean): void => {
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.editorTypingAnimationEnabled = isOn
              return s
            })
            this.viewModel.saveSettings()
          }
        }) {
          Text('打字动画').fontSize(16)
        }
      }
      .id(this.settingItemId('editorTypingAnimationEnabled'))
      .width('100%')
      .padding({ left: 4, right: 4, top: 2, bottom: 2 })
      .borderRadius(DesignTokens.radiusSm)
      .backgroundColor(this.settingHighlightColor('editorTypingAnimationEnabled'))
      .margin({ bottom: 16 })

      // 打字动画时长
      Row() {
        Text('打字动画时长')
          .fontSize(16)
          .layoutWeight(1)
        Text(`${this.viewModel.settings.editorTypingAnimationDurationMs}ms`)
          .fontSize(14)
          .margin({ right: 16 })
        Slider({ value: this.viewModel.settings.editorTypingAnimationDurationMs, min: 20, max: 500, step: 10 })
          .width(120)
          .enabled(this.viewModel.settings.editorTypingAnimationEnabled)
          .onChange((value: number) => {
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.editorTypingAnimationDurationMs = value
              return s
            })
            this.viewModel.saveSettings()
          })
      }
      .id(this.settingItemId('editorTypingAnimationDurationMs'))
      .width('100%')
      .padding({ left: 4, right: 4, top: 2, bottom: 2 })
      .borderRadius(DesignTokens.radiusSm)
      .backgroundColor(this.settingHighlightColor('editorTypingAnimationDurationMs'))
      .margin({ bottom: 16 })

      // 平滑光标
      Row() {
        GripAdaptiveToggleRow({
          side: this.gripSide,
          isOn: this.viewModel.settings.editorSmoothCursorEnabled,
          onChange: (isOn: boolean): void => {
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.editorSmoothCursorEnabled = isOn
              return s
            })
            this.viewModel.saveSettings()
          }
        }) {
          Text('平滑光标').fontSize(16)
        }
      }
      .id(this.settingItemId('editorSmoothCursorEnabled'))
      .width('100%')
      .padding({ left: 4, right: 4, top: 2, bottom: 2 })
      .borderRadius(DesignTokens.radiusSm)
      .backgroundColor(this.settingHighlightColor('editorSmoothCursorEnabled'))
      .margin({ bottom: 16 })

      // 平滑光标时长
      Row() {
        Text('平滑光标时长')
          .fontSize(16)
          .layoutWeight(1)
        Text(`${this.viewModel.settings.editorSmoothCursorDurationMs}ms`)
          .fontSize(14)
          .margin({ right: 16 })
        Slider({ value: this.viewModel.settings.editorSmoothCursorDurationMs, min: 20, max: 500, step: 10 })
          .width(120)
          .enabled(this.viewModel.settings.editorSmoothCursorEnabled)
          .onChange((value: number) => {
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.editorSmoothCursorDurationMs = value
              return s
            })
            this.viewModel.saveSettings()
          })
      }
      .id(this.settingItemId('editorSmoothCursorDurationMs'))
      .width('100%')
      .padding({ left: 4, right: 4, top: 2, bottom: 2 })
      .borderRadius(DesignTokens.radiusSm)
      .backgroundColor(this.settingHighlightColor('editorSmoothCursorDurationMs'))
      .margin({ bottom: 16 })

      // 协同文本动画
      Row() {
        GripAdaptiveToggleRow({
          side: this.gripSide,
          isOn: this.viewModel.settings.editorCoordinatedTextCursorAnimationEnabled,
          onChange: (isOn: boolean): void => {
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.editorCoordinatedTextCursorAnimationEnabled = isOn
              return s
            })
            this.viewModel.saveSettings()
          }
        }) {
          Text('协同动画').fontSize(16)
        }
      }
      .id(this.settingItemId('editorCoordinatedTextCursorAnimationEnabled'))
      .width('100%')
      .padding({ left: 4, right: 4, top: 2, bottom: 2 })
      .borderRadius(DesignTokens.radiusSm)
      .backgroundColor(this.settingHighlightColor('editorCoordinatedTextCursorAnimationEnabled'))
    }
  }

  // ════════════════════════════════════════════
  // 窄屏折叠分类内容 — 保存设置
  // ════════════════════════════════════════════
  @Builder
  SaveContent() {
    Column() {
      // 自动保存
      Row() {
        GripAdaptiveToggleRow({
          side: this.gripSide,
          isOn: this.viewModel.settings.autoSaveEnabled,
          onChange: (isOn: boolean): void => {
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.autoSaveEnabled = isOn
              return s
            })
            this.viewModel.saveSettings()
          }
        }) {
          Text($r('app.string.auto_save')).fontSize(16)
        }
      }
      .id(this.settingItemId('autoSaveEnabled'))
      .width('100%')
      .padding({ left: 4, right: 4, top: 2, bottom: 2 })
      .borderRadius(DesignTokens.radiusSm)
      .backgroundColor(this.settingHighlightColor('autoSaveEnabled'))
      .margin({ bottom: 16 })

      // 自动保存延迟：始终渲染，关闭自动保存时禁用，保证搜索能定位到实际设置项。
      Row() {
        Text($r('app.string.auto_save_delay'))
          .fontSize(16)
          .layoutWeight(1)
        Text(getContext(this).resourceManager.getStringSync($r('app.string.seconds_format').id)
          .replace('%s', `${Math.round(this.viewModel.settings.autoSaveDelayMs / 1000)}`))
          .fontSize(14)
          .margin({ right: 16 })
        Slider({ value: this.viewModel.settings.autoSaveDelayMs / 1000, min: 1, max: 10, step: 1 })
          .width(120)
          .enabled(this.viewModel.settings.autoSaveEnabled)
          .onChange((value: number) => {
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.autoSaveDelayMs = value * 1000
              return s
            })
            this.viewModel.saveSettings()
          })
      }
      .id(this.settingItemId('autoSaveDelayMs'))
      .width('100%')
      .padding({ left: 4, right: 4, top: 2, bottom: 2 })
      .borderRadius(DesignTokens.radiusSm)
      .backgroundColor(this.settingHighlightColor('autoSaveDelayMs'))
    }
  }

  // ════════════════════════════════════════════
  // 窄屏折叠分类内容 — 同步设置
  // ════════════════════════════════════════════
  // Issue #988：附近设备同步。这里明确真实条件，不把“已登录华为账号”
  // 当成“同账号”或“可同步”的证明。
  @Builder
  NearbyDeviceSyncRows() {
    Column({ space: 8 }) {
      Text('附近设备同步')
        .fontSize(16)
        .fontWeight(FontWeight.Medium)
        .width('100%')
      Text('通过系统可信设备直接同步作品、章节、星图和统计；双方都无需 GitHub 仓库，也不会传递 GitHub Token。')
        .fontSize(12)
        .opacity(0.72)
        .width('100%')
        .margin({ bottom: 2 })

      if (this.deviceSync.devices.length > 0) {
        ForEach(this.deviceSync.devices, (device: TrustedDeviceInfo) => {
          Row({ space: 8 }) {
            Column({ space: 2 }) {
              Text(device.deviceName)
                .fontSize(14)
                .width('100%')
              Text(device.online ? '在线' : '离线')
                .fontSize(11)
                .opacity(0.62)
                .width('100%')
            }
            .layoutWeight(1)
            Button(this.deviceSync.selectedNetworkId === device.networkId ? '已选择' : '选择')
              .fontSize(12)
              .height(30)
              .onClick(() => this.deviceSync.selectDevice(device))
          }
          .width('100%')
          .padding({ top: 4, bottom: 4 })
        }, (device: TrustedDeviceInfo): string => device.networkId)
      }

      // A single state line covers loading, empty, selection, connection and errors.
      Text(this.deviceSync.statusMessage)
        .fontSize(12)
        .fontColor($r('sys.color.font_secondary'))
        .width('100%')

      if (this.deviceSync.displayState === 'empty') {
        Button('刷新设备')
          .fontSize(12)
          .height(32)
          .onClick(() => this.deviceSync.refresh())
      }

      if (this.deviceSync.displayState === 'selected' ||
        this.deviceSync.displayState === 'connected') {
        Button(this.deviceSync.displayState === 'connected' ? '断开' : '连接')
          .fontSize(12)
          .height(32)
          .onClick(() => {
            if (this.deviceSync.displayState === 'connected') {
              this.deviceSync.disconnect()
            } else {
              this.deviceSync.connectSelected()
            }
          })
      }

      if (this.deviceSync.displayState === 'connected') {
        Text(this.deviceSync.remoteCloudLabel())
          .fontSize(12)
          .fontColor($r('sys.color.font_tertiary'))
          .width('100%')
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
    .padding({ top: 8, bottom: 8 })
  }

  @Builder
  SyncContent() {
    Column() {
      Column({ space: 8 }) {
        Text('云端同步')
          .fontSize(16)
          .fontWeight(FontWeight.Medium)
          .width('100%')
        // Issue #952：同步摘要跟随“同步”分类展开才显示；完整字段由用户按需展开。
        this.SyncBriefSummary()

        // 启用同步
        Row() {
          GripAdaptiveToggleRow({
            side: this.gripSide,
            isOn: this.viewModel.syncConfig?.enabled || false,
            onChange: (isOn: boolean): void => {
              if (this.viewModel.syncConfig) {
                const newConfig: SyncConfig = {
                  enabled: isOn,
                  activeProvider: this.viewModel.syncConfig.activeProvider,
                  providerConfig: this.viewModel.syncConfig.providerConfig,
                  autoSync: this.viewModel.syncConfig.autoSync,
                  syncIntervalSeconds: this.viewModel.syncConfig.syncIntervalSeconds,
                  hasNetworkPermission: this.viewModel.syncConfig.hasNetworkPermission,
                  hasNetworkStatePermission: this.viewModel.syncConfig.hasNetworkStatePermission
                }
                this.viewModel.updateSyncConfig(newConfig)
              }
            }
          }) {
            Text($r('app.string.enable_sync')).fontSize(16)
          }
        }
        .width('100%')
        .margin({ bottom: 16 })

      // Sync 详细配置（仅在同步开启时显示）
      if (this.viewModel.syncConfig?.enabled) {
        // 自动同步
        Row() {
          GripAdaptiveToggleRow({
            side: this.gripSide,
            isOn: this.viewModel.syncConfig?.autoSync || false,
            onChange: (isOn: boolean): void => {
              if (this.viewModel.syncConfig) {
                const newConfig: SyncConfig = {
                  enabled: this.viewModel.syncConfig.enabled,
                  activeProvider: this.viewModel.syncConfig.activeProvider,
                  providerConfig: this.viewModel.syncConfig.providerConfig,
                  autoSync: isOn,
                  syncIntervalSeconds: this.viewModel.syncConfig.syncIntervalSeconds,
                  hasNetworkPermission: this.viewModel.syncConfig.hasNetworkPermission,
                  hasNetworkStatePermission: this.viewModel.syncConfig.hasNetworkStatePermission
                }
                this.viewModel.updateSyncConfig(newConfig)
              }
            }
          }) {
            Text($r('app.string.auto_sync')).fontSize(16)
          }
        }
        .width('100%')
        .margin({ bottom: 16 })

        // 通用 Git 仓库 HTTPS 地址（GitHub 运行时自动选路，不要求用户选择传输方式）
        Column() {
          Text($r('app.string.repository_https_url'))
            .fontSize(16)
            .margin({ bottom: 8 })

          TextInput({ text: this.viewModel.syncRemoteUrl })
            .width('100%')
            .height(40)
            .fontSize(14)
            // Issue #932：输入框圆角归口到共享 token。
            .borderRadius(DesignTokens.inputFieldRadius)
            .onChange((value: string) => {
              this.viewModel.updateSyncRemoteUrl(value)
            })
        }
        .width('100%')
        .alignItems(HorizontalAlign.Start)
        .margin({ bottom: 16 })

        // 访问令牌或密码（公开仓库可匿名读取，可留空）
        Column() {
          Text($r('app.string.access_token_optional'))
            .fontSize(16)
            .margin({ bottom: 8 })

          TextInput({ text: this.viewModel.syncToken })
            .width('100%')
            .height(40)
            .fontSize(14)
            // Issue #932：输入框圆角归口到共享 token。
            .borderRadius(DesignTokens.inputFieldRadius)
            .type(InputType.Password)
            .showPasswordIcon(true)
            .onChange((value: string) => {
              this.viewModel.updateSyncToken(value)
            })
            .onBlur(() => {
              // Issue #924：输入完成立即有序落盘，不等防抖兜底。
              void this.viewModel.commitSyncToken()
            })
            .onSubmit(() => {
              void this.viewModel.commitSyncToken()
            })
        }
        .width('100%')
        .alignItems(HorizontalAlign.Start)
        .margin({ bottom: 16 })

        // 高级设置：分支与用户名默认收起，普通用户只需仓库地址 + 可选令牌。
        Row() {
          Text($r('app.string.advanced_settings'))
            .fontSize(16)
            .layoutWeight(1)
          Text(this.viewModel.syncAdvancedExpanded ? '−' : '+')
            .fontSize(18)
        }
        .width('100%')
        .onClick(() => {
          this.viewModel.toggleSyncAdvanced()
        })
        .margin({ bottom: 8 })

        if (this.viewModel.syncAdvancedExpanded) {
          Column() {
            Text($r('app.string.branch'))
              .fontSize(14)
              .margin({ bottom: 6 })
            TextInput({ text: this.viewModel.syncBranch })
              .width('100%')
              .height(40)
              .fontSize(14)
              .borderRadius(DesignTokens.inputFieldRadius)
              .onChange((value: string) => {
                this.viewModel.updateSyncBranch(value)
              })
          }
          .width('100%')
          .alignItems(HorizontalAlign.Start)
          .margin({ bottom: 12 })

          Column() {
            Text($r('app.string.username'))
              .fontSize(14)
              .margin({ bottom: 6 })
            TextInput({ text: this.viewModel.syncUsername })
              .width('100%')
              .height(40)
              .fontSize(14)
              .borderRadius(DesignTokens.inputFieldRadius)
              .onChange((value: string) => {
                this.viewModel.updateSyncUsername(value)
              })
          }
          .width('100%')
          .alignItems(HorizontalAlign.Start)
          .margin({ bottom: 16 })
        }

        // 同步间隔
        Row() {
          Text($r('app.string.sync_interval'))
            .fontSize(16)
            .layoutWeight(1)

          Text(getContext(this).resourceManager.getStringSync($r('app.string.seconds_format').id)
            .replace('%s', `${this.viewModel.syncConfig?.syncIntervalSeconds || 300}`))
            .fontSize(14)
            .margin({ right: 16 })

          Slider({
            value: this.viewModel.syncConfig?.syncIntervalSeconds || 300,
            min: 60,
            max: 3600,
            step: 60
          })
            .width(120)
            .onChange((value: number) => {
              if (this.viewModel.syncConfig) {
                const newConfig: SyncConfig = {
                  enabled: this.viewModel.syncConfig.enabled,
                  activeProvider: this.viewModel.syncConfig.activeProvider,
                  providerConfig: this.viewModel.syncConfig.providerConfig,
                  autoSync: this.viewModel.syncConfig.autoSync,
                  syncIntervalSeconds: value,
                  hasNetworkPermission: this.viewModel.syncConfig.hasNetworkPermission,
                  hasNetworkStatePermission: this.viewModel.syncConfig.hasNetworkStatePermission
                }
                this.viewModel.updateSyncConfig(newConfig)
              }
            })
        }
        .width('100%')
        .margin({ bottom: 16 })

      }

      // Sync actions
      Row({ space: 12 }) {
        // Issue #952：只禁手动同步进行中的按钮防连点；后台自动同步不得锁住设置页操作。
        this.SettingsInlineAction($r('app.string.sync_section'),
          () => this.requestSync(),
          !this.isManualSyncInProgress())

        this.SettingsInlineAction($r('app.string.diagnose'),
          () => this.viewModel.runSyncDiagnostics(),
          !this.viewModel.isDiagnosing)
      }
      .width('100%')
      .margin({ bottom: 16 })

      // Issue #926：同步结果带冲突时给出查看/解决入口。
      if (this.hasConflictEntry()) {
        this.SyncConflictEntry()
      }

      // Diagnostics result card
      if (this.viewModel.syncDiagnostics) {
        this.DiagnosticsResultCard()
      }
      }
      .width('100%')
      .alignItems(HorizontalAlign.Start)
      .margin({ bottom: 16 })

      // Nearby-device sync stays independent from repository configuration and follows cloud sync.
      this.NearbyDeviceSyncRows()
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }

  // ── 诊断结果卡片 ──
  @Builder
  DiagnosticsResultCard() {
    Column() {
      Text($r('app.string.diagnostics_result'))
        .fontSize(16)
        .fontWeight(FontWeight.Medium)
        .margin({ bottom: 12 })

      // Four check items
      Row() {
        this.DiagnosticItem(getContext(this).resourceManager.getStringSync($r('app.string.network').id),
          this.viewModel.syncDiagnostics!.networkOk, this.viewModel.syncDiagnostics!.networkStatus)
        this.DiagnosticItem(getContext(this).resourceManager.getStringSync($r('app.string.auth').id),
          this.viewModel.syncDiagnostics!.authOk, this.viewModel.syncDiagnostics!.authStatus)
        // SyncDiagnosticsResult 没有 remoteStatus 字段；仓库状态只能由 remoteOk 单独给出，
        // 不能复用 networkStatus，否则网络正常但仓库失败时会显示自相矛盾的文案。
        this.DiagnosticItem(getContext(this).resourceManager.getStringSync($r('app.string.repository').id),
          this.viewModel.syncDiagnostics!.remoteOk,
          this.viewModel.syncDiagnostics!.remoteOk ? '可访问' : '不可访问')
      }
      .width('100%')
      .justifyContent(FlexAlign.SpaceAround)
      .margin({ bottom: 12 })

      // User message
      Text(this.viewModel.syncDiagnostics!.errorCategory && this.viewModel.syncDiagnostics!.errorCategory.length > 0
        ? MessageKeyMapper.resolveMessage(getContext(this), `error.${this.viewModel.syncDiagnostics!.errorCategory}`, undefined, undefined)
        : (this.viewModel.syncDiagnostics!.rawError ?? this.viewModel.syncDiagnostics!.providerDetails ?? ''))
        .fontSize(14)
        .margin({ bottom: 8 })

      // Error category
      if (this.viewModel.syncDiagnostics!.errorCategory && this.viewModel.syncDiagnostics!.errorCategory.length > 0) {
        Row() {
          Text(getContext(this).resourceManager.getStringSync($r('app.string.error_category').id) + ': ')
            .fontSize(12)
          Text(this.viewModel.syncDiagnostics!.errorCategory)
            .fontSize(12)
        }
        .width('100%')
        .margin({ bottom: 4 })
      }
    }
    .width('100%')
    .padding(12)
    .borderRadius(DesignTokens.radiusSm)
    .margin({ bottom: 16 })
  }

  @Builder
  DiagnosticItem(label: string, ok: boolean, status: string) {
    Column() {
      Text(ok ? 'OK' : 'FAIL')
        .fontSize(14)
        .fontWeight(FontWeight.Bold)
      Text(label)
        .fontSize(12)
        .margin({ top: 4 })
      if (status) {
        Text(status)
          .fontSize(10)
          .margin({ top: 2 })
      }
    }
    .alignItems(HorizontalAlign.Center)
  }

  // ════════════════════════════════════════════
  // 窄屏折叠分类内容 — 恢复（占位）
  // ════════════════════════════════════════════
  @Builder
  RecoveryContent() {
    Column() {
      Text('恢复功能即将上线')
        .fontSize(14)
    }
    .width('100%')
  }

  // ════════════════════════════════════════════
  // 窄屏折叠分类内容 — AI 设置
  // ════════════════════════════════════════════
  @Builder
  AiContent() {
    Column() {
      Row() {
        GripAdaptiveToggleRow({
          side: this.gripSide,
          isOn: this.viewModel.settings.aiEnabled,
          onChange: (isOn: boolean): void => {
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.aiEnabled = isOn
              return s
            })
            this.viewModel.saveSettings()
          }
        }) {
          Text($r('app.string.enable_ai')).fontSize(16)
        }
      }
      .id(this.settingItemId('aiEnabled'))
      .width('100%')
      .padding({ left: 4, right: 4, top: 2, bottom: 2 })
      .borderRadius(DesignTokens.radiusSm)
      .backgroundColor(this.settingHighlightColor('aiEnabled'))
    }
  }

  // ════════════════════════════════════════════
  // 窄屏折叠分类内容 — 诊断与日志
  // ════════════════════════════════════════════
  @Builder
  DiagnosticsContent() {
    Column() {
      // 开启诊断日志
      Row() {
        GripAdaptiveToggleRow({
          side: this.gripSide,
          isOn: this.viewModel.settings.diagnosticsEnabled,
          onChange: (isOn: boolean): void => {
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.diagnosticsEnabled = isOn
              if (!isOn) {
                s.diagnosticsVerbose = false
              }
              return s
            })
            this.viewModel.saveSettings()
          }
        }) {
          Text('开启诊断日志').fontSize(16)
        }
      }
      .id(this.settingItemId('diagnosticsEnabled'))
      .width('100%')
      .padding({ left: 4, right: 4, top: 2, bottom: 2 })
      .borderRadius(DesignTokens.radiusSm)
      .backgroundColor(this.settingHighlightColor('diagnosticsEnabled'))
      .margin({ bottom: 16 })

      // 详细日志
      Row() {
        GripAdaptiveToggleRow({
          side: this.gripSide,
          isOn: this.viewModel.settings.diagnosticsVerbose,
          toggleEnabled: this.viewModel.settings.diagnosticsEnabled,
          onChange: (isOn: boolean): void => {
            this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
              s.diagnosticsVerbose = isOn
              return s
            })
            this.viewModel.saveSettings()
          }
        }) {
          Text('详细日志').fontSize(16)
        }
      }
      .id(this.settingItemId('diagnosticsVerbose'))
      .width('100%')
      .padding({ left: 4, right: 4, top: 2, bottom: 2 })
      .borderRadius(DesignTokens.radiusSm)
      .backgroundColor(this.settingHighlightColor('diagnosticsVerbose'))
      .margin({ bottom: 16 })

      // 统计设备标识（只读：由 Core/设备信息决定，不在设置页改写）
      Row() {
        Text('统计设备标识')
          .fontSize(16)
          .layoutWeight(1)
        Text(this.viewModel.settings.statsDeviceId ?? '未设置')
          .fontSize(13)
          .fontColor($r('sys.color.font_secondary'))
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .constraintSize({ maxWidth: '55%' })
      }
      .id(this.settingItemId('statsDeviceId'))
      .width('100%')
      .padding({ left: 4, right: 4, top: 2, bottom: 2 })
      .borderRadius(DesignTokens.radiusSm)
      .backgroundColor(this.settingHighlightColor('statsDeviceId'))
      .margin({ bottom: 16 })

      // 导出诊断包
      Column({ space: 12 }) {
        this.SettingsNeutralAction('导出诊断包', async () => {
          const result = await this.viewModel.exportDiagnostics()
          if (result.status === 'success') {
            promptAction.showToast({ message: '诊断包已导出' })
          } else if (result.status === 'failed') {
            promptAction.showToast({ message: '诊断包导出失败' })
          }
        })

        // 清空日志（问题6：ViewModel 内部同时清 Rust diagnostics 和 HiLog buffer）
        this.SettingsNeutralAction('清空日志', () => {
          this.viewModel.clearLogs()
        })

        // 复制设备信息（问题5：调用 copyDeviceInfoToClipboard 真正写入剪贴板）
        this.SettingsNeutralAction('复制设备信息', async () => {
          const ok = await this.viewModel.copyDeviceInfoToClipboard()
          if (ok) {
            this.copyMessage = '已复制'
            setTimeout(() => { this.copyMessage = '' }, 2000)
          } else {
            this.copyMessage = '复制失败'
            setTimeout(() => { this.copyMessage = '' }, 2000)
          }
        })
      }
      .width('100%')
    }
    .width('100%')
  }

  // ════════════════════════════════════════════
  // 窄屏折叠分类内容 — 实验室（占位）
  // ════════════════════════════════════════════
  @Builder
  LaboratoryContent() {
    Column() {
      Text('实验室功能即将上线')
        .fontSize(14)
    }
    .width('100%')
  }

  // ════════════════════════════════════════════
  // 窄屏折叠分类内容 — 关于
  // ════════════════════════════════════════════
  @Builder
  AboutContent() {
    Column() {
      // 应用名
      Row() {
        Text($r('app.string.about_app_name_label'))
          .fontSize(16)
          .layoutWeight(1)
        Text($r('app.string.about_app_name'))
          .fontSize(14)
      }
      .width('100%')
      .margin({ bottom: 8 })

      // 作者
      Row() {
        Text($r('app.string.about_author_label'))
          .fontSize(16)
          .layoutWeight(1)
        Text($r('app.string.about_author'))
          .fontSize(14)
      }
      .width('100%')
      .margin({ bottom: 8 })

      // 项目地址
      Column() {
        Text($r('app.string.about_project_label'))
          .fontSize(16)
          .margin({ bottom: 4 })
        Text('GitHub: github.com/Xiwei753/xiezuoruanjian')
          .fontSize(14)
        Text($r('app.string.about_gitee'))
          .fontSize(14)
          .margin({ top: 4 })
      }
      .width('100%')
      .alignItems(HorizontalAlign.Start)
      .margin({ bottom: 8 })

      // 开源协议
      Row() {
        Text($r('app.string.about_license_label'))
          .fontSize(16)
          .layoutWeight(1)
        Text($r('app.string.about_license'))
          .fontSize(14)
      }
      .width('100%')
      .margin({ bottom: 8 })

      // Issue #958：内置模型的第三方来源声明与完整许可（随包分发的文本）。
      Column() {
        this.SettingsNeutralAction($r('app.string.about_third_party_resources'), () => {
          this.openThirdPartyNoticesPanel()
        })
      }
      .width('100%')
      .margin({ bottom: 8 })

      // 镜像说明
      Text($r('app.string.about_mirror_note'))
        .fontSize(13)
        .margin({ bottom: 8 })

      // 版本信息
      Row() {
        Text($r('app.string.version_info'))
          .fontSize(16)
          .layoutWeight(1)
        Text($r('app.string.about_platform'))
          .fontSize(14)
      }
      .width('100%')

      // 工作区路径
      Row() {
        Text($r('app.string.workspace_path'))
          .fontSize(16)
          .layoutWeight(1)
      }
      .width('100%')
      .margin({ top: 8 })
    }
    .width('100%')
  }

  // ── 判断是否需要显示分组标题（单分类时不画分组标题，只画 ExpandableCategory 自己的标题） ──
  private shouldShowGroupHeader(groupDef: GroupDefinition): boolean {
    return groupDef.sections.length > 1
  }

  // ════════════════════════════════════════════
  // 辅助方法 — 分组标题
  // ════════════════════════════════════════════
  getGroupTitle(groupId: SettingsGroupId): string {
    switch (groupId) {
      case 'appearance':
        return getContext(this).resourceManager.getStringSync($r('app.string.appearance').id)
      case 'writing':
        return '写作'
      case 'data_sync':
        return '数据与同步'
      case 'advanced':
        return '高级'
      case 'about':
        return getContext(this).resourceManager.getStringSync($r('app.string.about_title').id)
      default:
        return ''
    }
  }

  // ── 分类标题 ──
  getSectionTitle(sectionId: SettingsSectionId): string {
    switch (sectionId) {
      case 'appearance':
        return getContext(this).resourceManager.getStringSync($r('app.string.appearance').id)
      case 'editor':
        return getContext(this).resourceManager.getStringSync($r('app.string.editor').id)
      case 'save':
        return getContext(this).resourceManager.getStringSync($r('app.string.save_section').id)
      case 'sync':
        return getContext(this).resourceManager.getStringSync($r('app.string.sync_section').id)
      case 'recovery':
        return '恢复'
      case 'ai':
        return getContext(this).resourceManager.getStringSync($r('app.string.ai_section').id)
      case 'diagnostics':
        return '诊断与日志'
      case 'laboratory':
        return '实验室'
      case 'about':
        return getContext(this).resourceManager.getStringSync($r('app.string.about_title').id)
      default:
        return ''
    }
  }

  // ── 分类搜索 token — 返回该分类下所有具体设置项标题，用于搜索匹配 ──
  getSectionSearchTerms(sectionId: SettingsSectionId): string[] {
    const ctx = getContext(this)
    switch (sectionId) {
      case 'appearance':
        return [
          ctx.resourceManager.getStringSync($r('app.string.theme_mode').id),
          '内置主题（实验性）',
          ctx.resourceManager.getStringSync($r('app.string.font_size').id),
          ctx.resourceManager.getStringSync($r('app.string.line_spacing_multiplier').id)
        ]
      case 'editor':
        return [
          ctx.resourceManager.getStringSync($r('app.string.auto_indent').id),
          ctx.resourceManager.getStringSync($r('app.string.indent_width').id)
        ]
      case 'save':
        return [
          ctx.resourceManager.getStringSync($r('app.string.auto_save').id),
          ctx.resourceManager.getStringSync($r('app.string.auto_save_delay').id)
        ]
      case 'sync':
        return [
          ctx.resourceManager.getStringSync($r('app.string.enable_sync').id),
          ctx.resourceManager.getStringSync($r('app.string.auto_sync').id),
          ctx.resourceManager.getStringSync($r('app.string.repository_https_url').id),
          ctx.resourceManager.getStringSync($r('app.string.access_token_optional').id),
          ctx.resourceManager.getStringSync($r('app.string.advanced_settings').id),
          ctx.resourceManager.getStringSync($r('app.string.sync_interval').id),
          ctx.resourceManager.getStringSync($r('app.string.diagnose').id)
        ]
      case 'recovery':
        return ['恢复功能即将上线']
      case 'ai':
        return [ctx.resourceManager.getStringSync($r('app.string.enable_ai').id)]
      case 'diagnostics':
        return ['开启诊断日志', '详细日志', '导出诊断包', '清空日志', '复制设备信息']
      case 'laboratory':
        return ['实验室功能即将上线']
      case 'about':
        return [
          ctx.resourceManager.getStringSync($r('app.string.about_app_name_label').id),
          ctx.resourceManager.getStringSync($r('app.string.about_author_label').id),
          ctx.resourceManager.getStringSync($r('app.string.about_project_label').id),
          ctx.resourceManager.getStringSync($r('app.string.about_license_label').id),
          ctx.resourceManager.getStringSync($r('app.string.version_info').id),
          ctx.resourceManager.getStringSync($r('app.string.workspace_path').id)
        ]
      default:
        return []
    }
  }

  // ── 分类当前值摘要 ──
  getSectionValueSummary(sectionId: SettingsSectionId): string {
    switch (sectionId) {
      case 'appearance': {
        const mode = this.viewModel.settings.appearanceMode || this.viewModel.settings.themeMode || 'system'
        if (mode === 'light') return getContext(this).resourceManager.getStringSync($r('app.string.light_mode').id)
        if (mode === 'dark') return getContext(this).resourceManager.getStringSync($r('app.string.dark_mode').id)
        return getContext(this).resourceManager.getStringSync($r('app.string.follow_system').id)
      }
      case 'editor':
        return `${this.viewModel.settings.editorFontSize}px`
      case 'save':
        return this.viewModel.settings.autoSaveEnabled ? '开启' : '关闭'
      case 'sync':
        return this.viewModel.syncConfig?.enabled ? '开启' : '关闭'
      case 'recovery':
        return ''
      case 'ai':
        return this.viewModel.settings.aiEnabled ? '开启' : '关闭'
      case 'diagnostics':
        return this.viewModel.settings.diagnosticsEnabled ? '开启' : '关闭'
      case 'laboratory':
        return ''
      case 'about':
        return ''
      default:
        return ''
    }
  }

  // ── 下拉选项辅助方法 ──
  getAppearanceOptions(): SettingsSelectOption[] {
    return [
      { value: getContext(this).resourceManager.getStringSync($r('app.string.follow_system').id), icon: '' },
      { value: getContext(this).resourceManager.getStringSync($r('app.string.light_mode').id), icon: '' },
      { value: getContext(this).resourceManager.getStringSync($r('app.string.dark_mode').id), icon: '' }
    ]
  }

  getBuiltinThemeOptions(): SettingsSelectOption[] {
    return this.viewModel.builtinThemes.map((theme: BuiltinThemeDto): SettingsSelectOption => {
      return { value: theme.name, icon: '' }
    })
  }

  getThemeIndex(): number {
    const mode = this.viewModel.settings.appearanceMode || this.viewModel.settings.themeMode || 'system'
    const themes = ['system', 'light', 'dark']
    return themes.indexOf(mode)
  }

  getThemeName(): string {
    const mode = this.viewModel.settings.appearanceMode || this.viewModel.settings.themeMode || 'system'
    if (mode === 'system') return getContext(this).resourceManager.getStringSync($r('app.string.follow_system').id)
    if (mode === 'light') return getContext(this).resourceManager.getStringSync($r('app.string.light_mode').id)
    if (mode === 'dark') return getContext(this).resourceManager.getStringSync($r('app.string.dark_mode').id)
    return getContext(this).resourceManager.getStringSync($r('app.string.follow_system').id)
  }

  getLocaleOptions(): SettingsSelectOption[] {
    return [
      { value: '跟随系统', icon: '' },
      { value: '简体中文', icon: '' },
      { value: 'English', icon: '' }
    ]
  }

  getLocaleIndex(): number {
    const locales: string[] = ['', 'zh-CN', 'en-US']
    const current: string = this.viewModel.settings.locale ?? ''
    const index: number = locales.indexOf(current)
    return index >= 0 ? index : 0
  }

  getLocaleName(): string {
    const names: string[] = ['跟随系统', '简体中文', 'English']
    return names[this.getLocaleIndex()]
  }

  getSelectedBuiltinThemeIndex(): number {
    const id = this.viewModel.settings.selectedBuiltinThemeId || ''
    if (id.length === 0) return 0
    return this.viewModel.builtinThemes.findIndex(t => t.theme_id === id)
  }

  getSelectedBuiltinThemeName(): string {
    const id = this.viewModel.settings.selectedBuiltinThemeId || ''
    if (id.length === 0) return '素笺默认'
    const theme = this.viewModel.builtinThemes.find(t => t.theme_id === id)
    return theme ? theme.name : '素笺默认'
  }

  // ════════════════════════════════════════════
  // Issue #972：宽屏浮层主体 — 两列分类卡片
  // 外框 / 标题 / 搜索由 WideSettingsOverlay 拥有；这里只负责把现有分类
  // 拆成两列，并用 WideSettingsSection 包裹现有设置项 builder。
  // ════════════════════════════════════════════

  // 搜索过滤后的分类顺序：沿用窄屏分组定义，但宽屏每张卡片直接对应一个分类。
  private overlaySectionIds(): SettingsSectionId[] {
    const groups: GroupDefinition[] = this.deriveFilteredGroups(this.activeSearchText())
    const result: SettingsSectionId[] = []
    for (const groupDef of groups) {
      for (const sectionId of groupDef.sections) {
        result.push(sectionId)
      }
    }
    return result
  }

  // 两列按分类数量均分，第一列多一个；列内各自滚动，长分类不会把另一列顶走。
  private overlayColumnSections(columnIndex: number): SettingsSectionId[] {
    const sections: SettingsSectionId[] = this.overlaySectionIds()
    const split: number = Math.ceil(sections.length / 2)
    return columnIndex === 0 ? sections.slice(0, split) : sections.slice(split)
  }

  @Builder
  WideOverlaySettingsBody() {
    Column() {
      // 浮层 header 由外壳拥有；保存 / 复制结果仍来自本组件的 ViewModel，在这里轻量提示。
      if (this.viewModel.saveMessage || this.copyMessage) {
        Row() {
          Blank()
          if (this.viewModel.saveMessage) {
            Text(this.resolveSaveMessage())
              .fontSize(12)
          }
          if (this.copyMessage) {
            Text(this.copyMessage)
              .fontSize(12)
              .margin({ left: 8 })
          }
        }
        .width('100%')
        .padding({ left: '11.5%', right: '11.5%', bottom: 4 })
      }

      if (this.overlaySectionIds().length === 0) {
        Column() {
          Text('没有匹配的设置')
            .fontSize(14)
            .fontColor($r('sys.color.font_secondary'))
        }
        .width('100%')
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
      } else {
        // 两列各 layoutWeight(1)；左右留白各占浮层宽 11.5%，中间留白占 6.3% 作为列间距
        // （参考图约 118 / 65 / 118vp）。留白用百分比占位而不是固定 vp，窗口变化时同比缩放。
        Row() {
          Column() {}
            .width('11.5%')

          this.WideOverlaySettingsColumn(this.overlayColumnSections(0), 0)

          Column() {}
            .width('6.3%')

          this.WideOverlaySettingsColumn(this.overlayColumnSections(1), 1)

          Column() {}
            .width('11.5%')
        }
        .width('100%')
        .layoutWeight(1)
        .alignItems(VerticalAlign.Top)
      }
    }
    .width('100%')
    .layoutWeight(1)
    .padding({ bottom: '3%' })
  }

  @Builder
  WideOverlaySettingsColumn(sectionIds: SettingsSectionId[], columnIndex: number) {
    Scroll(columnIndex === 0 ? this.wideColumnScroller0 : this.wideColumnScroller1) {
      Column({ space: 12 }) {
        ForEach(sectionIds, (sectionId: SettingsSectionId) => {
          WideSettingsSection({
            title: this.getSectionTitle(sectionId),
            summary: this.getSectionValueSummary(sectionId),
            expanded: this.isSectionExpandedForDisplay(sectionId),
            onToggle: () => {
              SettingsSectionContext.setCurrentSection(sectionId)
              this.recordContinuationSection(sectionId, '')
              this.viewModel.toggleSection(sectionId)
            }
          }) {
            WideSettingsCategoryContent({
              sectionId: sectionId,
              viewModel: this.viewModel,
              deviceSync: this.deviceSync,
              idPrefix: this.instanceToken,
              highlightedSettingKey: this.highlightedSettingKey,
              conflictRemaining: this.conflictRemaining,
              gripSide: this.gripSide,
              onSetThemeMode: (mode: ThemeMode): void => {
                this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
                  s.appearanceMode = mode
                  s.themeMode = mode
                  return s
                })
                this.themeAdapter.setMode(mode)
                this.viewModel.saveSettings()
              },
              onSetBuiltinTheme: (themeId: string): void => {
                this.viewModel.updateSettings((s: LocalSettings): LocalSettings => {
                  s.selectedBuiltinThemeId = themeId
                  return s
                })
                this.themeAdapter.setSelectedBuiltinThemeId(themeId)
                this.viewModel.saveSettings()
              },
              onRequestSync: (): void => {
                void this.requestSync()
              },
              onOpenConflicts: (): void => {
                this.openConflictsPanel()
              },
              onOpenThirdPartyNotices: (): void => {
                this.openThirdPartyNoticesPanel()
              },
              onExportDiagnostics: (): void => {
                void this.viewModel.exportDiagnostics().then((result) => {
                  promptAction.showToast({ message: result.status === 'success' ? '诊断包已导出' : '诊断包导出失败' })
                })
              },
              onClearLogs: (): void => {
                this.viewModel.clearLogs()
              },
              onCopyDeviceInfo: (): void => {
                void this.viewModel.copyDeviceInfoToClipboard().then((ok: boolean) => {
                  this.copyMessage = ok ? '已复制' : '复制失败'
                  setTimeout(() => { this.copyMessage = '' }, 2000)
                })
              }
            })
          }
        }, (sectionId: SettingsSectionId) => `wide_${columnIndex}_${sectionId}`)
      }
      .width('100%')
    }
    .id(`settings_${this.instanceToken}_wide_scroll_${columnIndex}`)
    .layoutWeight(1)
    .height('100%')
    .scrollBar(BarState.Off)
    .align(Alignment.Top)
  }

  build() {
    this.NavContent()
  }
}
