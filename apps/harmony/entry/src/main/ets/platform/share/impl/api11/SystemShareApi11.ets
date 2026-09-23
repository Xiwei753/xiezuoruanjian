// SystemShareApi11 — 普通系统分享 API11 基础 facade（单例）。
// 直接 import @kit.ShareKit 的 systemShare，完成 API11+ 基础能力：
// SharedData 构造、ShareController.show 调起系统分享面板。
// 只有 impl/apiXX 文件可以 import @kit.*；Service 不直接 import @kit.*。
//
// 真实接口（@hms.collaboration.systemShare d.ts）：
//   systemShare.SharedData          since 4.1.0(11)
//   systemShare.ShareController     since 4.1.0(11)
//   ShareController.show(context)   since 4.1.0(11)
// 工程 compatibleSdkVersion=12，API11 < 12，API12+ 设备上天然可用。
import { systemShare } from '@kit.ShareKit'
import { common } from '@kit.AbilityKit'
import { PlatformApiResolver } from '../../../version/PlatformApiResolver'
import { SharePayload } from '../../SharePayload'

export class SystemShareApi11 {
  private static instance: SystemShareApi11

  static getInstance(): SystemShareApi11 {
    if (!SystemShareApi11.instance) {
      SystemShareApi11.instance = new SystemShareApi11()
    }
    return SystemShareApi11.instance
  }

  // 查询系统能力是否支持普通系统分享。
  // 需同时满足 API>=11 和 SystemCapability.Collaboration.SystemShare，经 PlatformApiResolver 统一。
  isSupported(): boolean {
    const r: PlatformApiResolver = PlatformApiResolver.getInstance()
    return r.isApiAtLeast(11) && r.hasSystemCapability('SystemCapability.Collaboration.SystemShare')
  }

  // 系统分享入口：构造 SharedData → 创建 ShareController → controller.show(context, options)。
  // 成功返回 true，失败返回 false（系统 API 抛错或无内容时返回 false）。
  // d.ts 签名 show(context, options)，options 必填；ShareControllerOptions 所有字段可选，
  // 传 {} 即默认配置（单选、卡片预览、居中弹板），与原 SystemShareService 行为一致。
  async share(context: common.UIAbilityContext, payload: SharePayload): Promise<boolean> {
    const sharedData: systemShare.SharedData | null = this.buildSharedData(payload)
    if (sharedData === null) {
      // payload 无非空内容；不触发系统分享面板。
      return false
    }

    try {
      const controller: systemShare.ShareController = new systemShare.ShareController(sharedData)
      await controller.show(context, {})
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
  // 规格与既有 SystemShareService.buildSharedData 完全一致（原样搬过来）。
  //
  // SDK 真实接口（@hms.collaboration.systemShare d.ts，since 4.1.0(11)）：
  //   SharedData 构造函数必传首个 SharedRecord；后续记录用 addRecord 追加。
  //   无 addText/addURI 方法；文本记录 utd='general.text' + content，URI 记录 utd='general.file-uri' + uri。
  private buildSharedData(payload: SharePayload): systemShare.SharedData | null {
    const hasText: boolean = payload !== undefined && payload !== null &&
      payload.text !== undefined && payload.text !== null && payload.text.length > 0
    const hasUris: boolean = payload !== undefined && payload !== null &&
      payload.uris !== undefined && payload.uris !== null && payload.uris.length > 0
    if (!hasText && !hasUris) {
      return null
    }

    // 收集所有 SharedRecord；首个传 SharedData 构造函数，其余 addRecord 追加。
    const records: systemShare.SharedRecord[] = []

    // 标题（可选）。作为独立文本项加入（若 API 有专用 setTitle，可在此替换）。
    if (payload.title !== undefined && payload.title !== null && payload.title.length > 0) {
      records.push({ utd: 'general.text', content: payload.title })
    }

    // 正文文本。
    if (hasText) {
      records.push({ utd: 'general.text', content: payload.text })
    }

    // URI 列表（文件/图片）。过滤空串。
    if (payload.uris !== undefined && payload.uris !== null) {
      for (const uri of payload.uris) {
        if (uri !== undefined && uri !== null && uri.length > 0) {
          records.push({ utd: 'general.file-uri', uri: uri })
        }
      }
    }

    // records 非空由 hasText/hasUris 守卫保证；SharedData 构造函数要求至少一条记录。
    const data: systemShare.SharedData = new systemShare.SharedData(records[0])
    for (let i = 1; i < records.length; i++) {
      data.addRecord(records[i])
    }

    return data
  }
}
