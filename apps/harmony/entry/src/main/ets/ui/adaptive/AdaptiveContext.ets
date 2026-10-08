import window from '@ohos.window'
import display from '@ohos.display'
import hilog from '@ohos.hilog'
import { getBridge } from '../../app/AppContext'
import { WindowViewport, LayoutPlan, LayoutContract, FoldPosture, WidthClass, ShellMode, WorkspaceLayoutMode, PrimaryNavigationPlacement } from '../../corebridge/dto/PlatformDtos'
import { harmonyAvailablePaneCount, buildHarmonyLayoutPlan, defaultLayoutMetrics } from './LayoutPolicyHelper'
import { PlatformApiResolver } from '../../platform/version/PlatformApiResolver'
import { TabletWindowPolicy } from '../../platform/window/TabletWindowPolicy'

/**
 * 响应式布局状态 — 直接映射 Core LayoutPlan
 */
export interface AdaptiveState {
  widthVp: number
  heightVp: number
  layoutPlan: LayoutPlan
  // 便捷访问属性（从 layoutPlan 派生）
  sizeClass: WidthClass
  isTwoPane: boolean
  contentMaxWidth: number
  pagePadding: number
  gridColumns: number
  // 底部安全区域高度（导航栏避让），单位 vp
  safeBottomVp: number
  // 顶部安全区域高度（状态栏避让），单位 vp
  safeTopVp: number
  // 当前是否偏好宽屏布局（专用横屏平板且 Core 返回 Workbench 模式，或 sizeClass === Expanded）
  prefersWideLayout: boolean
  // 是否是专用横屏平板（deviceType=tablet + 非折叠 + 宽高比>=1.25）
  isDedicatedLandscapeTablet: boolean
}

/**
 * 窗口尺寸变化观察者
 */
export interface AdaptiveObserver {
  onWindowSizeChange(state: AdaptiveState): void
}

/**
 * AdaptiveObserver 的便捷实现类
 *
 * ArkTS 要求对象字面量必须有显式类型声明（arkts-no-untyped-obj-literals），
 * 因此提供此类替代在各页面中直接使用匿名对象字面量。
 */
export class SimpleAdaptiveObserver implements AdaptiveObserver {
  private callback: (state: AdaptiveState) => void

  constructor(callback: (state: AdaptiveState) => void) {
    this.callback = callback
  }

  onWindowSizeChange(state: AdaptiveState): void {
    this.callback(state)
  }
}

const TAG = 'AdaptiveContext'
const DOMAIN = 0x0000

/**
 * 响应式断点上下文（单例）
 *
 * 监听窗口尺寸变化，通过 bridge.resolveLayout 获取布局方案，通知所有订阅者。
 * 在 EntryAbility.onWindowStageCreate 中初始化。
 */
export class AdaptiveContext {
  private static instance: AdaptiveContext
  private state: AdaptiveState = AdaptiveContext.createDefaultState()
  private observers: Set<AdaptiveObserver> = new Set()
  private mainWindow: window.Window | null = null
  private isListening: boolean = false
  private isAvoidAreaListening: boolean = false

  // 缓存平台真实值，avoidAreaChange 时增量更新
  private cachedSafeTopVp: number = 0
  private cachedSafeBottomVp: number = 0
  private cachedKeyboardVisible: boolean = false
  private cachedFoldPosture: FoldPosture = FoldPosture.Unknown
  // 专用横屏平板判定（只算一次设备能力）
  private cachedIsDedicatedLandscapeTablet: boolean = false
  // 缓存上次的窗口状态（用于检测全屏 ↔ 分屏/自由窗口切换）
  private cachedWindowStatus: window.WindowStatusType = window.WindowStatusType.UNDEFINED
  // 窗口状态变更串行化：Promise 链确保前一个 apply/restore 完成后才执行下一个
  private windowStatusTask: Promise<void> = Promise.resolve()
  // 状态序号：递增 generation，过期的请求丢弃
  private windowStatusGeneration: number = 0

  private constructor() {}

  static getInstance(): AdaptiveContext {
    if (!AdaptiveContext.instance) {
      AdaptiveContext.instance = new AdaptiveContext()
    }
    return AdaptiveContext.instance
  }

