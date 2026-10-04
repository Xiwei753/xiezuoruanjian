// HarmonyOS NEXT Lifecycle — 包装 UIAbility 生命周期回调（API 12）。
// EntryAbility.ets 的 onForeground/onBackground/onWindowStageDestroy 调用 notifyXxx()。
// 对齐 UIAbility 的 onCreate / onDestroy / onForeground / onBackground。

export type LifecycleEvent = 'create' | 'destroy' | 'foreground' | 'background' | 'focus' | 'blur'

export interface LifecycleObserver {
  onCreate?(): void
  onDestroy?(): void
  onForeground?(): void
  onBackground?(): void
  onFocus?(): void
  onBlur?(): void
}

export interface LifecycleState {
  isForeground: boolean
  isFocused: boolean
  createdAt: number
  lastForegroundAt: number
  lastBackgroundAt: number
}

export class HarmonyLifecycle {
  private static instance: HarmonyLifecycle
  private observers: Set<LifecycleObserver> = new Set()
  private state: LifecycleState = {
    isForeground: true,
    isFocused: true,
    createdAt: Date.now(),
    lastForegroundAt: Date.now(),
    lastBackgroundAt: 0
  }
  private listeners: Map<LifecycleEvent, Set<() => void>> = new Map()

  static getInstance(): HarmonyLifecycle {
    if (!HarmonyLifecycle.instance) {
      HarmonyLifecycle.instance = new HarmonyLifecycle()
    }
    return HarmonyLifecycle.instance
  }

  addObserver(observer: LifecycleObserver): void {
    this.observers.add(observer)
  }

  removeObserver(observer: LifecycleObserver): void {
    this.observers.delete(observer)
  }

  on(event: LifecycleEvent, callback: () => void): void {
    if (!this.listeners.has(event)) {
      this.listeners.set(event, new Set())
    }
    this.listeners.get(event)!.add(callback)
  }

  off(event: LifecycleEvent, callback: () => void): void {
    this.listeners.get(event)?.delete(callback)
  }

  getState(): LifecycleState {
    const s: LifecycleState = {
      isForeground: this.state.isForeground,
      isFocused: this.state.isFocused,
      createdAt: this.state.createdAt,
      lastForegroundAt: this.state.lastForegroundAt,
      lastBackgroundAt: this.state.lastBackgroundAt
    }
    return s
  }

  isForeground(): boolean {
    return this.state.isForeground
  }

  isFocused(): boolean {
    return this.state.isFocused
  }

  notifyCreate(): void {
    this.state.createdAt = Date.now()
    this.observers.forEach(obs => obs.onCreate?.())
    this.emit('create')
  }

  notifyDestroy(): void {
    this.observers.forEach(obs => obs.onDestroy?.())
    this.emit('destroy')
  }

  notifyForeground(): void {
    this.state.isForeground = true
    this.state.lastForegroundAt = Date.now()
    this.observers.forEach(obs => obs.onForeground?.())
    this.emit('foreground')
  }

  notifyBackground(): void {
    this.state.isForeground = false
    this.state.lastBackgroundAt = Date.now()
    this.observers.forEach(obs => obs.onBackground?.())
    this.emit('background')
  }

  notifyFocus(): void {
    this.state.isFocused = true
    this.observers.forEach(obs => obs.onFocus?.())
    this.emit('focus')
  }

  notifyBlur(): void {
    this.state.isFocused = false
    this.observers.forEach(obs => obs.onBlur?.())
    this.emit('blur')
  }

  private emit(event: LifecycleEvent): void {
    this.listeners.get(event)?.forEach(cb => cb())
  }
}

// Issue #825 复核10：定时自动保存的开关与间隔现在能被就地重配。
//
// 之前这里只有一个 intervalMs，且 onForeground 无条件 startAutoSave()：
// 页面侧只能靠「removeObserver + new 一个」来改延迟，而 removeObserver 只是
// 从 Set 里删引用，不会调旧 observer 的 stopAutoSave()——旧定时器继续按旧间隔
// 跑，新 observer 又因为 addObserver 不会补发 onForeground 而当场不启动，
// 下一次前后台切换后新旧 interval 还有机会叠在一起。
//
// 所以这里改成可重配对象：enabled 决定定时保存开不开，reconfigure() 先停再按
// 新参数决定要不要立刻重启。退后台那次安全保存与这个开关是两件事，仍然无条件执行，
// 避免把「定时自动保存开关」和「退后台保护正文」混在一起。
export class AutoSaveLifecycleObserver implements LifecycleObserver {
  private saveCallback: () => Promise<void>
  private intervalId: number = -1
  private intervalMs: number
  private enabled: boolean

  constructor(saveCallback: () => Promise<void>, intervalMs: number = 30000, enabled: boolean = true) {
    this.saveCallback = saveCallback
    this.intervalMs = intervalMs
    this.enabled = enabled
  }

  /**
   * 按最新设置重配定时自动保存。
   * @param enabled 设置里的自动保存开关
   * @param intervalMs 最新的自动保存间隔
   * @param isForeground 调用方当前的真实前后台状态（observer 自己收不到「已在前台」）
   */
  reconfigure(enabled: boolean, intervalMs: number, isForeground: boolean): void {
    this.stopAutoSave()
    this.enabled = enabled
    this.intervalMs = intervalMs
    if (enabled && isForeground) {
      this.startAutoSave()
    }
  }

  onForeground(): void {
    if (!this.enabled) {
      return
    }
    this.startAutoSave()
  }

  onBackground(): void {
    this.stopAutoSave()
    this.saveCallback()
  }

  onDestroy(): void {
    this.stopAutoSave()
  }

  startAutoSave(): void {
    this.stopAutoSave()
    this.intervalId = setInterval(() => {
      this.saveCallback()
    }, this.intervalMs)
  }

  stopAutoSave(): void {
    if (this.intervalId !== -1) {
      clearInterval(this.intervalId)
      this.intervalId = -1
    }
  }
}

export class SyncLifecycleObserver implements LifecycleObserver {
  private syncCallback: () => Promise<void>
  private isSyncOnForeground: boolean

  constructor(syncCallback: () => Promise<void>, syncOnForeground: boolean = true) {
    this.syncCallback = syncCallback
    this.isSyncOnForeground = syncOnForeground
  }

  async onForeground(): Promise<void> {
    if (this.isSyncOnForeground) {
      await this.syncCallback()
    }
  }
}

export class ShareLifecycleObserver implements LifecycleObserver {
  private foregroundCallback: () => void
  private backgroundCallback: () => void

  constructor(foregroundCallback: () => void, backgroundCallback: () => void) {
    this.foregroundCallback = foregroundCallback
    this.backgroundCallback = backgroundCallback
  }

  onForeground(): void {
    this.foregroundCallback()
  }

  onBackground(): void {
    this.backgroundCallback()
  }
}
