// DlpAntiPeepApi23 — 防窥 API23 facade（单例）。
// 只放 API23（since 6.1.0(23)）方法：requestAntiPeepOptions / publishAntiPeepInformation。
//
// 本模块（privacy_acl HSP）自包含：不依赖 entry 的 PlatformApiResolver，
// 直接用 @kit.BasicServicesKit 的 deviceInfo 读 API Level，用全局 canIUse 查 SystemCapability。
import { dlpAntiPeep } from '@kit.DeviceSecurityKit'
import { common } from '@kit.AbilityKit'
import { deviceInfo } from '@kit.BasicServicesKit'

// dlpAntiPeep.AntiPeepOptionsResult.FAIL（since 6.1.0(23)）：SUCCESS=0, FAIL=1, ALREADY_ON=2。
const ANTI_PEEP_OPTIONS_FAIL: number = 1

// 内联版本判断：privacy_acl 模块不依赖 entry 的 PlatformApiResolver。
function isApiAtLeast(level: number): boolean {
  try {
    return deviceInfo.sdkApiVersion >= level
  } catch (e) {
    return false
  }
}

function hasSystemCapability(syscap: string): boolean {
  try {
    return canIUse(syscap)
  } catch (e) {
    return false
  }
}

export class DlpAntiPeepApi23 {
  private static instance: DlpAntiPeepApi23

  static getInstance(): DlpAntiPeepApi23 {
    if (!DlpAntiPeepApi23.instance) {
      DlpAntiPeepApi23.instance = new DlpAntiPeepApi23()
    }
    return DlpAntiPeepApi23.instance
  }

  // 查询系统能力是否支持 API23 防窥设置页。
  // 需同时满足 SystemCapability 和 API>=23。
  isSupported(): boolean {
    return hasSystemCapability('SystemCapability.Security.DlpAntiPeep') && isApiAtLeast(23)
  }

  // 打开防窥设置页（since 6.1.0(23)）。
  // try/catch；失败返回 FAIL，不伪造成功。
  async requestAntiPeepOptions(context: common.UIAbilityContext): Promise<dlpAntiPeep.AntiPeepOptionsResult> {
    try {
      return await dlpAntiPeep.requestAntiPeepOptions(context)
    } catch (e) {
      // 系统调用失败；返回 FAIL，不伪造成功。
      return ANTI_PEEP_OPTIONS_FAIL as dlpAntiPeep.AntiPeepOptionsResult
    }
  }

  // 发布防窥信息（since 6.1.0(23)）。
  // try/catch；失败静默返回，不伪造。
  async publishAntiPeepInformation(): Promise<void> {
    try {
      await dlpAntiPeep.publishAntiPeepInformation()
    } catch (e) {
      // 失败不阻塞。
    }
  }
}
