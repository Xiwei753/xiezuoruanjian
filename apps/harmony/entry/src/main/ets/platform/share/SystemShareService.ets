// SystemShareService — 普通系统分享后端。
// 独立类，独立后端：只走 @kit.ShareKit 的 systemShare.SharedData + systemShare.ShareController.show。
// 不负责碰一碰 / 隔空传送；那两个有各自独立的 service，不落本类。
// 失败返回 false，不假装成功。
//
// 官方接口（Issue #629 评论 5306513458 问题 3）：
//   import { systemShare } from '@kit.ShareKit'
//   const data = new systemShare.SharedData()
//   data.addText(...) / data.addURI(...)
//   const controller = new systemShare.ShareController(data)
//   await controller.show(uiAbilityContext)
//
// UIAbilityContext 由 AppContext.setAbilityContext() 注入本 service，
// feature/page 不直接 import Share Kit。

import { systemShare } from '@kit.ShareKit'
import { common } from '@kit.AbilityKit'
import { SharePayload } from './SharePayload'

export class SystemShareService {
  private static instance: SystemShareService

  // UIAbilityContext，由 AppContext 注入。无 context 时 share 返回 false，不假装成功。
  private abilityContext: common.UIAbilityContext | null = null
  // 系统分享能力缓存：share() 成功过一次即置 true。CapabilityRegistry 从此读，不自己探测 Share Kit API。
  private supported: boolean = false

  static getInstance(): SystemShareService {
    if (!SystemShareService.instance) {
      SystemShareService.instance = new SystemShareService()
    }
    return SystemShareService.instance
  }

  // 由 AppContext.setAbilityContext() 调用，注入 UIAbilityContext。
  // 不持有 Activity/Window 引用以外的平台状态；context 仅用于 controller.show()。
  setAbilityContext(context: common.UIAbilityContext): void {
    this.abilityContext = context
  }

  // 系统分享能力：share() 成功过即 true。CapabilityRegistry 汇总时读此方法。
  isSupported(): boolean {
    return this.supported
  }

  // 系统分享入口：构造 SharedData → 创建 ShareController → controller.show(context)。
  // 成功返回 true，失败返回 false（系统 API 抛错或无 context/无内容时返回 false）。
  async share(payload: SharePayload): Promise<boolean> {
    if (this.abilityContext === null) {
      // 未注入 UIAbilityContext；返回真实失败，不假装成功。
      return false
    }

    const sharedData: systemShare.SharedData | null = this.buildSharedData(payload)
    if (sharedData === null) {
      // payload 无非空内容；不触发系统分享面板。
      return false
    }

    try {
      const controller: systemShare.ShareController = new systemShare.ShareController(sharedData)
      await controller.show(this.abilityContext)
      this.supported = true
      return true
    } catch (e) {
      // 系统分享失败；返回真实失败。
      return false
    }
  }

  // —— private ——
  // 把业务 SharePayload 构造成 systemShare.SharedData。
  // 无非空内容时返回 null（调用方返回 false，不假装成功）。
  // 该映射仅服务于普通系统分享后端；碰一碰 / 隔空传送各自独立处理，不走本方法。
  private buildSharedData(payload: SharePayload): systemShare.SharedData | null {
    const hasText: boolean = payload !== undefined && payload !== null &&
      payload.text !== undefined && payload.text !== null && payload.text.length > 0
    const hasUris: boolean = payload !== undefined && payload !== null &&
      payload.uris !== undefined && payload.uris !== null && payload.uris.length > 0
    if (!hasText && !hasUris) {
      return null
    }

    const data: systemShare.SharedData = new systemShare.SharedData()

    // 标题（可选）。Share Kit 的 SharedData 支持通过 addText 携带标题摘要，
    // 这里把 title 作为独立文本项加入（若 API 有专用 setTitle，可在此替换）。
    if (payload.title !== undefined && payload.title !== null && payload.title.length > 0) {
      data.addText(payload.title)
    }

    // 正文文本。
    if (hasText) {
      data.addText(payload.text)
    }

    // URI 列表（文件/图片）。过滤空串。
    if (payload.uris !== undefined && payload.uris !== null) {
      for (const uri of payload.uris) {
        if (uri !== undefined && uri !== null && uri.length > 0) {
          data.addURI(uri)
        }
      }
    }

    return data
  }
}
