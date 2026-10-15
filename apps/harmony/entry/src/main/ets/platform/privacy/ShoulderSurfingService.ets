// ShoulderSurfingService — 防窥服务（单例，稳定 facade）。
// 检测身后有人窥视时通知 UI（如降低正文对比度 / 显示提示）。
// 不调 Core，只暴露平台状态。
//
// Issue #914：DLP 受限实现从普通构建彻底解耦。
// Service 不再直接 import DlpAntiPeepApi20/23 或 @kit.DeviceSecurityKit，
// 所有 DLP 访问通过 ShoulderSurfingProvider（packageName import，target sourceRoots 提供）实现。
// - default target → standard Provider（isSupported 返回 false，不引用 DLP）
// - acl target → acl Provider（委托 privacy_acl HSP 的真实 DLP 实现）
// Service 保留 observer 机制和状态管理（peeping/resolved/listening）。
import { common } from '@kit.AbilityKit'
import { ShoulderSurfingPort } from './ShoulderSurfingPort'
import { ShoulderSurfingProvider } from 'entry/ets/platform/privacy/ShoulderSurfingProvider'

export interface ShoulderSurfingObserver {
  onShoulderSurfingChanged?(detected: boolean): void
}

export class ShoulderSurfingService {
  private static instance: ShoulderSurfingService
  private supported: boolean = false
  private enabled: boolean = false
  private peeping: boolean = false
  // 是否已解析过真实状态（监听启动或主动查询后置 true）。
  private resolved: boolean = false
  private observers: Set<ShoulderSurfingObserver> = new Set<ShoulderSurfingObserver>()
  private listening: boolean = false
  private provider: ShoulderSurfingPort = new ShoulderSurfingProvider()

  static getInstance(): ShoulderSurfingService {
    if (!ShoulderSurfingService.instance) {
      ShoulderSurfingService.instance = new ShoulderSurfingService()
    }
    return ShoulderSurfingService.instance
  }

  // 注入 UIAbilityContext（转传给 Provider，供 acl Provider 调 requestAntiPeepOptions 用）。
  // 由 AppContext.setAbilityContext 调用。
  setAbilityContext(context: common.UIAbilityContext): void {
    (this.provider as ShoulderSurfingProvider).setAbilityContext(context)
  }

  // 查询系统是否支持防窥检测。委托 Provider。
  isSupported(): boolean {
    this.supported = this.provider.isSupported()
    return this.supported
  }

  // 防窥开关是否启用。委托 Provider；不支持时返回 false，不伪造。
  async isEnabled(): Promise<boolean> {
    if (!this.isSupported()) {
      return false
    }
    return await this.provider.isEnabled()
  }

  // 当前是否安全。安全 = 已解析且无窥视。
  isSafe(): boolean {
    return !this.peeping && this.resolved
  }

  // 当前是否被窥视。
  isPeeping(): boolean {
    return this.peeping
  }

  // 是否未知（未解析或不支持）。
  isUnknown(): boolean {
    if (!this.supported) {
      return true
    }
    return !this.resolved
  }

  // 兼容旧接口：返回是否被窥视（同 isPeeping）。
  isShoulderSurfingDetected(): boolean {
    return this.peeping
  }

  // 请求打开防窥保护。委托 Provider；不支持时返回 false，不伪造。
  async requestEnable(): Promise<boolean> {
    if (!this.isSupported()) {
      this.enabled = false
      return false
    }
    this.enabled = await this.provider.requestEnable()
    return this.enabled
  }

  addObserver(observer: ShoulderSurfingObserver): void {
    this.observers.add(observer)
  }

  removeObserver(observer: ShoulderSurfingObserver): void {
    this.observers.delete(observer)
  }

  isListening(): boolean {
    return this.listening
  }

  // 启动系统防窥状态监听。
  // 官方推荐流程：先查 isSupported，再查 isEnabled（开关是否打开），
  // 开关打开后注册监听（provider.startListening）并取初始状态（getPeepingStatus，三态）。
  // - 开关未打开时不伪装 resolved。
  // - provider.startListening 返回 false 时 listening 保持 false，不伪造已启动。
  // - getPeepingStatus 返回 null（失败）时 resolved 保持 false（unknown），不伪装 safe。
  async startListening(): Promise<void> {
    if (this.listening) {
      return
    }
    if (!this.isSupported()) {
      // 系统不支持；不监听，peeping 保持 false，resolved 保持 false。
      return
    }
    // 查防窥开关是否打开。开关未打开时不监听、不伪装 resolved。
    const switchOn: boolean = await this.provider.isEnabled()
    if (!switchOn) {
      return
    }
    // 注册监听；只有 provider.startListening 返回 true 才置 listening=true。
    const ok: boolean = this.provider.startListening((peeping: boolean) => {
      this.notifyPeeping(peeping)
    })
    if (!ok) {
      // 注册失败；listening 保持 false，不伪造已启动。
      return
    }
    this.listening = true
    // 取初始状态（三态 boolean|null）。只有拿到真实 boolean（非 null）才置 resolved=true。
    const initial: boolean | null = this.provider.getPeepingStatus()
    if (initial !== null) {
      this.peeping = initial
      this.resolved = true
    }
    // initial === null 时 resolved 保持 false（unknown），不伪装 safe。
  }

  // 停止系统监听。
  stopListening(): void {
    if (!this.listening) {
      return
    }
    this.provider.stopListening()
    this.listening = false
  }

  // —— private ——
  // private 分发：仅由系统回调触发，外部不得直接调用。
  private notifyPeeping(peeping: boolean): void {
    this.peeping = peeping
    this.resolved = true
    this.observers.forEach(o => { if (o.onShoulderSurfingChanged) o.onShoulderSurfingChanged(peeping) })
  }
}
