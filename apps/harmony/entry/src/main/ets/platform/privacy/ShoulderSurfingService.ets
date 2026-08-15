// ShoulderSurfingService — 防窥（Device Security Kit）服务。
// 检测身后有人窥视时通知 UI（如降低正文对比度 / 显示提示）。
// 不调 Core，只暴露平台状态。

export interface ShoulderSurfingObserver {
  onShoulderSurfingChanged?(detected: boolean): void
}

export class ShoulderSurfingService {
  private static instance: ShoulderSurfingService
  private supported: boolean = false
  private enabled: boolean = false
  private detected: boolean = false
  private observers: Set<ShoulderSurfingObserver> = new Set<ShoulderSurfingObserver>()

  static getInstance(): ShoulderSurfingService {
    if (!ShoulderSurfingService.instance) {
      ShoulderSurfingService.instance = new ShoulderSurfingService()
    }
    return ShoulderSurfingService.instance
  }

  isSupported(): boolean {
    return this.supported
  }

  isEnabled(): boolean {
    return this.enabled
  }

  isShoulderSurfingDetected(): boolean {
    return this.detected
  }

  setEnabled(enabled: boolean): void {
    this.enabled = enabled
  }

  addObserver(observer: ShoulderSurfingObserver): void {
    this.observers.add(observer)
  }

  removeObserver(observer: ShoulderSurfingObserver): void {
    this.observers.delete(observer)
  }

  // 平台层在收到防窥检测结果时调用。
  notifyDetected(detected: boolean): void {
    this.detected = detected
    this.observers.forEach(o => { if (o.onShoulderSurfingChanged) o.onShoulderSurfingChanged(detected) })
  }
}
