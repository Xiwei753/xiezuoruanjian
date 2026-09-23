// SystemShareService — 普通系统分享后端（稳定 facade）。
// 独立类，独立后端：只走 @kit.ShareKit 的 systemShare.SharedData + systemShare.ShareController.show。
// 不负责碰一碰 / 隔空传送；那两个有各自独立的 service，不落本类。
// 失败返回 false，不假装成功。
//
// 真实接口经 SystemShareApi11 facade 调 @kit.ShareKit 的 systemShare：
//   systemShare.SharedData          since 4.1.0(11)
//   systemShare.ShareController     since 4.1.0(11)
//   ShareController.show(context)   since 4.1.0(11)
// Service 不直接 import @kit.ShareKit，系统调用和数据构造都在 impl/api11 facade。
// 只有 impl/api11 文件可以 import @kit.*；本稳定层仅保留 @kit.AbilityKit 的 common（基础平台类型）。
//
// UIAbilityContext 由 AppContext.setAbilityContext() 注入本 service，
// feature/page 不直接 import Share Kit。

import { common } from '@kit.AbilityKit'
import { SystemShareApi11 } from './impl/api11/SystemShareApi11'
import { SharePayload } from './SharePayload'

export class SystemShareService {
  private static instance: SystemShareService

  // UIAbilityContext，由 AppContext 注入。无 context 时 share 返回 false，不假装成功。
  private abilityContext: common.UIAbilityContext | null = null
  // 系统分享能力缓存：share() 成功过一次即置 true。CapabilityRegistry 从此读，不自己探测 Share Kit API。
  private supported: boolean = false

  static getInstance(): SystemShareService {
    if (!SystemShareService.instance) {
      SystemShareService.instance = new SystemShareService()
    }
    return SystemShareService.instance
  }

  // 由 AppContext.setAbilityContext() 调用，注入 UIAbilityContext。
  // 不持有 Activity/Window 引用以外的平台状态；context 仅用于 controller.show()。
  setAbilityContext(context: common.UIAbilityContext): void {
    this.abilityContext = context
  }

  // 系统分享能力：share() 成功过即 true；否则委托 impl 按 API Level + SystemCapability 判断。
  // CapabilityRegistry 汇总时读此方法。systemShare since API11 < compatibleSdkVersion 12，始终可用。
  isSupported(): boolean {
    return this.supported || SystemShareApi11.getInstance().isSupported()
  }

  // 系统分享入口：委托 SystemShareApi11 完成实际系统调用。
  // 成功返回 true，失败返回 false（系统 API 抛错或无 context/无内容时返回 false）。
  async share(payload: SharePayload): Promise<boolean> {
    if (this.abilityContext === null) {
      // 未注入 UIAbilityContext；返回真实失败，不假装成功。
      return false
    }

    const ok: boolean = await SystemShareApi11.getInstance().share(this.abilityContext, payload)
    if (ok) {
      this.supported = true
    }
    return ok
  }
}
