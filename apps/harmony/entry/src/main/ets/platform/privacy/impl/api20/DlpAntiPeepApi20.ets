// DlpAntiPeepApi20 — 防窥 API20 基础 facade（单例）。
// 直接 import @kit.DeviceSecurityKit 的 dlpAntiPeep，完成 API20+ 基础能力：
// 支持状态、窥屏状态、监听启停、错误映射。
// 只有 impl/apiXX 文件可以 import @kit.*；Service 不直接 import @kit.*。
//
// 真实事件源：dlpAntiPeep.on('dlpAntiPeep')（since 6.0.0(20)）。
import { dlpAntiPeep } from '@kit.DeviceSecurityKit'
import { PlatformApiResolver } from '../../../version/PlatformApiResolver'

// dlpAntiPeep.DlpAntiPeepStatus 枚举值（since 6.0.0(20)）。
// PASS=0（安全/无窥视）, HIDE=1（被窥视/信息隐藏）。
const ANTI_PEEP_HIDE: number = 1

export class DlpAntiPeepApi20 {
  private static instance: DlpAntiPeepApi20

  static getInstance(): DlpAntiPeepApi20 {
    if (!DlpAntiPeepApi20.instance) {
      DlpAntiPeepApi20.instance = new DlpAntiPeepApi20()
    }
    return DlpAntiPeepApi20.instance
  }

  // 查询系统能力是否支持防窥。
  // 用 canIUse('SystemCapability.Security.DlpAntiPeep') 判断，经 PlatformApiResolver 统一。
  isSupported(): boolean {
    return PlatformApiResolver.getInstance().hasSystemCapability('SystemCapability.Security.DlpAntiPeep')
  }

  // 防窥开关是否启用。
  async isSwitchOn(): Promise<boolean> {
    try {
      return await dlpAntiPeep.isDlpAntiPeepSwitchOn()
    } catch (e) {
      // 系统调用失败；保守返回 false。
      return false
    }
  }

  // 当前是否被窥视。HIDE=1→true（被窥视），PASS=0→false（安全）；失败返回 false。
  getPeepingStatus(): boolean {
    try {
      const status: dlpAntiPeep.DlpAntiPeepStatus = dlpAntiPeep.getDlpAntiPeepInfo()
      const n: number = status as number
      return n === ANTI_PEEP_HIDE
    } catch (e) {
      // 系统调用失败；保守返回 false（无窥视）。
      return false
    }
  }

  // 启动 dlpAntiPeep.on('dlpAntiPeep') 监听，DlpAntiPeepStatus 映射成 boolean。
  // 失败则不伪造状态（回调不触发）。
  startListening(onPeeping: (peeping: boolean) => void): void {
    try {
      dlpAntiPeep.on('dlpAntiPeep', (status: dlpAntiPeep.DlpAntiPeepStatus) => {
        onPeeping(this.mapPeepStatus(status))
      })
    } catch (e) {
      // 注册失败；不伪造状态。
    }
  }

  // 停止 dlpAntiPeep 监听。
  stopListening(): void {
    try {
      dlpAntiPeep.off('dlpAntiPeep')
    } catch (e) {
      // off 失败不阻塞。
    }
  }

  // 透传防窥信息给系统。
  passDlpAntiPeepInfo(): void {
    try {
      dlpAntiPeep.passDlpAntiPeepInfo()
    } catch (e) {
      // 失败不阻塞。
    }
  }

  // —— private ——
  // dlpAntiPeep.DlpAntiPeepStatus → boolean（HIDE=1→true 被窥视）。
  private mapPeepStatus(status: dlpAntiPeep.DlpAntiPeepStatus): boolean {
    const n: number = status as number
    return n === ANTI_PEEP_HIDE
  }
}