  /**
   * 在 EntryAbility.onWindowStageCreate 中调用，
   * 获取主窗口并注册尺寸变化监听。
   */
  async initialize(windowStage: window.WindowStage): Promise<void> {
    try {
      this.mainWindow = await windowStage.getMainWindow()
      hilog.info(DOMAIN, TAG, 'Got main window')

      // 初始化专用横屏平板判定（只算一次设备能力）
      this.cachedIsDedicatedLandscapeTablet = TabletWindowPolicy.isDedicatedLandscapeTablet()

      // 读取初始安全区域 / 键盘 / 折叠状态
      this.readSafeAreas()
      this.readKeyboardVisible()
      this.readFoldPosture()
      // 读取初始窗口状态
      this.readWindowStatus()

      // 底部手势条（导航指示器）沉浸式适配。
      // ArkUI 默认把页面内容布局在安全区内，实机 dumpLayout 证实 Navigation / NavDestination /
      // Scroll 全部被夹在 y=2603，屏幕底部 2603~2690 是一条画不出任何内容的死区：
      // 滚动时列表项从 2603 这条白带"上面"经过，而不是从屏幕底边往上走。
      // setWindowLayoutFullScreen(true) 让窗口内容铺满整屏，Navigation / NavDestination /
      // Scroll 随即延伸到 y=2690，滚动内容得以经过手势条区域。
      // 门控在 API 23：HDS 的 HdsNavigation 标题栏要靠 avoidLayoutSafeArea（API 20+）
      // 才会继续避让顶部状态栏，两者配套才有正确结果；API 23 以下走非 HDS 壳，
      // 保持改动前的默认避让行为，不引入未验证的沉浸态。
      if (PlatformApiResolver.getInstance().isApiAtLeast(23)) {
        if (this.cachedIsDedicatedLandscapeTablet) {
          // 专用横屏平板：不在这里设全屏，交给 TabletWindowPolicy.applyTabletWindowPolicy
          // （它已经有 setWindowLayoutFullScreen(true) + 隐藏系统栏 + 强制横屏）
          hilog.info(DOMAIN, TAG, 'Dedicated landscape tablet: deferring full screen to TabletWindowPolicy')
        } else {
          this.mainWindow.setWindowLayoutFullScreen(true)
          hilog.info(DOMAIN, TAG, 'Window layout full screen enabled for bottom inset immersion')
        }
      }

      // Issue #752 评论 5808818396：先注册窗口尺寸变化监听，再读取当前 windowRect。
      // 监听先挂上，保证后续尺寸变化不会漏；当前 windowRect 若仍是 0x0 则什么都不提交，
      // 等已经注册好的监听收到第一组有效尺寸。
      this.registerWindowSizeChange()

      // 读取初始尺寸
      const properties = this.mainWindow.getWindowProperties()
      const rect = properties.windowRect
      const displayInfo = display.getDefaultDisplaySync()
      const density = displayInfo.densityPixels
      const widthVp = rect.width / density
      const heightVp = rect.height / density
      // Issue #752：实机冷启动首帧 windowRect 可能是 0x0，不当作有效尺寸写入布局状态。
      // 只注册监听，等 windowSizeChange 回调带来真实尺寸，避免 "Initialized: Compact 0x0"。
      if (widthVp <= 0 || heightVp <= 0) {
        hilog.info(DOMAIN, TAG, 'Window rect not ready: %{public}sx%{public}s, deferring until first valid size', `${widthVp}`, `${heightVp}`)
      } else {
        await this.updateState(widthVp, heightVp)
        hilog.info(DOMAIN, TAG, `Initialized: ${this.state.sizeClass} ${this.state.widthVp}x${this.state.heightVp}`)
      }

      // 注册避让区域变化监听（安全区域 + 键盘）
      this.registerAvoidAreaChange()
      // 注册折叠状态变化监听
      this.registerFoldStatusChange()
      // 注册窗口状态变化监听（检测全屏 ↔ 分屏/自由窗口切换）
      this.registerWindowStatusChange()
    } catch (err) {
      hilog.error(DOMAIN, TAG, `Failed to initialize: ${JSON.stringify(err)}`)
    }
  }

