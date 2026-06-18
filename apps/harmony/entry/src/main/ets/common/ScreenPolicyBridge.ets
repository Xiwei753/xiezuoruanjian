import hilog from '@ohos.hilog'
import { getBridge } from './AppContext'
import {
  ScreenRole, ActionRole, ActionPlacement, ShellMode,
  ActionSlot, ScreenPolicy
} from '../model/CoreDtos'

const TAG = 'ScreenPolicyBridge'
const DOMAIN = 0x0000

/**
 * Screen Policy Bridge — 通过 Core resolve_screen_policy 获取跨端统一动作位置语义。
 *
 * Harmony 端传入 ScreenRole + ShellMode，调用 Core NAPI 获取 ActionSlot 列表。
 * 不允许 Harmony 端自行决定动作位置，所有位置必须来自 Core 的 ActionPlacement。
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

  async resolveScreenPolicy(screenRole: ScreenRole, shellMode: ShellMode): Promise<ScreenPolicy | null> {
    try {
      const bridge = getBridge()
      const result = await bridge.resolveScreenPolicy(screenRole, shellMode)
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
    const rawSlots = data['actionSlots'] as Array<Record<string, Object>>
    const actionSlots: ActionSlot[] = rawSlots.map((slot: Record<string, Object>) => {
      const rawVisibleIn = slot['visibleIn'] as string[]
      return {
        actionId: slot['actionId'] as string,
        role: this.parseActionRole(slot['role'] as string),
        placement: this.parseActionPlacement(slot['placement'] as string),
        visibleIn: rawVisibleIn.map((s: string) => this.parseShellMode(s)),
        requiresConfirmation: slot['requiresConfirmation'] as boolean
      }
    })

    return {
      screenRole: this.parseScreenRole(data['screenRole'] as string),
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

  private parseActionPlacement(value: string): ActionPlacement {
    const entries = Object.entries(ActionPlacement)
    for (const [, v] of entries) {
      if (v === value) return v as ActionPlacement
    }
    return ActionPlacement.TopTrailing
  }

  private parseShellMode(value: string): ShellMode {
    switch (value) {
      case 'SinglePane': return ShellMode.SinglePane
      case 'SupportingPane': return ShellMode.SupportingPane
      case 'TwoPane': return ShellMode.TwoPane
      default: return ShellMode.SinglePane
    }
  }
}

export function getScreenPolicyBridge(): ScreenPolicyBridge {
  return ScreenPolicyBridge.getInstance()
}