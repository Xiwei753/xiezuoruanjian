// SystemShareService — 普通系统分享后端。
// 独立类，独立后端：只走 @kit.ShareKit 的 systemShareManager.shareContent。
// 不负责碰一碰 / 隔空传送；那两个有各自独立的 service，不落本类。
// 失败返回 false，不假装成功。

import { systemShareManager, SharedContent } from '@kit.ShareKit'
import { SharePayload } from './SharePayload'

export class SystemShareService {
  private static instance: SystemShareService

  static getInstance(): SystemShareService {
    if (!SystemShareService.instance) {
      SystemShareService.instance = new SystemShareService()
    }
    return SystemShareService.instance
  }

  // 系统分享入口：直接调 systemShareManager.shareContent。
  // 成功返回 true，失败返回 false（系统 API 抛错时捕获并返回 false）。
  async share(payload: SharePayload): Promise<boolean> {
    const content: SharedContent = this.toSharedContent(payload)
    try {
      await systemShareManager.shareContent(content)
      return true
    } catch (e) {
      // 系统分享失败；返回真实失败。
      return false
    }
  }

  // —— private ——
  // 把业务 SharePayload 映射成系统 SharedContent。
  // 该映射仅服务于普通系统分享后端；碰一碰 / 隔空传送各自独立处理，不走本映射。
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
