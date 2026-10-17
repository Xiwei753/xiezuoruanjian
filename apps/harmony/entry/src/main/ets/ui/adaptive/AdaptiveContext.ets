import window from '@ohos.window'
import display from '@ohos.display'
import hilog from '@ohos.hilog'
import { FoldPosture, WidthClass } from '../../corebridge/dto/PlatformDtos'
import { PlatformApiResolver } from '../../platform/version/PlatformApiResolver'
import { TabletWindowPolicy } from '../../platform/window/TabletWindowPolicy'

// ── Harmony 平台布局断点（vp） ──
// Issue #920 D：页面结构完全由平台 UI 层决定，不再向 Core 要布局模式。
// 断点沿用产品既有的响应式设计，保证窗口尺寸变化时页面结构与改动前等价。
const WIDTH_MEDIUM_MIN_VP: number = 600
const WIDTH_WIDE_MIN_VP: number = 840
// 宽屏工作区内容的纸面最大宽度（vp）。
const WORKBENCH_CONTENT_MAX_WIDTH_VP: number = 840
// 页面水平内边距（vp）：工作台档 32，其他档 16。
const WIDE_PAGE_PADDING_VP: number = 32
const COMPACT_PAGE_PADDING_VP: number = 16
// 工作台档要求的最小高度（vp）：中等宽度但过矮时塞不下双栏，退回单栏。
const HEIGHT_MEDIUM_MIN_VP: number = 480

/**
 * 响应式布局状态 — Harmony 平台自行推导的窗口断点结果。
 *
 * Issue #920 D：不再包含 Core LayoutPlan；布局模式、纸面宽度、内边距与
 * 一级导航摆放都由本层按窗口尺寸本地决定。
 */
