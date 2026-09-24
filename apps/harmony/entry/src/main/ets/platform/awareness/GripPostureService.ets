// GripPostureService — 握持姿态感知（单例）。
// UI 订阅调整高频操作区（如单手握持时把工具栏移到握持侧）。
// 只暴露平台状态，不写业务规则，不调 Core。
//
// 真实事件源经 GripPostureApi20 facade 调 @kit.MultimodalAwarenessKit 的 motion。
// Service 不直接 import 版本敏感的高版本能力 Kit（@kit.AbilityKit 的 common 等基础平台类型除外），系统调用和枚举映射都在 impl/api20 facade。
// notifyPostureChanged 是 private 分发方法，仅由系统回调触发。
// 不允许页面伪造平台状态。

import { GripPosture, GripPostureObserver } from './GripPostureTypes'
import { GripPostureApi20 } from './impl/api20/GripPostureApi20'

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
  // 未监听时查询 facade 真实能力；监听后返回缓存。
  isSupported(): boolean {
    if (!this.listening) {
      this.supported = GripPostureApi20.getInstance().isSupported()
    }
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
  // 经 GripPostureApi20 facade 注册 motion.on('holdingHandChanged')，
  // 系统事件映射成 unknown/notHeld/left/right/both。
  // 只有 facade.startListening() 返回 true 才置 listening=true；
  // 不因 canIUse/isSupported 为 true 就把"注册成功"当成既定事实。
  async startListening(): Promise<void> {
    if (this.listening) {
      return
    }
    const api: GripPostureApi20 = GripPostureApi20.getInstance()
    this.supported = api.isSupported()
    if (!this.supported) {
      // 系统不支持；不监听，不伪造状态。
      return
    }
    // 以 startListening() 的真实返回值为准，不因 isSupported 就伪造已启动。
    const ok: boolean = api.startListening((posture: GripPosture) => {
      this.notifyPostureChanged(posture)
    })
    if (!ok) {
      // 注册失败；listening 保持 false，不伪造已启动。
      return
    }
    this.listening = true
  }

  // 停止系统监听。
  stopListening(): void {
    if (!this.listening) {
      return
    }
    GripPostureApi20.getInstance().stopListening()
    this.listening = false
  }

  // —— private ——
  // private 分发：仅由系统回调触发，外部不得直接调用。
  private notifyPostureChanged(posture: GripPosture): void {
    this.posture = posture
    this.observers.forEach(o => { if (o.onGripPostureChanged) o.onGripPostureChanged(posture) })
  }
}
