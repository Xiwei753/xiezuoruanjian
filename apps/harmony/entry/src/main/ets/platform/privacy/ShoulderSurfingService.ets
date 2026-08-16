// ShoulderSurfingService — 防窥（Device Security Kit）服务。
// 检测身后有人窥视时通知 UI（如降低正文对比度 / 显示提示）。
// 不调 Core，只暴露平台状态。
//
// 真实事件源：@kit.DeviceSecurityKit 的 screenShieldManager。
// supported / enabled / detected 全部来自系统，不默认 false 后等手工 notify。
// notifyDetected 改成 private 分发，仅由系统回调调用。

import { screenShieldManager, ScreenPeepingStatus } from '@kit.DeviceSecurityKit'

export interface ShoulderSurfingObserver {
  onShoulderSurfingChanged?(detected: boolean): void
}

export class ShoulderSurfingService {
  private static instance: ShoulderSurfingService
  private supported: boolean = false
  private enabled: boolean = false
  private detected: boolean = false
  private observers: Set<ShoulderSurfingObserver> = new Set<ShoulderSurfingObserver>()
  private listening: boolean = false

  static getInstance(): ShoulderSurfingService {
    if (!ShoulderSurfingService.instance) {
      ShoulderSurfingService.instance = new ShoulderSurfingService()
    }
    return ShoulderSurfingService.instance
  }

  // 查询系统是否支持防窥检测。调用系统 API screenShieldManager.isSupported()。
  isSupported(): boolean {
    try {
      this.supported = screenShieldManager.isSupported()
    } catch (e) {
      // 系统不支持或 API 缺失；保守 false。
      this.supported = false
    }
    return this.supported
  }

  isEnabled(): boolean {
    return this.enabled
  }

  isShoulderSurfingDetected(): boolean {
    return this.detected
  }

  // 调用系统 API enable/disable，真实切换防窥保护。
  async setEnabled(enabled: boolean): Promise<void> {
    if (!this.isSupported()) {
      // 系统不支持时不假装已启用。
      this.enabled = false
      return
    }
    try {
      if (enabled) {
        await screenShieldManager.enable()
        this.enabled = true
      } else {
        await screenShieldManager.disable()
        this.enabled = false
      }
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
  // 注册 screenShieldManager.on('screenPeepingStatusChange')，系统事件回填 detected。
  async startListening(): Promise<void> {
    if (this.listening) {
      return
    }
    if (!this.isSupported()) {
      // 系统不支持；不监听，detected 保持 false。
      return
    }
    try {
      screenShieldManager.on('screenPeepingStatusChange', (status: ScreenPeepingStatus) => {
        // ScreenPeepingStatus 表示是否被窥视；映射成 boolean。
        this.notifyDetected(this.mapPeepingStatus(status))
      })
      this.listening = true
    } catch (e) {
      // 注册失败；不伪造状态。
      this.listening = false
    }
  }

  // 停止系统监听。
  stopListening(): void {
    if (!this.listening) {
      return
    }
    try {
      screenShieldManager.off('screenPeepingStatusChange')
    } catch (e) {
      // off 失败不阻塞。
    }
    this.listening = false
  }

  // —— private ——
  private mapPeepingStatus(status: ScreenPeepingStatus): boolean {
    // ScreenPeepingStatus 取值：true/非零表示被窥视，false/0 表示安全。
    // 不同版本可能是 boolean 或 number；统一转 boolean。
    if (typeof status === 'boolean') {
      return status as boolean
    }
    return (status as number) !== 0
  }

  // private 分发：仅由系统回调触发，外部不得直接调用。
  private notifyDetected(detected: boolean): void {
    this.detected = detected
    this.observers.forEach(o => { if (o.onShoulderSurfingChanged) o.onShoulderSurfingChanged(detected) })
  }
}