export interface AdaptiveState {
  widthVp: number
  heightVp: number
  // 平台自己的宽度分类（见 harmonySizeClass）
  sizeClass: WidthClass
  // 宽屏工作区（Workbench）档，决定纸面宽度 / 内边距 / 设置浮层形态
  isWorkbenchLayout: boolean
  // 一级导航放侧栏（宽窗口），否则放底部
  isWidePrimaryNavigation: boolean
  contentMaxWidth: number
  pagePadding: number
  // 底部安全区域高度（导航栏避让），单位 vp
  safeBottomVp: number
  // 顶部安全区域高度（状态栏避让），单位 vp
  safeTopVp: number
  // 当前是否偏好宽屏布局（专用横屏平板且进入工作台档，或 sizeClass === Expanded）
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
 * 响应式布局上下文（单例）
 *
 * 监听窗口尺寸变化，按 Harmony 平台自己的断点推导布局状态，通知所有订阅者。
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
      // 先注册窗口状态变化监听（确保不漏事件）
      this.registerWindowStatusChange()
      // 再读取当前窗口状态
      this.readWindowStatus()
      // 将初次已知状态送入 reconcileWindowPolicy 做首次政策应用
      if (this.cachedIsDedicatedLandscapeTablet && this.mainWindow) {
        this.reconcileWindowPolicy(this.cachedWindowStatus)
      }

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
        this.updateState(widthVp, heightVp)
        hilog.info(DOMAIN, TAG, `Initialized: ${this.state.sizeClass} ${this.state.widthVp}x${this.state.heightVp}`)
      }

      // 注册避让区域变化监听（安全区域 + 键盘）
      this.registerAvoidAreaChange()
      // 注册折叠状态变化监听
      this.registerFoldStatusChange()
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
      sizeClass: this.state.sizeClass,
      isWorkbenchLayout: this.state.isWorkbenchLayout,
      isWidePrimaryNavigation: this.state.isWidePrimaryNavigation,
      contentMaxWidth: this.state.contentMaxWidth,
      pagePadding: this.state.pagePadding,
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

  // Issue #920 D：按 Harmony 平台自己的断点本地推导布局状态，不再调用 Core。
  private updateState(widthVp: number, heightVp: number): void {
    // Issue #752：防御性判断，0x0 尺寸不写入布局状态。
    if (widthVp <= 0 || heightVp <= 0) {
      return
    }
    // Issue #752：首次拿到非 0 有效尺寸时记录首帧路标日志。
    const isFirstValidSize: boolean = this.state.widthVp <= 0 || this.state.heightVp <= 0
    const sizeClass: WidthClass = this.harmonySizeClass(widthVp)
    const isWorkbenchLayout: boolean = AdaptiveContext.isWorkbenchLayout(widthVp, heightVp)
    // prefersWideLayout: 专用横屏平板且进入工作台档（能展示完整宽屏工作台），或 sizeClass === Expanded。
    // 不能只按 600vp 判定：600-695vp 展示不下完整工作台，此时进入 Writing 会出现
    // 无章节树且无法新建章节的死状态，所以用 isWorkbenchLayout 兜一层。
    const prefersWideLayout: boolean =
      (this.cachedIsDedicatedLandscapeTablet && isWorkbenchLayout) || sizeClass === WidthClass.Expanded
    this.state = {
      widthVp: Math.round(widthVp * 100) / 100,
      heightVp: Math.round(heightVp * 100) / 100,
      sizeClass: sizeClass,
      isWorkbenchLayout: isWorkbenchLayout,
      // 一级导航放侧栏：宽窗口（>=840vp）与旧 Core 的 Side 判定一致。
      isWidePrimaryNavigation: widthVp >= WIDTH_WIDE_MIN_VP,
      contentMaxWidth: isWorkbenchLayout ? WORKBENCH_CONTENT_MAX_WIDTH_VP : Number.MAX_SAFE_INTEGER,
      pagePadding: isWorkbenchLayout ? WIDE_PAGE_PADDING_VP : COMPACT_PAGE_PADDING_VP,
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

  // 宽屏工作台档：宽度 >= 600vp，且不是「中等宽度 + 过矮窗口」。
  // 中等宽度（600-839vp）但高度 < 480vp 时塞不下双栏，退回单栏。
  private static isWorkbenchLayout(widthVp: number, heightVp: number): boolean {
    if (widthVp < WIDTH_MEDIUM_MIN_VP) {
      return false
    }
    if (widthVp < WIDTH_WIDE_MIN_VP && heightVp < HEIGHT_MEDIUM_MIN_VP) {
      return false
    }
    return true
  }

  /** Harmony 自身的大小分类（平台决策，不在 Core）。 */
  private harmonySizeClass(widthVp: number): WidthClass {
    if (widthVp < WIDTH_MEDIUM_MIN_VP) {
      return WidthClass.Compact
    }
    if (widthVp < WIDTH_WIDE_MIN_VP) {
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
    return {
      widthVp: 0,
      heightVp: 0,
      sizeClass: WidthClass.Compact,
      isWorkbenchLayout: false,
      isWidePrimaryNavigation: false,
      contentMaxWidth: Number.MAX_SAFE_INTEGER,
      pagePadding: COMPACT_PAGE_PADDING_VP,
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

  /**
   * 统一的窗口政策协调入口，供初始状态和事件回调共用。
   *
   * 将串行化逻辑集中在此方法中：
   * - 更新 cachedWindowStatus
   * - 递增 generation 并串到 windowStatusTask 链上确保串行执行
   * - FULL_SCREEN → applyTabletWindowPolicy（强制横屏 + 隐藏系统栏）
   * - UNDEFINED → 不执行政策，等待状态明确后再统一处理
   * - 其他 → restoreWindowPolicy（恢复默认方向与系统栏）
   */
  private reconcileWindowPolicy(windowStatus: window.WindowStatusType): void {
    if (!this.mainWindow || !this.cachedIsDedicatedLandscapeTablet) {
      return
    }
    // 更新缓存状态
    this.cachedWindowStatus = windowStatus

    this.windowStatusGeneration++
    const currentGeneration: number = this.windowStatusGeneration
    const mainWindow: window.Window = this.mainWindow

    this.windowStatusTask = this.windowStatusTask.then(() => {
      if (currentGeneration !== this.windowStatusGeneration) {
        hilog.info(DOMAIN, TAG, `WindowStatus task generation ${currentGeneration} is stale, skipping`)
        return
      }
      if (windowStatus === window.WindowStatusType.FULL_SCREEN) {
        return TabletWindowPolicy.applyTabletWindowPolicy(mainWindow)
      } else if (windowStatus === window.WindowStatusType.UNDEFINED) {
        // UNDEFINED：冷启动窗口状态还没就绪，不凭猜测强制横屏，等待状态明确后再统一处理
        hilog.info(DOMAIN, TAG, 'WindowStatus UNDEFINED: deferring policy until status is confirmed')
        return
      } else {
        return TabletWindowPolicy.restoreWindowPolicy(mainWindow)
      }
    }).catch((err: Error) => {
      hilog.error(DOMAIN, TAG, `WindowStatus task failed: ${JSON.stringify(err)}`)
    })
  }

  private handleWindowStatusChange(windowStatus: window.WindowStatusType): void {
    if (!this.mainWindow || !this.cachedIsDedicatedLandscapeTablet) {
      return
    }
    if (windowStatus === this.cachedWindowStatus) {
      return
    }
    hilog.info(DOMAIN, TAG, `WindowStatus changed: ${this.cachedWindowStatus} → ${windowStatus}`)
    this.reconcileWindowPolicy(windowStatus)
  }
}

// ========== 便捷函数 ==========

export function getAdaptiveContext(): AdaptiveContext {
  return AdaptiveContext.getInstance()
}

export function getAdaptiveState(): AdaptiveState {
  return AdaptiveContext.getInstance().getCurrentState()
}
