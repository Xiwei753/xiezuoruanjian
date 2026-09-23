// GripPostureApi20 — 握持姿态 API20 facade（单例）。
// 直接 import @kit.MultimodalAwarenessKit 的 motion，把系统枚举映射成稳定 GripPosture。
// 只有 impl/apiXX 文件可以 import @kit.*；Service 不直接 import @kit.*。
//
// 真实事件源：motion.on('holdingHandChanged')（since 20）。
// 错误码 801 = 能力不支持；失败时不伪造状态（回调不触发）。
import { motion } from '@kit.MultimodalAwarenessKit'
import { GripPosture } from '../../GripPostureTypes'
import { PlatformApiResolver } from '../../../version/PlatformApiResolver'

// motion.HoldingHandStatus 枚举值（since 20）。
// NOT_HELD=0, LEFT_HAND_HELD=1, RIGHT_HAND_HELD=2, BOTH_HANDS_HELD=3, UNKNOWN_STATUS=16。
const HOLDING_NOT_HELD: number = 0
const HOLDING_LEFT: number = 1
const HOLDING_RIGHT: number = 2
const HOLDING_BOTH: number = 3

export class GripPostureApi20 {
  private static instance: GripPostureApi20

  static getInstance(): GripPostureApi20 {
    if (!GripPostureApi20.instance) {
      GripPostureApi20.instance = new GripPostureApi20()
    }
    return GripPostureApi20.instance
  }

  // 查询系统能力是否支持握姿感知。
  // 需同时满足 API>=20 和 SystemCapability.MultimodalAwareness.Motion，经 PlatformApiResolver 统一。
  isSupported(): boolean {
    const r: PlatformApiResolver = PlatformApiResolver.getInstance()
    return r.isApiAtLeast(20) && r.hasSystemCapability('SystemCapability.MultimodalAwareness.Motion')
  }

  // 启动 motion.on('holdingHandChanged') 监听，系统枚举映射成 GripPosture。
  // 返回真实注册结果：只有 motion.on 真正注册成功才返回 true，失败返回 false。
  // 801 或失败则不伪造状态（回调不触发，调用方据 startListening() 返回值判断）。
  startListening(onChanged: (posture: GripPosture) => void): boolean {
    try {
      motion.on('holdingHandChanged', (status: motion.HoldingHandStatus) => {
        onChanged(this.mapHoldingHand(status))
      })
      return true
    } catch (e) {
      // 系统不支持（错误码 801）或注册失败；不伪造状态，返回 false。
      return false
    }
  }

  // 停止 motion 监听。
  stopListening(): void {
    try {
      motion.off('holdingHandChanged')
    } catch (e) {
      // off 失败不阻塞。
    }
  }

  // —— private ——
  // motion.HoldingHandStatus → 稳定 GripPosture。
  private mapHoldingHand(status: motion.HoldingHandStatus): GripPosture {
    const n: number = status as number
    if (n === HOLDING_NOT_HELD) {
      return 'notHeld'
    }
    if (n === HOLDING_LEFT) {
      return 'left'
    }
    if (n === HOLDING_RIGHT) {
      return 'right'
    }
    if (n === HOLDING_BOTH) {
      return 'both'
    }
    // UNKNOWN_STATUS(16) 及其余未知值都归 unknown。
    return 'unknown'
  }
}