  /**
   * 获取当前响应式状态（返回副本）
   */
  getCurrentState(): AdaptiveState {
    return {
      widthVp: this.state.widthVp,
      heightVp: this.state.heightVp,
      layoutPlan: this.state.layoutPlan,
      sizeClass: this.state.sizeClass,
      isTwoPane: this.state.isTwoPane,
      contentMaxWidth: this.state.contentMaxWidth,
      pagePadding: this.state.pagePadding,
      gridColumns: this.state.gridColumns,
      safeBottomVp: this.cachedSafeBottomVp,
      safeTopVp: this.cachedSafeTopVp,
      prefersWideLayout: this.state.prefersWideLayout,
      isDedicatedLandscapeTablet: this.state.isDedicatedLandscapeTablet
    }
  }

  /**
   * 订阅窗口尺寸变化
   */
  addObserver(observer: AdaptiveObserver): void {
    this.observers.add(observer)
    // 订阅时立即同步一次当前状态，避免订阅方停留在默认的 Compact 布局
    try {
      observer.onWindowSizeChange(this.getCurrentState())
    } catch (error) {
      hilog.error(DOMAIN, TAG, `addObserver initial notify failed: ${JSON.stringify(error)}`)
    }
  }

  /**
   * 取消订阅
   */
  removeObserver(observer: AdaptiveObserver): void {
    this.observers.delete(observer)
  }

  /**
   * 便捷订阅：传入回调函数
   * 返回的函数可用于取消订阅
   */
  onWindowSizeChange(callback: (state: AdaptiveState) => void): () => void {
    const observer: AdaptiveObserver = new SimpleAdaptiveObserver(callback)
    // addObserver 内部已立即推送一次当前状态
    this.addObserver(observer)
    // 返回取消订阅函数
    return () => {
      this.removeObserver(observer)
    }
  }

  /**
   * 停止监听并清理资源
   */
  dispose(): void {
    this.unregisterWindowSizeChange()
    this.unregisterAvoidAreaChange()
    this.unregisterFoldStatusChange()
    this.unregisterWindowStatusChange()
    this.observers.clear()
    this.mainWindow = null
    this.cachedSafeTopVp = 0
    this.cachedSafeBottomVp = 0
    this.cachedKeyboardVisible = false
    this.cachedFoldPosture = FoldPosture.Unknown
  }

  // ========== 私有方法 ==========

  private registerWindowSizeChange(): void {
    if (this.isListening || !this.mainWindow) {
      return
    }
    try {
      this.mainWindow.on('windowSizeChange', (data: window.Size) => {
        if (!this.mainWindow) {
          return
        }
        // Issue #752：忽略 0x0 尺寸回调，不写入布局状态。
        if (data.width <= 0 || data.height <= 0) {
          hilog.info(DOMAIN, TAG, 'windowSizeChange ignored: %{public}sx%{public}s (zero size)', `${data.width}`, `${data.height}`)
          return
        }
        const displayInfo = display.getDefaultDisplaySync()
        const density = displayInfo.densityPixels
        const widthVp = data.width / density
        const heightVp = data.height / density
        this.updateState(widthVp, heightVp)
      })
      this.isListening = true
      hilog.info(DOMAIN, TAG, 'Registered windowSizeChange listener')
    } catch (err) {
      hilog.error(DOMAIN, TAG, `Failed to register windowSizeChange: ${JSON.stringify(err)}`)
    }
  }

  private unregisterWindowSizeChange(): void {
    if (!this.isListening || !this.mainWindow) {
      return
    }
    try {
      this.mainWindow.off('windowSizeChange')
      this.isListening = false
      hilog.info(DOMAIN, TAG, 'Unregistered windowSizeChange listener')
    } catch (err) {
      hilog.error(DOMAIN, TAG, `Failed to unregister windowSizeChange: ${JSON.stringify(err)}`)
    }
  }

