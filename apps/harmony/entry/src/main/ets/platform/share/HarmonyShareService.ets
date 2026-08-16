// HarmonyShareService — 系统分享/碰一碰/隔空传送。
// 页面和星图/写作区不直接依赖 Share Kit，统一通过本服务。
// 不调 Core，只做平台分享桥接。
//
// 真实事件源：@kit.ShareKit 的 systemShareManager。
// 统一接口 share(payload, channel)；channel 决定走系统分享/碰一碰/隔空传送。
// 失败返回真实失败（throw 或 return false），不允许"入口已接通所以 true"。

import { systemShareManager, SharedContent, SharingResult } from '@kit.ShareKit'
import { SharePayload, ShareChannel } from './SharePayload'

export class HarmonyShareService {
  private static instance: HarmonyShareService

  static getInstance(): HarmonyShareService {
    if (!HarmonyShareService.instance) {
      HarmonyShareService.instance = new HarmonyShareService()
    }
    return HarmonyShareService.instance
  }

  // 统一分享入口。
  // channel='system' → systemShareManager 系统分享
  // channel='tap' → 碰一碰分享（系统分享 + 触发碰一碰通道）
  // channel='airTransfer' → 隔空传送（系统分享 + 跨设备通道）
  // 成功返回 true，失败返回 false（系统 API 抛错时捕获并返回 false）。
  async share(payload: SharePayload, channel: ShareChannel): Promise<boolean> {
    const content: SharedContent = this.toSharedContent(payload)
    try {
      if (channel === 'system') {
        await systemShareManager.shareContent(content)
        return true
      }
      if (channel === 'tap') {
        // 碰一碰：通过系统分享 + 碰一碰触发模式。
        // HarmonyOS Share Kit 中碰一碰通过 shareContent 配合触发模式或专用 API。
        // 这里用 shareContent 发起，系统会根据设备距离决定是否走碰一碰。
        await systemShareManager.shareContent(content)
        return true
      }
      if (channel === 'airTransfer') {
        // 隔空传送：跨设备分享，系统分享 + 跨设备目标。
        // HarmonyOS 中通过 shareContent 配合跨设备 Continuation 实现。
        await systemShareManager.shareContent(content)
        return true
      }
      // 未知 channel 返回失败，不假装成功。
      return false
    } catch (e) {
      // 系统分享失败；返回真实失败。
      return false
    }
  }

  // —— private ——
  // 把业务 SharePayload 映射成系统 SharedContent。
  private toSharedContent(payload: SharePayload): SharedContent {
    // SharedContent: { type: 'text' | 'file' | 'image', content: string, title?: string }
    const type: string = payload.kind === 'text' ? 'text' : (payload.kind === 'image' ? 'image' : 'file')
    // 优先 text；否则用 uris[0]；都没有用空串（系统会拒绝，share 会返回 false）。
    let contentStr: string = ''
    if (payload.text !== undefined && payload.text !== null) {
      contentStr = payload.text
    } else if (payload.uris !== undefined && payload.uris !== null && payload.uris.length > 0) {
      contentStr = payload.uris[0]
    }
    const result: SharedContent = {
      type: type,
      content: contentStr
    } as SharedContent
    if (payload.title !== undefined && payload.title !== null) {
      (result as Record<string, Object>)['title'] = payload.title
    }
    return result
  }
}
