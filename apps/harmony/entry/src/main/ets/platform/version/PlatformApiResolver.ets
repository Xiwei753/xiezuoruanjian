// PlatformApiResolver — HarmonyOS API Level / SystemCapability 集中解析（单例）。
// 页面/feature/CapabilityRegistry 都不自己判断 HarmonyOS API，统一走这里。
// 版本分流集中在此；impl/apiXX facade 用它判断能力，Service 不接触 API Level。
//
// 版本解析器允许 import @kit.BasicServicesKit 读系统版本；其他 @kit.* 不 import。
// canIUse 是 ArkTS 全局函数，直接调用。
// 官方参考：https://developer.huawei.com/consumer/cn/forum/topic/0207223312558379085
import { deviceInfo } from '@kit.BasicServicesKit'

export class PlatformApiResolver {
  private static instance: PlatformApiResolver
  private apiLevel: number = 0
  private levelResolved: boolean = false

  static getInstance(): PlatformApiResolver {
    if (!PlatformApiResolver.instance) {
      PlatformApiResolver.instance = new PlatformApiResolver()
    }
    return PlatformApiResolver.instance
  }

  // 包装全局 canIUse（ArkTS 全局函数），try/catch 返回 false。
  canIUse(syscap: string): boolean {
    try {
      return canIUse(syscap)
    } catch (e) {
      return false
    }
  }

  // 读取 API Level。用 deviceInfo.sdkApiVersion（number）作为 API level；
  // 不可用就保守返回 0。结果缓存。
  getApiLevel(): number {
    if (this.levelResolved) {
      return this.apiLevel
    }
    try {
      this.apiLevel = deviceInfo.sdkApiVersion
      this.levelResolved = true
    } catch (e) {
      // 读取失败；保守返回 0，不标记已解析以便下次重试。
      this.apiLevel = 0
      this.levelResolved = false
    }
    return this.apiLevel
  }

  // 同 canIUse。语义别名，供调用方按"系统能力"语义使用。
  hasSystemCapability(syscap: string): boolean {
    return this.canIUse(syscap)
  }

  // 判断 API Level 是否 >= level。
  isApiAtLeast(level: number): boolean {
    return this.getApiLevel() >= level
  }
}
