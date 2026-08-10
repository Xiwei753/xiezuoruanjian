import hilog from '@ohos.hilog'
import { getBridge } from './AppContext'
import {
  ScreenRole, ActionRole, ActionRegion, ActionTarget,
  ActionSlot, ScreenPolicy
} from '../model/CoreDtos'

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
        return this.parseScreenPolicy(result.data)
      }
      return null
    } catch (err) {
      hilog.error(DOMAIN, TAG, `resolveScreenPolicy failed: ${JSON.stringify(err)}`)
      return null
    }
  }

  private parseScreenPolicy(data: Record<string, Object>): ScreenPolicy {
    const rawSlots = (data['actionSlots'] ?? data['action_slots'] ?? []) as Array<Record<string, Object>>
    const actionSlots: ActionSlot[] = rawSlots.map((slot: Record<string, Object>) => {
      const rawOrder = (slot['order'] ?? 0) as number
      return {
        role: this.parseActionRole((slot['role'] ?? '') as string),
        // #610 评论二：业务目标身份（Delete+Volume vs Delete+Chapter 等）。
        target: this.parseActionTarget((slot['target'] ?? 'App') as string),
        region: this.parseActionRegion((slot['region'] ?? '') as string),
        order: Number(rawOrder),
        requiresConfirmation: (slot['requiresConfirmation'] ?? slot['requires_confirmation'] ?? false) as boolean
      }
    })
    actionSlots.sort((a: ActionSlot, b: ActionSlot) => a.order - b.order)

    return {
      screenRole: this.parseScreenRole((data['screenRole'] ?? data['screen_role'] ?? 'Home') as string),
      actionSlots: actionSlots
    }
  }

  private parseScreenRole(value: string): ScreenRole {
    const entries = Object.entries(ScreenRole)
    for (const [, v] of entries) {
      if (v === value) return v as ScreenRole
    }
    return ScreenRole.Home
  }

  private parseActionRole(value: string): ActionRole {
    const entries = Object.entries(ActionRole)
    for (const [, v] of entries) {
      if (v === value) return v as ActionRole
    }
    return ActionRole.Back
  }

  private parseActionRegion(value: string): ActionRegion {
    const entries = Object.entries(ActionRegion)
    for (const [, v] of entries) {
      if (v === value) return v as ActionRegion
    }
    return ActionRegion.HeaderTrailing
  }

  private parseActionTarget(value: string): ActionTarget {
    const entries = Object.entries(ActionTarget)
    for (const [, v] of entries) {
      if (v === value) return v as ActionTarget
    }
    return ActionTarget.App
  }
}

export function getScreenPolicyBridge(): ScreenPolicyBridge {
  return ScreenPolicyBridge.getInstance()
}
