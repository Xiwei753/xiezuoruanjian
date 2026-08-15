// CapabilityRegistry — 鸿蒙系统特色能力注册中心（单例）。
// 集中探测 GripPosture / ShoulderSurfing / WindowPrivacy / Continuation / Share / Stylus。
// 不复用编辑器 capability；编辑器能力由 EditorRenderBackend 自描述。

import { HarmonySystemCapabilities, DEFAULT_HARMONY_CAPABILITIES } from './HarmonyCapabilities'

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

  // 运行时探测系统能力并更新缓存。具体探测由各平台服务（GripPostureService 等）提供。
  refresh(): void {
    // 占位：实际探测在各服务初始化时回填。此处保持默认值。
    this.capabilities = DEFAULT_HARMONY_CAPABILITIES
  }
}
