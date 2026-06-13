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
    return { ...this.state }
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

export class AutoSaveLifecycleObserver implements LifecycleObserver {
  private saveCallback: () => Promise<void>
  private intervalId: number = -1
  private intervalMs: number

  constructor(saveCallback: () => Promise<void>, intervalMs: number = 30000) {
    this.saveCallback = saveCallback
    this.intervalMs = intervalMs
  }

  onForeground(): void {
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
