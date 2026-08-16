// TapShareService — 碰一碰分享后端。
// 独立类，独立后端：按 HarmonyOS Share Kit 碰一碰官方独立流程。
//
// 关键约束（Issue #629 评论 3 第 6 部分）：
//   碰一碰必须走官方独立流程，**不落 systemShareManager.shareContent**。
//   三个 channel 不允许都落同一个 API。
//
// 本机无 hvigor/ohpm/ShareKit SDK，无法确认官方碰一碰专用 API 的精确签名
// （HarmonyOS 碰一碰通常通过 Share Kit 的碰一碰触发接口或系统级碰一碰服务，
//   而非 systemShareManager.shareContent）。
// 因此本实现为占位骨架：SDK 不可用时返回 false，**不假装成功**。
// 待接入真实 SDK 后，在此替换为官方碰一碰专用 API 调用（仍不得回退到 systemShareManager）。

import { SharePayload } from './SharePayload'

export class TapShareService {
  private static instance: TapShareService

  static getInstance(): TapShareService {
    if (!TapShareService.instance) {
      TapShareService.instance = new TapShareService()
    }
    return TapShareService.instance
  }

  // 碰一碰分享入口。
  // 官方碰一碰 API 需 SDK 确认，此处为独立后端，**不落 systemShareManager**。
  // SDK 不可用时返回 false，不假装成功。
  async share(payload: SharePayload): Promise<boolean> {
    // 碰一碰专用流程；官方 API 需 SDK 确认，此处独立后端不落 systemShareManager。
    // TODO(Issue #629): 接入 Share Kit 碰一碰官方专用 API（非 systemShareManager.shareContent）。
    return false
  }
}
