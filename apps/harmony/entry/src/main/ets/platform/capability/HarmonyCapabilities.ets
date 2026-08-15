// HarmonySystemCapabilities — 鸿蒙系统特色能力集合。
// 启动时由 CapabilityRegistry 探测并报告给 UI / 编辑器链路。
// 不复用编辑器 capability（编辑器能力由 EditorRenderBackend 静态链路自描述）。

export interface HarmonySystemCapabilities {
  supportsGripPosture: boolean
  supportsShoulderSurfing: boolean
  supportsWindowPrivacy: boolean
  supportsAppContinuation: boolean
  supportsShare: boolean
  supportsStylus: boolean
}

// 默认值：保守起见全部 false，由 CapabilityRegistry.refresh() 在运行时探测后覆盖。
export const DEFAULT_HARMONY_CAPABILITIES: HarmonySystemCapabilities = {
  supportsGripPosture: false,
  supportsShoulderSurfing: false,
  supportsWindowPrivacy: false,
  supportsAppContinuation: false,
  supportsShare: false,
  supportsStylus: false
}
