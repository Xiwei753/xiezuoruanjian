// HarmonyMaterialService — 系统材质（沉浸光感）能力的稳定门面。
//
// 页面层只依赖这个门面，不直接碰 @kit.ArkUI 的 uiMaterial：
// 版本判定和能力探测收在 impl 里，将来系统语义变化只改一处。
// 门面如实返回平台对象或 undefined，不在这里做参数兜底 ——
// 兜底出来的假材质会让「材质没生效」和「材质生效了但参数不对」两件事混在一起，
// 实机排查时无法区分。

import { uiMaterial } from '@kit.ArkUI'
import { PlatformApiResolver } from '../version/PlatformApiResolver'
import { ImmersiveMaterialApi26 } from './impl/api26/ImmersiveMaterialApi26'

export class HarmonyMaterialService {
  private static instance: HarmonyMaterialService

  static getInstance(): HarmonyMaterialService {
    if (!HarmonyMaterialService.instance) {
      HarmonyMaterialService.instance = new HarmonyMaterialService()
    }
    return HarmonyMaterialService.instance
  }

  /**
   * 沉浸式系统材质对象。API < 26 或设备不支持时返回 undefined，
   * 调用方直接给 systemMaterial(undefined) 即可退回普通背景。
   *
   * applyShadow 传 false 时材质不再自带阴影，调用方要自己设 shadow 通用属性
   * （官方唯一明文允许的替换通道，详见 impl 里的注释）。
   */
  floatingMaterial(applyShadow: boolean = true): uiMaterial.ImmersiveMaterial | undefined {
    if (!PlatformApiResolver.getInstance().isApiAtLeast(26)) {
      return undefined
    }
    const api: ImmersiveMaterialApi26 = ImmersiveMaterialApi26.getInstance()
    if (!api.isSupported()) {
      return undefined
    }
    return api.buildFloatingMaterial(applyShadow)
  }
}
