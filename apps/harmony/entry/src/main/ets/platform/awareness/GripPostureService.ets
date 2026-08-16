// GripPostureService — 握持姿态感知（单例）。
// UI 订阅调整高频操作区（如单手握持时把工具栏移到握持侧）。
// 只暴露平台状态，不写业务规则，不调 Core。
//
// 真实事件源：@kit.MultimodalAwarenessKit 的 multimodalAwareness.on('gripPostureChange')。
// notifyPostureChanged 改成 private 分发方法，仅由系统回调调用。
// 不允许页面伪造平台状态。

import { multimodalAwareness, GripPostureData, GripPostureType } from '@kit.MultimodalAwarenessKit'

export type GripPosture = 'unknown' | 'notHeld' | 'left' | 'right' | 'both'

export interface GripPostureObserver {
  onGripPostureChanged?(posture: GripPosture): void
}

// HarmonyOS GripPostureType 常量（@kit.MultimodalAwarenessKit）。
// 0=unknown, 1=notHeld, 2=left, 3=right, 4=both。
const GRIP_POSTURE_NOT_HELD: number = 1
const GRIP_POSTURE_LEFT: number = 2
const GRIP_POSTURE_RIGHT: number = 3
const GRIP_POSTURE_BOTH: number = 4

export class GripPostureService {
  private static instance: GripPostureService
  private posture: GripPosture = 'unknown'
  private observers: Set<GripPostureObserver> = new Set<GripPostureObserver>()
  private listening: boolean = false
  private supported: boolean = false

  static getInstance(): GripPostureService {
    if (!GripPostureService.instance) {
      GripPostureService.instance = new GripPostureService()
    }
    return GripPostureService.instance
  }

  getPosture(): GripPosture {
    return this.posture
  }

  // 查询系统能力是否支持握姿感知。
  // 在 startListening() 调用后由系统回填；未启动时保守返回 false。
  isSupported(): boolean {
    return this.supported
  }

  addObserver(observer: GripPostureObserver): void {
    this.observers.add(observer)
  }

  removeObserver(observer: GripPostureObserver): void {
    this.observers.delete(observer)
  }

  isListening(): boolean {
    return this.listening
  }

  // 启动系统握姿感知监听。
  // 注册 multimodalAwareness.on('gripPostureChange')，系统事件映射成 unknown/notHeld/left/right/both。
  async startListening(): Promise<void> {
    if (this.listening) {
      return
    }
    try {
      multimodalAwareness.on('gripPostureChange', (data: GripPostureData) => {
        const posture: GripPosture = this.mapPosture(data.posture)
        this.notifyPostureChanged(posture)
      })
      this.listening = true
      this.supported = true
    } catch (e) {
      // 系统不支持握姿感知；不伪造状态。
      this.listening = false
      this.supported = false
    }
  }

  // 停止系统监听。
  stopListening(): void {
    if (!this.listening) {
      return
    }
    try {
      multimodalAwareness.off('gripPostureChange')
    } catch (e) {
      // off 失败不阻塞；标记为已停止即可。
    }
    this.listening = false
  }

  // —— private ——
  private mapPosture(raw: GripPostureType): GripPosture {
    const n: number = raw as number
    if (n === GRIP_POSTURE_NOT_HELD) {
      return 'notHeld'
    }
    if (n === GRIP_POSTURE_LEFT) {
      return 'left'
    }
    if (n === GRIP_POSTURE_RIGHT) {
      return 'right'
    }
    if (n === GRIP_POSTURE_BOTH) {
      return 'both'
    }
    // 0/unknown/未知都归 unknown。
    return 'unknown'
  }

  // private 分发：仅由系统回调触发，外部不得直接调用。
  private notifyPostureChanged(posture: GripPosture): void {
    this.posture = posture
    this.observers.forEach(o => { if (o.onGripPostureChanged) o.onGripPostureChanged(posture) })
  }
}
