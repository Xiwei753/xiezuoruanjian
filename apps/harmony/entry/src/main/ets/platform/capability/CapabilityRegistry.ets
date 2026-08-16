// CapabilityRegistry — 鸿蒙系统特色能力注册中心（单例）。
// 集中探测 GripPosture / ShoulderSurfing / WindowPrivacy / Continuation / Share / Stylus。
// 不复用编辑器 capability；编辑器能力由 EditorRenderBackend 自描述。
//
// refresh() 查询各 service 真实 support 状态汇总，不靠默认 bool 假装能力存在。

import { HarmonySystemCapabilities, DEFAULT_HARMONY_CAPABILITIES } from './HarmonyCapabilities'
import { GripPostureService } from '../awareness/GripPostureService'
import { ShoulderSurfingService } from '../privacy/ShoulderSurfingService'
import { WindowPrivacyService } from '../privacy/WindowPrivacyService'
import { InputDeviceService } from '../input/InputDeviceService'
import { systemShareManager } from '@kit.ShareKit'

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
  //   supportsShare → systemShareManager 可用性（try/catch 探测）
  //   supportsStylus → InputDeviceService 查询是否有 stylus 设备
  refresh(): void {
    const grip: GripPostureService = GripPostureService.getInstance()
    const shoulder: ShoulderSurfingService = ShoulderSurfingService.getInstance()
    const windowPrivacy: WindowPrivacyService = WindowPrivacyService.getInstance()
    const inputDevice: InputDeviceService = InputDeviceService.getInstance()

    this.capabilities = {
      supportsGripPosture: grip.isSupported(),
      supportsShoulderSurfing: shoulder.isSupported(),
      supportsWindowPrivacy: windowPrivacy.isSupported(),
      supportsAppContinuation: this.detectAppContinuation(),
      supportsShare: this.detectShare(),
      supportsStylus: inputDevice.hasDeviceOfKind('stylus')
    }
  }

  // —— private ——
  // 应用接续能力：module.json5 中 EntryAbility 已设 continuable:true。
  // 运行时直接 true（配置在编译期确定）。
  private detectAppContinuation(): boolean {
    return true
  }

  // 系统分享能力：探测 systemShareManager 是否可用。
  // 通过引用 systemShareManager 对象 + 检查 shareContent 方法存在性判断。
  // 静态 import 在不支持的设备上会编译失败，故本方法只在已成功 import 的前提下检查方法可达。
  private detectShare(): boolean {
    try {
      // systemShareManager 已通过静态 import 引入；检查其 shareContent 方法是否存在。
      const mgr: Object = systemShareManager as Object
      const fn: Object | undefined = (mgr as Record<string, Object>)['shareContent']
      return fn !== undefined && fn !== null
    } catch (e) {
      return false
    }
  }
}
