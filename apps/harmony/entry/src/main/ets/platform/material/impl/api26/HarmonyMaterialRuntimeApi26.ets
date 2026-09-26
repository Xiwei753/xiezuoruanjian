// HarmonyMaterialRuntimeApi26 — API26+ 系统材质配置状态读取。
// Issue #773 评论 5844518092 第5项：从 @kit.ArkUI uiMaterial 读取材质信息，
// 写进诊断包 theme_runtime_sanitized.json，不再靠截图猜"是不是系统材质"。
//
// Issue #773 评论 5846511131：getMaterialInfo().state 是应用 module.json5 的材质配置状态，
// DEFAULT / ENABLE / DISABLE 对应配置值，不代表某个 HDS 组件已经实际生效。
// 删除 immersiveMaterialSupported（state !== DISABLE 推导不成立），
// 改为真实语义字段：appMaterialState / appMaterialType / hdsRequestedMaterialType / hdsRequestedMaterialLevel。
//
// SDK 实际 API（@ohos.arkui.uiMaterial.d.ts，@since 26.0.0，即 API26）：
//   uiMaterial.getMaterialInfo(): MaterialInfo
//   MaterialInfo { state: MaterialState, type: MaterialType }
//   MaterialState: DEFAULT=0, ENABLE=1, DISABLE=2
//   MaterialType:  IMMERSIVE=2
//
// 官方说明：
// https://developer.huawei.com/consumer/cn/doc/HarmonyOS-Guides/arkts-immersive-light-sense-enable

import { uiMaterial } from '@kit.ArkUI'
import hilog from '@ohos.hilog'

// 材质运行态 DTO — 与 HarmonyDiagnosticsRuntimeSnapshot.ThemeRuntimeDto 材质字段对齐。
// Issue #773 评论 5846511131：字段改为真实语义，不再用 state !== DISABLE 推导 immersiveMaterialSupported。
export interface MaterialRuntimeInfo {
  appMaterialState: string           // module.json5 材质配置状态：DEFAULT/ENABLE/DISABLE
  appMaterialType: string            // module.json5 材质配置类型：IMMERSIVE 等
  hdsRequestedMaterialType: string   // 应用层 HDS 请求的材质类型（固定 "IMMERSIVE"）
  hdsRequestedMaterialLevel: string  // 应用层 HDS 请求的材质级别（固定 "ADAPTIVE"）
}

const LOG_TAG = 'HarmonyMaterialRuntimeApi26'
const DOMAIN = 0x0000

// HDS 请求的材质类型/级别常量 — 应用层声明值，不随系统变化。
const HDS_REQUESTED_MATERIAL_TYPE = 'IMMERSIVE'
const HDS_REQUESTED_MATERIAL_LEVEL = 'ADAPTIVE'

export class HarmonyMaterialRuntimeApi26 {
  private static instance: HarmonyMaterialRuntimeApi26

  private constructor() {}

  static getInstance(): HarmonyMaterialRuntimeApi26 {
    if (!HarmonyMaterialRuntimeApi26.instance) {
      HarmonyMaterialRuntimeApi26.instance = new HarmonyMaterialRuntimeApi26()
    }
    return HarmonyMaterialRuntimeApi26.instance
  }

  // 读取材质配置状态。API26 调用 uiMaterial.getMaterialInfo() 真实接口。
  // Issue #773 评论 5846511131：state 是配置状态不是支持状态，不推导 immersiveMaterialSupported。
  // 失败时 appMaterialState/appMaterialType 写 "error"，不伪造成功。
  getMaterialRuntimeInfo(): MaterialRuntimeInfo {
    try {
      const info = uiMaterial.getMaterialInfo()
      const result: MaterialRuntimeInfo = {
        appMaterialState: this.materialStateName(info.state),
        appMaterialType: this.materialTypeName(info.type),
        hdsRequestedMaterialType: HDS_REQUESTED_MATERIAL_TYPE,
        hdsRequestedMaterialLevel: HDS_REQUESTED_MATERIAL_LEVEL
      }
      return result
    } catch (e) {
      hilog.error(DOMAIN, LOG_TAG, 'getMaterialInfo failed: %{public}s', String(e))
      const fallback: MaterialRuntimeInfo = {
        appMaterialState: 'error',
        appMaterialType: 'error',
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