  private async updateState(widthVp: number, heightVp: number): Promise<void> {
    // Issue #752：防御性判断，0x0 尺寸不写入布局状态。
    if (widthVp <= 0 || heightVp <= 0) {
      return
    }
    // Issue #752：首次拿到非 0 有效尺寸时记录首帧路标日志。
    const isFirstValidSize: boolean = this.state.widthVp <= 0 || this.state.heightVp <= 0
    const bridge = getBridge()
    // #610/#753：平台端只上报测量到的原始窗口尺寸（dp），
    // paneCount / 折叠 / 指针 / 键盘都不再是 Core 的输入，由 Core 自己从尺寸推导。
    const viewport: WindowViewport = {
      widthDp: Math.round(widthVp * 100) / 100,
      heightDp: Math.round(heightVp * 100) / 100,
      occlusions: []
    }

    try {
      const result = await bridge.resolveLayout(viewport)
      const contract: LayoutContract | undefined = result.data
      if (result.success && contract !== undefined) {
        // Harmony 平台值（纸面宽度/内边距/网格列数）在平台层叠加，不在 Core。
        const plan: LayoutPlan = buildHarmonyLayoutPlan(contract, widthVp)
        const sizeClass: WidthClass = this.harmonySizeClass(widthVp)
        // prefersWideLayout: 专用横屏平板且 Core 返回 Workbench 模式（能展示完整宽屏工作台），或 sizeClass === Expanded
        // 不能只按 600vp 判定：600-695vp 时 Core 返回 SinglePane（最小 workbench 宽度 = 696dp），
        // 此时进入 Writing 会出现无章节树且无法新建章节的死状态。
        // 用 Core 的 workspaceLayoutMode 判断，确保只在真正能展示工作台时才启用宽屏布局。
        const prefersWideLayout: boolean =
          (this.cachedIsDedicatedLandscapeTablet && contract.workspaceLayoutMode === WorkspaceLayoutMode.Workbench) || sizeClass === WidthClass.Expanded
        this.state = {
          widthVp: Math.round(widthVp * 100) / 100,
          heightVp: Math.round(heightVp * 100) / 100,
          layoutPlan: plan,
          sizeClass: sizeClass,
          isTwoPane: contract.shellMode === ShellMode.TwoPane,
          contentMaxWidth: plan.contentMaxWidthVp === 0 ? Number.MAX_SAFE_INTEGER : plan.contentMaxWidthVp,
          pagePadding: plan.pagePaddingVp,
          gridColumns: plan.gridColumns,
          safeBottomVp: this.cachedSafeBottomVp,
          safeTopVp: this.cachedSafeTopVp,
          prefersWideLayout: prefersWideLayout,
          isDedicatedLandscapeTablet: this.cachedIsDedicatedLandscapeTablet
        }
        // Issue #752：第一次拿到非 0 有效尺寸的首帧路标日志。
        if (isFirstValidSize) {
          hilog.info(DOMAIN, TAG, 'First valid window size: %{public}sx%{public}s vp', `${this.state.widthVp}`, `${this.state.heightVp}`)
        }
        this.notifyObservers()
      }
    } catch (err) {
      hilog.error(DOMAIN, TAG, `resolveLayout failed: ${JSON.stringify(err)}`)
    }
  }

  /** Harmony 自身的大小分类（平台决策，不在 Core），与 harmonyAvailablePaneCount 同一断点。 */
  private harmonySizeClass(widthVp: number): WidthClass {
    const paneCount = harmonyAvailablePaneCount(widthVp)
    if (paneCount <= 1) {
      return WidthClass.Compact
    }
    if (paneCount === 2) {
      return WidthClass.Medium
    }
    return WidthClass.Expanded
  }

  private notifyObservers(): void {
    const state = this.getCurrentState()
    this.observers.forEach(obs => {
      try {
        obs.onWindowSizeChange(state)
      } catch (err) {
        hilog.error(DOMAIN, TAG, `Observer error: ${JSON.stringify(err)}`)
      }
    })
  }

