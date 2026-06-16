import window from '@ohos.window'
import hilog from '@ohos.hilog'

/**
 * 窗口尺寸断点枚举
 * Compact  (< 520vp) — 手机/折叠外屏
 * Medium   (520–840vp) — 折叠展开小屏/竖屏平板
 * Expanded (>= 840vp) — 平板/折叠内屏横向
 */
export enum WindowSizeClass {
  Compact,
  Medium,
  Expanded
}

/**
 * 响应式布局状态
 */
export interface AdaptiveState {
  widthVp: number
  heightVp: number
  sizeClass: WindowSizeClass
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

const TAG = 'AdaptiveContext'
const DOMAIN = 0x0000

// 断点阈值
const COMPACT_MAX_VP = 520
const MEDIUM_MAX_VP = 840

// 各断点对应的布局参数
const CONTENT_MAX_WIDTH: Record<number, number> = {
  [WindowSizeClass.Compact]: Number.MAX_SAFE_INTEGER,
  [WindowSizeClass.Medium]: 720,
  [WindowSizeClass.Expanded]: 840
}

const PAGE_PADDING: Record<number, number> = {
  [WindowSizeClass.Compact]: 16,
  [WindowSizeClass.Medium]: 24,
  [WindowSizeClass.Expanded]: 32
}

const GRID_COLUMNS: Record<number, number> = {
  [WindowSizeClass.Compact]: 2,
  [WindowSizeClass.Medium]: 3,
  [WindowSizeClass.Expanded]: 4
}

/**
 * 响应式断点上下文（单例）
 *
 * 监听窗口尺寸变化，计算断点状态，通知所有订阅者。
 * 在 EntryAbility.onWindowStageCreate 中初始化。
 */
export class AdaptiveContext {
  private static instance: AdaptiveContext
  private state: AdaptiveState = AdaptiveContext.createDefaultState()
  private observers: Set<AdaptiveObserver> = new Set()
  private mainWindow: window.Window | null = null
  private isListening: boolean = false

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

      // 读取初始尺寸
      const properties = this.mainWindow.getWindowProperties()
      const rect = properties.windowRect
      const density = properties.densityPixels
      const widthVp = rect.width / density
      const heightVp = rect.height / density
      this.updateState(widthVp, heightVp)

      // 注册窗口尺寸变化监听
      this.registerWindowSizeChange()
      hilog.info(DOMAIN, TAG, `Initialized: ${WindowSizeClass[this.state.sizeClass]} ${this.state.widthVp}x${this.state.heightVp}`)
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
    const observer: AdaptiveObserver = { onWindowSizeChange: callback }
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
    this.observers.clear()
    this.mainWindow = null
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
        const density = properties.densityPixels
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

  private updateState(widthVp: number, heightVp: number): void {
    const sizeClass = AdaptiveContext.computeSizeClass(widthVp)
    const isTwoPane = sizeClass === WindowSizeClass.Expanded

    const oldSizeClass = this.state.sizeClass

    this.state = {
      widthVp: Math.round(widthVp * 100) / 100,
      heightVp: Math.round(heightVp * 100) / 100,
      sizeClass: sizeClass,
      isTwoPane: isTwoPane,
      contentMaxWidth: CONTENT_MAX_WIDTH[sizeClass],
      pagePadding: PAGE_PADDING[sizeClass],
      gridColumns: GRID_COLUMNS[sizeClass]
    }

    // 断点变化时打印日志
    if (oldSizeClass !== sizeClass) {
      hilog.info(DOMAIN, TAG, `Size class changed: ${WindowSizeClass[oldSizeClass]} → ${WindowSizeClass[sizeClass]}`)
    }

    this.notifyObservers()
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

  private static computeSizeClass(widthVp: number): WindowSizeClass {
    if (widthVp < COMPACT_MAX_VP) {
      return WindowSizeClass.Compact
    } else if (widthVp < MEDIUM_MAX_VP) {
      return WindowSizeClass.Medium
    } else {
      return WindowSizeClass.Expanded
    }
  }

  private static createDefaultState(): AdaptiveState {
    return {
      widthVp: 0,
      heightVp: 0,
      sizeClass: WindowSizeClass.Compact,
      isTwoPane: false,
      contentMaxWidth: CONTENT_MAX_WIDTH[WindowSizeClass.Compact],
      pagePadding: PAGE_PADDING[WindowSizeClass.Compact],
      gridColumns: GRID_COLUMNS[WindowSizeClass.Compact]
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