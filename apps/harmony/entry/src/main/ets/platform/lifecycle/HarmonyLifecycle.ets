// HarmonyOS NEXT Lifecycle — 包装 UIAbility 生命周期回调（API 12）。
// EntryAbility.ets 的 onForeground/onBackground/onWindowStageDestroy 调用 notifyXxx()。
// 对齐 UIAbility 的 onCreate / onDestroy / onForeground / onBackground。

import { HarmonyBackgroundSaveTask } from './HarmonyBackgroundSaveTask'
import { HarmonyDiagnosticEvents, DiagnosticField } from '../diagnostics/HarmonyDiagnosticEvents'

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
//
// Issue #927：退后台保存不再直接调用 saveCallback 丢弃 Promise，而是交给平台
// HarmonyBackgroundSaveTask.runSave 申请短时任务托管：申请到 requestId 后才开始
// 异步落盘，完成/失败后释放。单一 in-flight 标志保证 onWindowStageDestroy +
// onBackground 连续触发只产生一笔后台保存事务。前台 setInterval 定时保存语义不变。
export class AutoSaveLifecycleObserver implements LifecycleObserver {
  private saveCallback: () => Promise<boolean>
  private intervalId: number = -1
  private intervalMs: number
  private enabled: boolean
  // Issue #927：后台保存成功/失败结果回调（页面用于日志/提示；dirty 状态由协调器回流保证）。
  private onSaveResult: ((success: boolean) => void) | null = null
  // Issue #927：单一 in-flight 标志，避免双生命周期事件生成两笔后台保存。
  private backgroundSaveInFlight: boolean = false

  constructor(
    saveCallback: () => Promise<boolean>,
    intervalMs: number = 30000,
    enabled: boolean = true,
    onSaveResult?: (success: boolean) => void
  ) {
    this.saveCallback = saveCallback
    this.intervalMs = intervalMs
    this.enabled = enabled
    this.onSaveResult = onSaveResult !== undefined ? onSaveResult : null
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

  // Issue #927：退后台保存统一走平台短时任务（runSave），不再直接丢弃 saveCallback 的 Promise。
  onBackground(): void {
    this.stopAutoSave()
    this.runBackgroundSave()
  }

  onDestroy(): void {
    this.stopAutoSave()
  }

  startAutoSave(): void {
    this.stopAutoSave()
    this.intervalId = setInterval(() => {
      // 前台定时保存的结果由协调器经显示状态回流；这里只兜住异常，避免 unhandled rejection。
      this.saveCallback().catch(() => {})
    }, this.intervalMs)
  }

  stopAutoSave(): void {
    if (this.intervalId !== -1) {
      clearInterval(this.intervalId)
      this.intervalId = -1
    }
  }

  // Issue #927：一次后台保存事务。申请短时任务前不开始异步落盘；in-flight 期间忽略重复事件。
  private runBackgroundSave(): void {
    if (this.backgroundSaveInFlight) {
      return
    }
    this.backgroundSaveInFlight = true
    const task: HarmonyBackgroundSaveTask = HarmonyBackgroundSaveTask.getInstance()
    if (!task.isSupported()) {
      // 平台不支持短时任务：记录诊断并返回真实失败，不假装保存已完成。
      this.backgroundSaveInFlight = false
      const reasonField: DiagnosticField = { key: 'reason', value: 'syscap_missing' }
      HarmonyDiagnosticEvents.record('Warn', 'System', 'background_save_unavailable', 'AutoSaveLifecycleObserver', [reasonField])
      this.notifySaveResult(false)
      return
    }
    task.runSave(this.saveCallback).then((success: boolean) => {
      this.backgroundSaveInFlight = false
      if (!success) {
        const stageField: DiagnosticField = { key: 'stage', value: 'save' }
        HarmonyDiagnosticEvents.record('Error', 'App', 'background_save_failed', 'AutoSaveLifecycleObserver', [stageField])
      }
      this.notifySaveResult(success)
    }, () => {
      // runSave 按契约不抛异常；兜底路径同样按失败上报，不静默吞掉。
      this.backgroundSaveInFlight = false
      const stageField: DiagnosticField = { key: 'stage', value: 'exception' }
      HarmonyDiagnosticEvents.record('Error', 'App', 'background_save_failed', 'AutoSaveLifecycleObserver', [stageField])
      this.notifySaveResult(false)
    })
  }

  private notifySaveResult(success: boolean): void {
    const callback = this.onSaveResult
    if (callback !== null) {
      callback(success)
    }
  }
}

// Issue #925：同步调度器的生命周期适配器。此前只有定义、生产代码无人注册；
// 现在由 HarmonyAutoSyncScheduler 在 Ability 级装配时真实注册，把前台/后台事件
// 转发给调度器（前台立即做一次到期判断，后台停止前台唤醒）。
export class SyncLifecycleObserver implements LifecycleObserver {
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