  private static createDefaultState(): AdaptiveState {
    const defaultPlan: LayoutPlan = {
      shellMode: ShellMode.SinglePane,
      workspaceLayoutMode: WorkspaceLayoutMode.SinglePane,
      primaryNavigationPlacement: PrimaryNavigationPlacement.Bottom,
      contentMaxWidthVp: 0,
      pagePaddingVp: 16,
      gridColumns: 2,
      metrics: defaultLayoutMetrics()
    }
    return {
      widthVp: 0,
      heightVp: 0,
      layoutPlan: defaultPlan,
      sizeClass: WidthClass.Compact,
      isTwoPane: false,
      contentMaxWidth: Number.MAX_SAFE_INTEGER,
      pagePadding: 16,
      gridColumns: 2,
      safeBottomVp: 0,
      safeTopVp: 0,
      prefersWideLayout: false,
      isDedicatedLandscapeTablet: false
    }
  }

  // ========== 安全区域 / 键盘 / 折叠 读取方法 ==========

  /**
   * 读取系统栏避让区域，更新 cachedSafeTopVp / cachedSafeBottomVp
   */
  private readSafeAreas(): void {
    if (!this.mainWindow) {
      return
    }
    try {
      const displayInfo = display.getDefaultDisplaySync()
      const density = displayInfo.densityPixels

      // 系统栏（状态栏）避让区域 → safeTopVp
      const systemAvoidArea = this.mainWindow.getWindowAvoidArea(window.AvoidAreaType.TYPE_SYSTEM)
      this.cachedSafeTopVp = systemAvoidArea.topRect.height / density

      // 底部导航栏避让区域 → safeBottomVp
      const navigationAvoidArea = this.mainWindow.getWindowAvoidArea(window.AvoidAreaType.TYPE_NAVIGATION_INDICATOR)
      this.cachedSafeBottomVp = navigationAvoidArea.bottomRect.height / density

      hilog.info(DOMAIN, TAG, `SafeArea: top=${this.cachedSafeTopVp}vp, bottom=${this.cachedSafeBottomVp}vp`)
    } catch (err) {
      hilog.error(DOMAIN, TAG, `readSafeAreas failed: ${JSON.stringify(err)}`)
    }
  }

  /**
   * 检测键盘是否可见，更新 cachedKeyboardVisible
   */
  private readKeyboardVisible(): void {
    if (!this.mainWindow) {
      return
    }
    try {
      const keyboardAvoidArea = this.mainWindow.getWindowAvoidArea(window.AvoidAreaType.TYPE_KEYBOARD)
      this.cachedKeyboardVisible = keyboardAvoidArea.bottomRect.height > 0
      hilog.info(DOMAIN, TAG, `KeyboardVisible: ${this.cachedKeyboardVisible}`)
    } catch (err) {
      hilog.error(DOMAIN, TAG, `readKeyboardVisible failed: ${JSON.stringify(err)}`)
    }
  }

  /**
   * 读取折叠状态，更新 cachedFoldPosture
   * 注意：display.getDisplayFoldStatus() 不在标准 API 类型定义中，
   * 折叠状态改由 onConfigurationUpdate 回调注入，此处保持 Unknown。
   */
  private readFoldPosture(): void {
    // getDisplayFoldStatus() 不在标准 display 类型上，CI 构建不通过。
    // 折叠状态由外部通过 updateFoldPosture() 注入。
    this.cachedFoldPosture = FoldPosture.Unknown
    hilog.info(DOMAIN, TAG, `FoldPosture: Unknown (deferred to external injection)`)
  }

  /**
   * 读取初始窗口状态，更新 cachedWindowStatus。
   * 用 window.Window.getWindowStatus()（API 12+），不用 WindowProperties——
   * WindowProperties 没有窗口模式字段。
   */
  private readWindowStatus(): void {
    if (!this.mainWindow) {
      return
    }
    try {
      this.cachedWindowStatus = this.mainWindow.getWindowStatus()
      hilog.info(DOMAIN, TAG, `Initial windowStatus: ${this.cachedWindowStatus}`)
    } catch (err) {
      hilog.error(DOMAIN, TAG, `readWindowStatus failed: ${JSON.stringify(err)}`)
    }
  }

  /**
   * 外部注入折叠状态（如从 onConfigurationUpdate 回调中调用）
   */
  updateFoldPosture(posture: FoldPosture): void {
    if (this.cachedFoldPosture !== posture) {
      this.cachedFoldPosture = posture
      hilog.info(DOMAIN, TAG, `FoldPosture updated: ${posture}`)
    }
  }

