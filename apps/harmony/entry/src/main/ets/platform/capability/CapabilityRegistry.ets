// CapabilityRegistry — 鸿蒙系统特色能力注册中心（单例）。
// 集中探测 GripPosture / ShoulderSurfing / WindowPrivacy / Continuation / Share / Stylus。
// 不复用编辑器 capability；编辑器能力由 EditorRenderBackend 自描述。
//
// refresh() 查询各 service 真实 support 状态汇总，不靠默认 bool 假装能力存在。
//
// Issue #629 评论5 第6部分：
//   - 不再探测 systemShareManager / shareContent（普通分享已改 systemShare.SharedData + ShareController）。
//   - 不再 InputDeviceService.hasDeviceOfKind('stylus')（InputDeviceService 不再产生 stylus kind）。
//   - stylus 能力从 StylusInputService.isSupported() 读，由它统一负责。
//   - share 能力从 SystemShareService/TapShareService/AirTransferService 三后端实际 support 汇总，
//     本类不认识 Share Kit API 名称。

import { HarmonySystemCapabilities, DEFAULT_HARMONY_CAPABILITIES } from './HarmonyCapabilities'
import { GripPostureService } from '../awareness/GripPostureService'
import { ShoulderSurfingService } from '../privacy/ShoulderSurfingService'
import { WindowPrivacyService } from '../privacy/WindowPrivacyService'
import { StylusInputService } from '../input/StylusInputService'
import { SystemShareService } from '../share/SystemShareService'
import { TapShareService } from '../share/TapShareService'
import { AirTransferService } from '../share/AirTransferService'

export class CapabilityRegistry {
  private static instance: CapabilityRegistry
  private capabilities: HarmonySystemCapabilities = DEFAULT_HARMONY_CAPABILITIES

  static getInstance(): CapabilityRegistry {
    if (!CapabilityRegistry.instance) {
      CapabilityRegistry.instance = new CapabilityRegistry()
    }
    return CapabilityRegistry.instance
  }

  getCapabilities(): HarmonySystemCapabilities {
    return this.capabilities
  }

  // 运行时探测系统能力并更新缓存。
  // 查询各 service 真实 support 状态：
  //   supportsGripPosture → GripPostureService.isSupported()
  //   supportsShoulderSurfing → ShoulderSurfingService.isSupported()
  //   supportsWindowPrivacy → WindowPrivacyService.isSupported()
  //   supportsAppContinuation → module.json5 continuable:true（运行时直接 true）
  //   supportsShare → SystemShareService/TapShareService/AirTransferService 三后端任一 isSupported()
  //   supportsStylus → StylusInputService.isSupported()
  // 不探测 systemShareManager.shareContent；不调 InputDeviceService.hasDeviceOfKind('stylus')。
  refresh(): void {
    const grip: GripPostureService = GripPostureService.getInstance()
    const shoulder: ShoulderSurfingService = ShoulderSurfingService.getInstance()
    const windowPrivacy: WindowPrivacyService = WindowPrivacyService.getInstance()
    const stylus: StylusInputService = StylusInputService.getInstance()
    const systemShare: SystemShareService = SystemShareService.getInstance()
    const tapShare: TapShareService = TapShareService.getInstance()
    const airTransfer: AirTransferService = AirTransferService.getInstance()

    this.capabilities = {
      supportsGripPosture: grip.isSupported(),
      supportsShoulderSurfing: shoulder.isSupported(),
      supportsWindowPrivacy: windowPrivacy.isSupported(),
      supportsAppContinuation: this.detectAppContinuation(),
      supportsShare: systemShare.isSupported() || tapShare.isSupported() || airTransfer.isSupported(),
      supportsStylus: stylus.isSupported()
    }
  }

  // —— private ——
  // 应用接续能力：module.json5 中 EntryAbility 已设 continuable:true。
  // 运行时直接 true（配置在编译期确定）。
  private detectAppContinuation(): boolean {
    return true
  }
}
