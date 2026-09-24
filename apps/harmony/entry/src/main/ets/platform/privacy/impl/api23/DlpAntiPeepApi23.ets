// DlpAntiPeepApi23 — 防窥 API23 facade（单例）。
// 只放 API23（since 6.1.0(23)）方法：requestAntiPeepOptions / publishAntiPeepInformation。
// 不把 API23 方法塞进 api20 文件。版本敏感的 @kit.* 系统调用收进 impl/apiXX；稳定 Service 不直接 import 版本敏感的高版本能力 Kit（@kit.AbilityKit 的 common 等基础平台类型除外）。
import { dlpAntiPeep } from '@kit.DeviceSecurityKit'
import { common } from '@kit.AbilityKit'
import { PlatformApiResolver } from '../../../version/PlatformApiResolver'

// dlpAntiPeep.AntiPeepOptionsResult.FAIL（since 6.1.0(23)）：SUCCESS=0, FAIL=1, ALREADY_ON=2。
const ANTI_PEEP_OPTIONS_FAIL: number = 1

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
    const resolver: PlatformApiResolver = PlatformApiResolver.getInstance()
    return resolver.hasSystemCapability('SystemCapability.Security.DlpAntiPeep') && resolver.isApiAtLeast(23)
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
