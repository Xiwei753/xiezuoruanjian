import window from '@ohos.window'
import display from '@ohos.display'
import hilog from '@ohos.hilog'
import { getBridge } from './AppContext'
import {
  WindowMetrics, LayoutPlan, FoldPosture, Orientation, PointerKind,
  WidthClass, HeightClass, ShellMode, EditorMode, NavigationMode
} from '../model/CoreDtos'

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

      // 读取初始安全区域 / 键盘 / 折叠状态
      this.readSafeAreas()
      this.readKeyboardVisible()
      this.readFoldPosture()

      // 读取初始尺寸
      const properties = this.mainWindow.getWindowProperties()
      const rect = properties.windowRect
      const displayInfo = display.getDefaultDisplaySync()
      const density = displayInfo.densityPixels
      const widthVp = rect.width / density
      const heightVp = rect.height / density
      await this.updateState(widthVp, heightVp)

      // 注册窗口尺寸变化监听
      this.registerWindowSizeChange()
      // 注册避让区域变化监听（安全区域 + 键盘）
      this.registerAvoidAreaChange()
      // 注册折叠状态变化监听
      this.registerFoldStatusChange()
      hilog.info(DOMAIN, TAG, `Initialized: ${this.state.sizeClass} ${this.state.widthVp}x${this.state.heightVp}`)
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
      gridColumns: this.state.gridColumns
    }
  }

  /**
   * 订阅窗口尺寸变化
   */
  addObserver(observer: AdaptiveObserver): void {
    this.observers.add(observer)
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
    this.observers.add(observer)
    // 立即通知当前状态
    callback(this.getCurrentState())
    // 返回取消订阅函数
    return () => {
      this.observers.delete(observer)
    }
  }

  /**
   * 停止监听并清理资源
   */
  dispose(): void {
    this.unregisterWindowSizeChange()
    this.unregisterAvoidAreaChange()
    this.unregisterFoldStatusChange()
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
        const properties = this.mainWindow.getWindowProperties()
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
    const bridge = getBridge()
    const metrics: WindowMetrics = {
      widthVp: Math.round(widthVp * 100) / 100,
      heightVp: Math.round(heightVp * 100) / 100,
      safeTopVp: this.cachedSafeTopVp,
      safeBottomVp: this.cachedSafeBottomVp,
      keyboardVisible: this.cachedKeyboardVisible,
      foldPosture: this.cachedFoldPosture,
      orientation: widthVp > heightVp ? Orientation.Landscape : Orientation.Portrait,
      pointer: PointerKind.Touch
    }

    try {
      const result = await bridge.resolveLayout(metrics)
      if (result.success && result.data) {
        const plan = result.data
        this.state = {
          widthVp: metrics.widthVp,
          heightVp: metrics.heightVp,
          layoutPlan: plan,
          sizeClass: plan.widthClass,
          isTwoPane: plan.shellMode === ShellMode.TwoPane,
          contentMaxWidth: plan.contentMaxWidthVp === 0 ? Number.MAX_SAFE_INTEGER : plan.contentMaxWidthVp,
          pagePadding: plan.pagePaddingVp,
          gridColumns: plan.gridColumns
        }
        this.notifyObservers()
      }
    } catch (err) {
      hilog.error(DOMAIN, TAG, `resolveLayout failed: ${JSON.stringify(err)}`)
    }
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
      widthClass: WidthClass.Compact,
      heightClass: HeightClass.Compact,
      shellMode: ShellMode.SinglePane,
      editorMode: EditorMode.FullWidth,
      navigationMode: NavigationMode.Stack,
      contentMaxWidthVp: 0,
      pagePaddingVp: 16,
      gridColumns: 2,
      showSidePanel: false,
      showBottomBar: true
    }
    return {
      widthVp: 0,
      heightVp: 0,
      layoutPlan: defaultPlan,
      sizeClass: WidthClass.Compact,
      isTwoPane: false,
      contentMaxWidth: Number.MAX_SAFE_INTEGER,
      pagePadding: 16,
      gridColumns: 2
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
   */
  private readFoldPosture(): void {
    try {
      const foldStatus = display.getDisplayFoldStatus()
      if (foldStatus === display.FoldStatus.FOLD_STATUS_EXPANDED) {
        this.cachedFoldPosture = FoldPosture.FullyOpened
      } else if (foldStatus === display.FoldStatus.FOLD_STATUS_HALF_FOLDED) {
        this.cachedFoldPosture = FoldPosture.HalfOpened
      } else if (foldStatus === display.FoldStatus.FOLD_STATUS_FOLDED) {
        this.cachedFoldPosture = FoldPosture.Closed
      } else {
        this.cachedFoldPosture = FoldPosture.Unknown
      }
      hilog.info(DOMAIN, TAG, `FoldPosture: ${this.cachedFoldPosture}`)
    } catch (err) {
      // 非折叠设备或不支持该 API，保持 Unknown
      this.cachedFoldPosture = FoldPosture.Unknown
      hilog.info(DOMAIN, TAG, `FoldPosture: Unknown (unsupported or error)`)
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
          this.updateState(widthVp, heightVp)
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
      display.on('foldStatusChange', (foldStatus: display.FoldStatus) => {
        let newPosture: FoldPosture
        if (foldStatus === display.FoldStatus.FOLD_STATUS_EXPANDED) {
          newPosture = FoldPosture.FullyOpened
        } else if (foldStatus === display.FoldStatus.FOLD_STATUS_HALF_FOLDED) {
          newPosture = FoldPosture.HalfOpened
        } else if (foldStatus === display.FoldStatus.FOLD_STATUS_FOLDED) {
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
            this.updateState(widthVp, heightVp)
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
}

// ========== 便捷函数 ==========

export function getAdaptiveContext(): AdaptiveContext {
  return AdaptiveContext.getInstance()
}

export function getAdaptiveState(): AdaptiveState {
  return AdaptiveContext.getInstance().getCurrentState()
}
