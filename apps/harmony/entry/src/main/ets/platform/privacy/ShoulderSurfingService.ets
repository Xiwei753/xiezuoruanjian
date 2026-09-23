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
import { PlatformApiResolver } from '../version/PlatformApiResolver'

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

  // 防窥开关是否启用。异步查询系统并缓存到 this.enabled。
  async isEnabled(): Promise<boolean> {
    if (!this.isSupported()) {
      this.enabled = false
      return false
    }
    this.enabled = await DlpAntiPeepApi20.getInstance().isSwitchOn()
    return this.enabled
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

  // 打开/关闭防窥保护。
  // API>=23 且有 context 时，调 DlpAntiPeepApi23.requestAntiPeepOptions(context) 打开系统设置页；
  // 不支持时不伪造 enabled 状态。
  async setEnabled(enabled: boolean): Promise<void> {
    if (!enabled) {
      // 关闭：无系统 disable API，仅清本地缓存。
      this.enabled = false
      return
    }
    if (!this.isSupported()) {
      // 系统不支持时不假装已启用。
      this.enabled = false
      return
    }
    const context: common.UIAbilityContext | null = this.abilityContext
    if (context === null) {
      // 未注入 context；不伪造。
      return
    }
    if (!PlatformApiResolver.getInstance().isApiAtLeast(23)) {
      // API<23 无设置页入口；不伪造。
      return
    }
    try {
      // requestAntiPeepOptions 返回 dlpAntiPeep.AntiPeepOptionsResult（枚举）。
      // Service 不 import @kit.DeviceSecurityKit，用 number 接收并比较数值。
      // SUCCESS=0 或 ALREADY_ON=2 视为已启用；FAIL=1 视为未启用。
      const result: number = await DlpAntiPeepApi23.getInstance().requestAntiPeepOptions(context) as number
      this.enabled = result !== 1
    } catch (e) {
      // 系统调用失败；保持当前状态不伪造。
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
  // 经 DlpAntiPeepApi20 facade 注册 dlpAntiPeep.on('dlpAntiPeep')，系统事件回填 peeping。
  async startListening(): Promise<void> {
    if (this.listening) {
      return
    }
    if (!this.isSupported()) {
      // 系统不支持；不监听，peeping 保持 false。
      return
    }
    DlpAntiPeepApi20.getInstance().startListening((peeping: boolean) => {
      this.notifyPeeping(peeping)
    })
    this.listening = true
    // 首次监听时主动查询一次当前状态，标记已解析。
    this.peeping = DlpAntiPeepApi20.getInstance().getPeepingStatus()
    this.resolved = true
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
