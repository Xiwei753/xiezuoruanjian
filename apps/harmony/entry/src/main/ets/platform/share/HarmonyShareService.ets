// HarmonyShareService — 系统分享/碰一碰/隔空传送的统一调度入口。
// 页面和星图/写作区不直接依赖 Share Kit，统一通过本服务。
// 不调 Core，只做平台分享桥接。
//
// 设计（Issue #629 评论 3 第 6 部分）：
//   本类**只调度**，不直接持有 toSharedContent，也不直接调 systemShareManager。
//   三个 channel 各自走独立后端 service：
//     'system'      → SystemShareService  （systemShareManager.shareContent）
//     'tap'         → TapShareService     （碰一碰官方独立流程，不落 systemShareManager）
//     'airTransfer' → AirTransferService  （sharableTarget.share() 官方独立流程，不落 systemShareManager）
//   不允许三个 channel 都落同一个 API。
// 失败返回真实失败（return false），不允许"入口已接通所以 true"。

import { SharePayload, ShareChannel } from './SharePayload'
import { SystemShareService } from './SystemShareService'
import { TapShareService } from './TapShareService'
import { AirTransferService } from './AirTransferService'

export class HarmonyShareService {
  private static instance: HarmonyShareService

  private readonly systemShare: SystemShareService
  private readonly tapShare: TapShareService
  private readonly airTransfer: AirTransferService

  static getInstance(): HarmonyShareService {
    if (!HarmonyShareService.instance) {
      HarmonyShareService.instance = new HarmonyShareService()
    }
    return HarmonyShareService.instance
  }

  private constructor() {
    this.systemShare = SystemShareService.getInstance()
    this.tapShare = TapShareService.getInstance()
    this.airTransfer = AirTransferService.getInstance()
  }

  // 统一分享入口：按 channel 委托到对应独立后端 service。
  // 成功返回 true，失败返回 false；未知 channel 返回 false，不假装成功。
  async share(payload: SharePayload, channel: ShareChannel): Promise<boolean> {
    if (channel === 'system') {
      return await this.systemShare.share(payload)
    }
    if (channel === 'tap') {
      return await this.tapShare.share(payload)
    }
    if (channel === 'airTransfer') {
      return await this.airTransfer.share(payload)
    }
    // 未知 channel 返回失败，不假装成功。
    return false
  }
}
