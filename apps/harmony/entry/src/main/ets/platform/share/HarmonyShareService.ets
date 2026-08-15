// HarmonyShareService — 系统分享/碰一碰/隔空传送。
// 页面和星图/写作区不直接依赖 Share Kit，统一通过本服务。
// 不调 Core，只做平台分享桥接。

import { SharePayload } from './SharePayload'

export class HarmonyShareService {
  private static instance: HarmonyShareService

  static getInstance(): HarmonyShareService {
    if (!HarmonyShareService.instance) {
      HarmonyShareService.instance = new HarmonyShareService()
    }
    return HarmonyShareService.instance
  }

  // 普通系统分享（SystemShareKit）。
  async share(payload: SharePayload): Promise<boolean> {
    // 运行时由 @ohos.systemShareManager 发起；静态骨架返回 true 表示入口已接通。
    return true
  }

  // 碰一碰分享（NearbyShare）。
  async shareViaNearby(payload: SharePayload): Promise<boolean> {
    return true
  }

  // 隔空传送（跨设备 Continuation + Share）。
  async shareViaAirDrop(payload: SharePayload): Promise<boolean> {
    return true
  }
}
