// ShoulderSurfingService — 防窥服务（单例）。
// 检测身后有人窥视时通知 UI（如降低正文对比度 / 显示提示）。
// 不调 Core，只暴露平台状态。
//
// 真实事件源经 DlpAntiPeepApi20 facade 调 @kit.DeviceSecurityKit 的 dlpAntiPeep。
// Service 不直接 import @kit.*（除 @kit.AbilityKit 的 common 用于 context 类型），
// 系统调用和枚举映射都在 impl/apiXX facade。
// supported / enabled / safe / peeping / unknown 全部来自系统，不默认 false 后等手工 notify。
// notifyPeeping 是 private 分发，仅由系统回调调用。

import { common } from '@kit.AbilityKit'
import { DlpAntiPeepApi20 } from './impl/api20/DlpAntiPeepApi20'
import { DlpAntiPeepApi23 } from './impl/api23/DlpAntiPeepApi23'

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
  private abilityContext: common.UIAbilityContext | null = null

  static getInstance(): ShoulderSurfingService {
    if (!ShoulderSurfingService.instance) {
      ShoulderSurfingService.instance = new ShoulderSurfingService()
    }
    return ShoulderSurfingService.instance
  }

  // 注入 UIAbilityContext（给 Api23 requestAntiPeepOptions 用）。
  // 由 AppContext.setAbilityContext 调用。
  setAbilityContext(context: common.UIAbilityContext): void {
    this.abilityContext = context
  }

  // 查询系统是否支持防窥检测。经 DlpAntiPeepApi20 facade 判断 SystemCapability。
  isSupported(): boolean {
    this.supported = DlpAntiPeepApi20.getInstance().isSupported()
    return this.supported
  }

  // 防窥开关是否启用。永远查系统真实开关，不返回本地缓存 this.enabled。
  // 不支持时返回 false，不伪造。
  async isEnabled(): Promise<boolean> {
    if (!this.isSupported()) {
      return false
    }
    return await DlpAntiPeepApi20.getInstance().isSwitchOn()
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

  // 请求打开防窥保护（打开系统设置页）。
  // API>=23 且有 context 时调 DlpAntiPeepApi23.requestAntiPeepOptions(context) 打开系统设置页；
  // 请求完成后重新调 DlpAntiPeepApi20.isSwitchOn() 确认真实开关（不只靠 AntiPeepOptionsResult 推断缓存），
  // 把真实开关写入 this.enabled 并返回。
  // 不支持/无 context 时返回当前真实开关状态，不伪造。
  // 是否能请求开启由 DlpAntiPeepApi23.isSupported() 判断（封装 API23+ 能力），Service 不认识 23 这个数字。
  async requestEnable(): Promise<boolean> {
    if (!this.isSupported()) {
      // 系统不支持时不假装已启用；返回真实开关（必然为 false）。
      this.enabled = false
      return false
    }
    const context: common.UIAbilityContext | null = this.abilityContext
    if (context === null || !DlpAntiPeepApi23.getInstance().isSupported()) {
      // 未注入 context 或 API23 防窥设置页能力不可用；不伪造，返回当前真实开关。
      this.enabled = await DlpAntiPeepApi20.getInstance().isSwitchOn()
      return this.enabled
    }
    try {
      // requestAntiPeepOptions 返回 dlpAntiPeep.AntiPeepOptionsResult（枚举）。
      // Service 不 import @kit.DeviceSecurityKit，用 number 接收。
      // 请求完成后必须重新调 isDlpAntiPeepSwitchOn() 确认真实开关，
      // 而不是只靠 AntiPeepOptionsResult 推断缓存。
      await DlpAntiPeepApi23.getInstance().requestAntiPeepOptions(context)
      this.enabled = await DlpAntiPeepApi20.getInstance().isSwitchOn()
      return this.enabled
    } catch (e) {
      // 系统调用失败；重新查真实开关，不伪造。
      this.enabled = await DlpAntiPeepApi20.getInstance().isSwitchOn()
      return this.enabled
    }
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
  // 官方推荐流程：先查 SystemCapability（isSupported），再查 isDlpAntiPeepSwitchOn()（开关是否打开），
  // 开关打开后再注册监听（facade.startListening，返回真实注册结果）并取初始状态（getPeepingStatus，三态）。
  // - 开关未打开时不伪装 resolved。
  // - facade.startListening 返回 false 时 listening 保持 false，不伪造已启动。
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
    const switchOn: boolean = await DlpAntiPeepApi20.getInstance().isSwitchOn()
    if (!switchOn) {
      return
    }
    // 注册监听；只有 facade.startListening 返回 true 才置 listening=true。
    const ok: boolean = DlpAntiPeepApi20.getInstance().startListening((peeping: boolean) => {
      this.notifyPeeping(peeping)
    })
    if (!ok) {
      // 注册失败；listening 保持 false，不伪造已启动。
      return
    }
    this.listening = true
    // 取初始状态（三态 boolean|null）。只有拿到真实 boolean（非 null）才置 resolved=true。
    const initial: boolean | null = DlpAntiPeepApi20.getInstance().getPeepingStatus()
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
    DlpAntiPeepApi20.getInstance().stopListening()
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
