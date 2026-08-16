// AirTransferService — 隔空传送分享后端。
// 独立类，独立后端：按 HarmonyOS Share Kit 隔空传送官方 sharableTarget.share() 流程。
//
// 关键约束（Issue #629 评论 3 第 6 部分）：
//   隔空传送必须走官方 sharableTarget.share() 独立流程，**不落 systemShareManager.shareContent**。
//   三个 channel 不允许都落同一个 API。
//
// 本机无 hvigor/ohpm/ShareKit SDK，无法确认官方 sharableTarget 的精确获取与调用签名
// （HarmonyOS 隔空传送通常通过 Share Kit 的 sharableTarget 接口或跨设备 Continuation 通道，
//   而非 systemShareManager.shareContent）。
// 因此本实现为占位骨架：SDK 不可用时返回 false，**不假装成功**。
// 待接入真实 SDK 后，在此替换为官方 sharableTarget.share() 调用（仍不得回退到 systemShareManager）。

import { SharePayload } from './SharePayload'

export class AirTransferService {
  private static instance: AirTransferService

  static getInstance(): AirTransferService {
    if (!AirTransferService.instance) {
      AirTransferService.instance = new AirTransferService()
    }
    return AirTransferService.instance
  }

  // 隔空传送分享入口。
  // 官方 sharableTarget.share() 流程；官方 API 需 SDK 确认，**不落 systemShareManager**。
  // SDK 不可用时返回 false，不假装成功。
  async share(payload: SharePayload): Promise<boolean> {
    // sharableTarget.share() 流程；官方 API 需 SDK 确认，不落 systemShareManager。
    // TODO(Issue #629): 接入 Share Kit 隔空传送官方 sharableTarget.share()（非 systemShareManager.shareContent）。
    return false
  }
}
