// HarmonyMaterialRuntimeApi26 — API26+ 系统材质真实运行态读取。
// Issue #773 评论 5844518092 第5项：从 @kit.ArkUI uiMaterial 读取材质信息，
// 写进诊断包 theme_runtime_sanitized.json，不再靠截图猜"是不是系统材质"。
//
// SDK 实际 API（@ohos.arkui.uiMaterial.d.ts，@since 26.0.0，即 API26）：
//   uiMaterial.getMaterialInfo(): MaterialInfo
//   MaterialInfo { state: MaterialState, type: MaterialType }
//   MaterialState: DEFAULT=0, ENABLE=1, DISABLE=2
//   MaterialType:  IMMERSIVE=2
// 评论里提到的 isImmersiveMaterialSupported() / getGlobalMaterialLevel() 在 SDK 中
// 不存在，此处以 SDK 实际签名为准：
//   - immersiveMaterialSupported 用 state !== DISABLE 推断（DEFAULT/ENABLE 均算吃到沉浸光感）
//   - globalMaterialLevel 写 "unavailable"（SDK 未暴露全局材质级别查询）

import { uiMaterial } from '@kit.ArkUI'
import hilog from '@ohos.hilog'

// 材质运行态 DTO — 与 HarmonyDiagnosticsRuntimeSnapshot.ThemeRuntimeDto 材质字段对齐。
export interface MaterialRuntimeInfo {
  materialState: string
  materialType: string
  immersiveMaterialSupported: boolean
  globalMaterialLevel: string
  hdsRequestedMaterialType: string  // 固定 "IMMERSIVE"
  hdsRequestedMaterialLevel: string // 固定 "ADAPTIVE"
}

const LOG_TAG = 'HarmonyMaterialRuntimeApi26'
const DOMAIN = 0x0000

// HDS 请求的材质类型/级别常量 — 应用层声明值，不随系统变化。
const HDS_REQUESTED_MATERIAL_TYPE = 'IMMERSIVE'
const HDS_REQUESTED_MATERIAL_LEVEL = 'ADAPTIVE'

// SDK 未暴露全局材质级别查询 API（getGlobalMaterialLevel 不存在）。
const GLOBAL_MATERIAL_LEVEL_UNAVAILABLE = 'unavailable'

export class HarmonyMaterialRuntimeApi26 {
  private static instance: HarmonyMaterialRuntimeApi26

  private constructor() {}

  static getInstance(): HarmonyMaterialRuntimeApi26 {
    if (!HarmonyMaterialRuntimeApi26.instance) {
      HarmonyMaterialRuntimeApi26.instance = new HarmonyMaterialRuntimeApi26()
    }
    return HarmonyMaterialRuntimeApi26.instance
  }

  // 读取材质运行态。API26 调用 uiMaterial.getMaterialInfo() 真实接口。
  // 失败时 materialState/materialType 写 "error"，不伪造成功。
  getMaterialRuntimeInfo(): MaterialRuntimeInfo {
    try {
      const info = uiMaterial.getMaterialInfo()
      const materialState = this.materialStateName(info.state)
      const materialType = this.materialTypeName(info.type)
      // 沉浸光感是否生效：state 非 DISABLE 即表示沉浸材质对部分/全部组件生效。
      const immersiveMaterialSupported = info.state !== uiMaterial.MaterialState.DISABLE
      const result: MaterialRuntimeInfo = {
        materialState: materialState,
        materialType: materialType,
        immersiveMaterialSupported: immersiveMaterialSupported,
        globalMaterialLevel: GLOBAL_MATERIAL_LEVEL_UNAVAILABLE,
        hdsRequestedMaterialType: HDS_REQUESTED_MATERIAL_TYPE,
        hdsRequestedMaterialLevel: HDS_REQUESTED_MATERIAL_LEVEL
      }
      return result
    } catch (e) {
      hilog.error(DOMAIN, LOG_TAG, 'getMaterialInfo failed: %{public}s', String(e))
      const fallback: MaterialRuntimeInfo = {
        materialState: 'error',
        materialType: 'error',
        immersiveMaterialSupported: false,
        globalMaterialLevel: GLOBAL_MATERIAL_LEVEL_UNAVAILABLE,
        hdsRequestedMaterialType: HDS_REQUESTED_MATERIAL_TYPE,
        hdsRequestedMaterialLevel: HDS_REQUESTED_MATERIAL_LEVEL
      }
      return fallback
    }
  }

  // MaterialState 枚举值 → 字符串名。未知值回退为 "UNKNOWN(<n>)"。
  private materialStateName(state: uiMaterial.MaterialState): string {
    switch (state) {
      case uiMaterial.MaterialState.DEFAULT:
        return 'DEFAULT'
      case uiMaterial.MaterialState.ENABLE:
        return 'ENABLE'
      case uiMaterial.MaterialState.DISABLE:
        return 'DISABLE'
      default:
        return `UNKNOWN(${state})`
    }
  }

  // MaterialType 枚举值 → 字符串名。未知值回退为 "UNKNOWN(<n>)"。
  private materialTypeName(type: uiMaterial.MaterialType): string {
    switch (type) {
      case uiMaterial.MaterialType.IMMERSIVE:
        return 'IMMERSIVE'
      default:
        return `UNKNOWN(${type})`
    }
  }
}