  // ========== 避让区域变化监听 ==========

  private registerAvoidAreaChange(): void {
    if (this.isAvoidAreaListening || !this.mainWindow) {
      return
    }
    try {
      this.mainWindow.on('avoidAreaChange', (data: window.AvoidAreaOptions) => {
        if (!this.mainWindow) {
          return
        }
        const displayInfo = display.getDefaultDisplaySync()
        const density = displayInfo.densityPixels
        let changed = false

        if (data.type === window.AvoidAreaType.TYPE_SYSTEM) {
          const newTop = data.area.topRect.height / density
          if (this.cachedSafeTopVp !== newTop) {
            this.cachedSafeTopVp = newTop
            changed = true
          }
        }

        if (data.type === window.AvoidAreaType.TYPE_NAVIGATION_INDICATOR) {
          const newBottom = data.area.bottomRect.height / density
          if (this.cachedSafeBottomVp !== newBottom) {
            this.cachedSafeBottomVp = newBottom
            changed = true
          }
        }

        if (data.type === window.AvoidAreaType.TYPE_KEYBOARD) {
          const newKeyboardVisible = data.area.bottomRect.height > 0
          if (this.cachedKeyboardVisible !== newKeyboardVisible) {
            this.cachedKeyboardVisible = newKeyboardVisible
            changed = true
          }
        }

        // 有变化时重新计算布局
        if (changed) {
          const properties = this.mainWindow.getWindowProperties()
          const rect = properties.windowRect
          const widthVp = rect.width / density
          const heightVp = rect.height / density
          // Issue #752：忽略 0x0 尺寸，不写入布局状态。
          if (widthVp > 0 && heightVp > 0) {
            this.updateState(widthVp, heightVp)
          }
        }
      })
      this.isAvoidAreaListening = true
      hilog.info(DOMAIN, TAG, 'Registered avoidAreaChange listener')
    } catch (err) {
      hilog.error(DOMAIN, TAG, `Failed to register avoidAreaChange: ${JSON.stringify(err)}`)
    }
  }

  private unregisterAvoidAreaChange(): void {
    if (!this.isAvoidAreaListening || !this.mainWindow) {
      return
    }
    try {
      this.mainWindow.off('avoidAreaChange')
      this.isAvoidAreaListening = false
      hilog.info(DOMAIN, TAG, 'Unregistered avoidAreaChange listener')
    } catch (err) {
      hilog.error(DOMAIN, TAG, `Failed to unregister avoidAreaChange: ${JSON.stringify(err)}`)
    }
  }

  // ========== 折叠状态变化监听 ==========

  private registerFoldStatusChange(): void {
    try {
      display.on('foldStatusChange', (foldStatus: number) => {
        let newPosture: FoldPosture
        // display.FoldStatus 枚举值：FOLD_STATUS_EXPANDED=2, FOLD_STATUS_HALF_FOLDED=3, FOLD_STATUS_FOLDED=1
        if (foldStatus === 2) {
          newPosture = FoldPosture.FullyOpened
        } else if (foldStatus === 3) {
          newPosture = FoldPosture.HalfOpened
        } else if (foldStatus === 1) {
          newPosture = FoldPosture.Closed
        } else {
          newPosture = FoldPosture.Unknown
        }

        if (this.cachedFoldPosture !== newPosture) {
          this.cachedFoldPosture = newPosture
          hilog.info(DOMAIN, TAG, `FoldPosture changed: ${newPosture}`)

          // 折叠状态变化通常伴随尺寸变化，但保险起见也触发一次布局更新
          if (this.mainWindow) {
            const properties = this.mainWindow.getWindowProperties()
            const rect = properties.windowRect
            const displayInfo = display.getDefaultDisplaySync()
            const density = displayInfo.densityPixels
            const widthVp = rect.width / density
            const heightVp = rect.height / density
            // Issue #752：忽略 0x0 尺寸，不写入布局状态。
            if (widthVp > 0 && heightVp > 0) {
              this.updateState(widthVp, heightVp)
            }
          }
        }
      })
      hilog.info(DOMAIN, TAG, 'Registered foldStatusChange listener')
    } catch (err) {
      // 非折叠设备或不支持该 API，静默忽略
      hilog.info(DOMAIN, TAG, `foldStatusChange not supported: ${JSON.stringify(err)}`)
    }
  }

