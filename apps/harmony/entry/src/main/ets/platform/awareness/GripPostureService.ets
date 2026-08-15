// GripPostureService — 握持姿态感知（单例）。
// UI 订阅调整高频操作区（如单手握持时把工具栏移到握持侧）。
// 只暴露平台状态，不写业务规则，不调 Core。

export type GripPosture = 'unknown' | 'notHeld' | 'left' | 'right' | 'both'

export interface GripPostureObserver {
  onGripPostureChanged?(posture: GripPosture): void
}

export class GripPostureService {
  private static instance: GripPostureService
  private posture: GripPosture = 'unknown'
  private observers: Set<GripPostureObserver> = new Set<GripPostureObserver>()

  static getInstance(): GripPostureService {
    if (!GripPostureService.instance) {
      GripPostureService.instance = new GripPostureService()
    }
    return GripPostureService.instance
  }

  getPosture(): GripPosture {
    return this.posture
  }

  addObserver(observer: GripPostureObserver): void {
    this.observers.add(observer)
  }

  removeObserver(observer: GripPostureObserver): void {
    this.observers.delete(observer)
  }

  // 平台层在收到握持姿态事件时调用。
  notifyPostureChanged(posture: GripPosture): void {
    this.posture = posture
    this.observers.forEach(o => { if (o.onGripPostureChanged) o.onGripPostureChanged(posture) })
  }
}
