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
      const displayInfo = display.getDefaultDisplaySync()
      const density = displayInfo.densityPixels
      const widthVp = rect.width / density
      const heightVp = rect.height / density
      await this.updateState(widthVp, heightVp)

      // 注册窗口尺寸变化监听
      this.registerWindowSizeChange()
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
      safeTopVp: 0,  // TODO: 从平台获取
      safeBottomVp: 0,  // TODO: 从平台获取
      keyboardVisible: false,  // TODO: 从平台获取
      foldPosture: FoldPosture.Unknown,  // TODO: 从平台获取
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
}

// ========== 便捷函数 ==========

export function getAdaptiveContext(): AdaptiveContext {
  return AdaptiveContext.getInstance()
}

export function getAdaptiveState(): AdaptiveState {
  return AdaptiveContext.getInstance().getCurrentState()
}