  private unregisterFoldStatusChange(): void {
    try {
      display.off('foldStatusChange')
      hilog.info(DOMAIN, TAG, 'Unregistered foldStatusChange listener')
    } catch (err) {
      // 静默忽略
    }
  }

  // ========== 窗口状态变化监听 ==========

  // Issue #880 复核：窗口状态变化（全屏 ↔ 分屏 / 悬浮窗）时重算系统栏与方向政策。
  // 只在真正回到独占全屏（WindowStatusType.FULL_SCREEN）时重新套用平板政策；
  // 分屏 / 悬浮 / 最大化一概恢复默认，避免把全屏宽屏 UI 硬套到小块可用窗口上。
  // 国产折叠屏 / 阔直屏不执行这套强制政策（cachedIsDedicatedLandscapeTablet 恒为 false）。
  // 窗口尺寸本身仍由 windowSizeChange 驱动，本监听不改布局状态。
  private registerWindowStatusChange(): void {
    if (!this.mainWindow) {
      return
    }
    try {
      this.mainWindow.on('windowStatusChange', (windowStatus: window.WindowStatusType) => {
        this.handleWindowStatusChange(windowStatus)
      })
      hilog.info(DOMAIN, TAG, 'Registered windowStatusChange listener')
    } catch (err) {
      // windowStatusChange 依赖 SystemCapability.Window.SessionManager，部分设备不可用，静默忽略
      hilog.info(DOMAIN, TAG, `windowStatusChange not supported: ${JSON.stringify(err)}`)
    }
  }

  private unregisterWindowStatusChange(): void {
    if (!this.mainWindow) {
      return
    }
    try {
      this.mainWindow.off('windowStatusChange')
      hilog.info(DOMAIN, TAG, 'Unregistered windowStatusChange listener')
    } catch (err) {
      // 静默忽略
    }
  }

  private handleWindowStatusChange(windowStatus: window.WindowStatusType): void {
    if (!this.mainWindow || !this.cachedIsDedicatedLandscapeTablet) {
      return
    }
    if (windowStatus === this.cachedWindowStatus) {
      return
    }
    const oldStatus: window.WindowStatusType = this.cachedWindowStatus
    this.cachedWindowStatus = windowStatus
    hilog.info(DOMAIN, TAG, `WindowStatus changed: ${oldStatus} → ${windowStatus}`)

    // 递增 generation，串到 windowStatusTask 链上确保串行执行
    this.windowStatusGeneration++
    const currentGeneration: number = this.windowStatusGeneration
    const mainWindow: window.Window = this.mainWindow

    this.windowStatusTask = this.windowStatusTask.then(() => {
      // 执行前检查 generation 是否仍是最新的，过期的丢弃
      if (currentGeneration !== this.windowStatusGeneration) {
        hilog.info(DOMAIN, TAG, `WindowStatus task generation ${currentGeneration} is stale (current: ${this.windowStatusGeneration}), skipping`)
        return
      }
      if (windowStatus === window.WindowStatusType.FULL_SCREEN) {
        // 真正回到独占全屏：重新套用平板窗口政策（强制横屏 + 隐藏系统栏）
        return TabletWindowPolicy.applyTabletWindowPolicy(mainWindow)
      } else {
        // 分屏 / 悬浮 / 最大化：恢复默认方向与系统栏，布局按真实 windowRect 走
        return TabletWindowPolicy.restoreWindowPolicy(mainWindow)
      }
    }).catch((err: Error) => {
      hilog.error(DOMAIN, TAG, `WindowStatus task failed: ${JSON.stringify(err)}`)
    })
  }
}

// ========== 便捷函数 ==========

export function getAdaptiveContext(): AdaptiveContext {
  return AdaptiveContext.getInstance()
}

export function getAdaptiveState(): AdaptiveState {
  return AdaptiveContext.getInstance().getCurrentState()
}
