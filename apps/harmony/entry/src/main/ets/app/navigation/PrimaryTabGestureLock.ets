// PrimaryTabGestureLock — 按住主操作悬浮按钮时，锁掉一级 Tabs 的横向滑动切页。
//
// 起因：HdsTabs 的横向滑动切页走的是 HDS 组件内部的手势管线，FAB 侧拦不住。实机试过三种
// 组件级方案都无效（都不是猜的，是逐个装包复测的）：
//   1. FAB 上绑 .gesture(PanGesture())      —— 父容器滑动优先，手指在按钮上照样翻页
//   2. FAB 上绑 .priorityGesture(PanGesture()) —— 同样无效
//   3. FAB 上绑 .gesture(PanGesture(), GestureMask.IgnoreInternal) —— 官方语义就是
//      「禁掉父组件内建手势」，但 HdsTabs 不受理这些通用属性
// 所以只能从 Tabs 自己下手：按住期间把 .scrollable 关掉。
//
// 为什么不复用 currentPrimaryNavInteractive：那个状态由 Core ScreenPolicy 驱动，语义是
// 「当前角色是否允许离开」（Writing/Settings 为 false），并且和底栏显隐解耦是 Issue #774 的
// 明确要求。这里是纯手势锁，语义完全不同，所以单独一个 store，两者取与后传给 Tabs，
// 互不覆盖。
//
// 生命周期：FAB onTouch 的 Down 时加锁，Up / Cancel 时解锁。Taps 未注册的观察者不会收到通知。

type GestureLockObserver = (locked: boolean) => void

export class PrimaryTabGestureLock {
  private static instance: PrimaryTabGestureLock
  private locked: boolean = false
  private observers: GestureLockObserver[] = []

  private constructor() {}

  static getInstance(): PrimaryTabGestureLock {
    if (!PrimaryTabGestureLock.instance) {
      PrimaryTabGestureLock.instance = new PrimaryTabGestureLock()
    }
    return PrimaryTabGestureLock.instance
  }

  // FAB 按下时传 true，抬起/取消时传 false。重复同值直接返回，避免多余的响应式刷新。
  setLocked(locked: boolean): void {
    if (this.locked === locked) {
      return
    }
    this.locked = locked
    for (const observer of this.observers) {
      observer(locked)
    }
  }

  isLocked(): boolean {
    return this.locked
  }

  // PrimaryTabShell 订阅；订阅时立即回调一次当前值，保证初始状态一致。
  subscribe(observer: GestureLockObserver): void {
    this.observers.push(observer)
    observer(this.locked)
  }

  unsubscribe(observer: GestureLockObserver): void {
    const index = this.observers.indexOf(observer)
    if (index >= 0) {
      this.observers.splice(index, 1)
    }
  }
}
