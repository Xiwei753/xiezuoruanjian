import hilog from '@ohos.hilog'
import { getBridge } from '../../app/AppContext'
import { ScreenRole, ScreenPolicy } from '../../corebridge/dto/PlatformDtos'

const TAG = 'ScreenPolicyBridge'
const DOMAIN = 0x0000

/**
 * Screen Policy Bridge — 通过 Core resolve_screen_policy 获取跨端统一动作区域语义。
 *
 * #610：Harmony 端只传入 ScreenRole，动作区域（region）与顺序（order）是产品语义，
 * 不随壳层模式变化；控件呈现由 HarmonyStyleAdapter 决定。
 */
export class ScreenPolicyBridge {
  private static instance: ScreenPolicyBridge

  private constructor() {}

  static getInstance(): ScreenPolicyBridge {
    if (!ScreenPolicyBridge.instance) {
      ScreenPolicyBridge.instance = new ScreenPolicyBridge()
    }
    return ScreenPolicyBridge.instance
  }

  async resolveScreenPolicy(screenRole: ScreenRole): Promise<ScreenPolicy | null> {
    try {
      const bridge = getBridge()
      const result = await bridge.resolveScreenPolicy(screenRole)
      if (result.success && result.data) {
        return result.data
      }
      return null
    } catch (err) {
      hilog.error(DOMAIN, TAG, `resolveScreenPolicy failed: ${JSON.stringify(err)}`)
      return null
    }
  }

}

export function getScreenPolicyBridge(): ScreenPolicyBridge {
  return ScreenPolicyBridge.getInstance()
}
