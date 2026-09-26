// SystemShareApi11 — 普通系统分享 API11 基础 facade（单例）。
// 直接 import @kit.ShareKit 的 systemShare，完成 API11+ 基础能力：
// SharedData 构造、ShareController.show 调起系统分享面板。
// 版本敏感的 @kit.* 系统调用收进 impl/apiXX；稳定 Service 不直接 import 版本敏感的高版本能力 Kit（@kit.AbilityKit 的 common 等基础平台类型除外）。
//
// 真实接口（@hms.collaboration.systemShare d.ts）：
//   systemShare.SharedData          since 4.1.0(11)
//   systemShare.ShareController     since 4.1.0(11)
//   ShareController.show(context)   since 4.1.0(11)
// 工程 compatibleSdkVersion=12，API11 < 12，API12+ 设备上天然可用。
import { systemShare } from '@kit.ShareKit'
import { common } from '@kit.AbilityKit'
import { BusinessError } from '@ohos.base'
import hilog from '@ohos.hilog'
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
      const err = e as BusinessError
      hilog.error(0x0000, 'SystemShareApi11', 'share failed: code=%{public}d message=%{public}s', err.code, err.message)
      return false
    }
  }

  // —— private ——
  // 把业务 SharePayload 构造成 systemShare.SharedData。
  // 无非空内容时返回 null（调用方返回 false，不假装成功）。
  // 该映射仅服务于普通系统分享后端；碰一碰 / 隔空传送各自独立处理，不走本方法。
  //
  // 按 payload.kind 选主记录（Issue #766）：
  //   kind='file'：第一条必须是 general.file-uri，title 写进该 record 的 title 字段；
  //                其余 URI addRecord；text 在文件记录之后，不允许抢第一条。
  //   kind='text'（及其他）：第一条是 general.text，title 写进同一条 record 的 title 字段；
  //                不再把 title 伪装成独立正文记录。
  //
  // SDK 真实接口（@hms.collaboration.systemShare d.ts，since 4.1.0(11)）：
  //   SharedData 构造函数必传首个 SharedRecord；后续记录用 addRecord 追加。
  //   SharedRecord 有 title?: string 元数据字段（非正文内容）。
  //   无 addText/addURI 方法；文本记录 utd='general.text' + content，URI 记录 utd='general.file-uri' + uri。
  private buildSharedData(payload: SharePayload): systemShare.SharedData | null {
    const hasText: boolean = payload !== undefined && payload !== null &&
      payload.text !== undefined && payload.text !== null && payload.text.length > 0
    const hasTitle: boolean = payload !== undefined && payload !== null &&
      payload.title !== undefined && payload.title !== null && payload.title.length > 0
    const titleValue: string | undefined = hasTitle ? payload.title : undefined

    // 过滤有效 URI（非空串）。
    const validUris: string[] = []
    if (payload !== undefined && payload !== null &&
      payload.uris !== undefined && payload.uris !== null) {
      for (const uri of payload.uris) {
        if (uri !== undefined && uri !== null && uri.length > 0) {
          validUris.push(uri)
        }
      }
    }

    if (!hasText && validUris.length === 0) {
      return null
    }

    if (payload !== undefined && payload !== null && payload.kind === 'file' && validUris.length > 0) {
      // 文件分享：第一条必须是 general.file-uri，带 title 元数据。
      const primary: systemShare.SharedRecord = {
        utd: payload.utd ?? 'general.file-uri',
        uri: validUris[0],
        title: titleValue
      }
      const data: systemShare.SharedData = new systemShare.SharedData(primary)
      // 其余文件 URI 追加。
      for (let i = 1; i < validUris.length; i++) {
        data.addRecord({ utd: payload.utd ?? 'general.file-uri', uri: validUris[i], title: titleValue })
      }
      // 正文（如果存在）放到文件记录之后，不允许抢第一条。
      if (hasText) {
        data.addRecord({ utd: 'general.text', content: payload.text })
      }
      return data
    }

    // 纯文本分享：第一条是 general.text，标题写进同一条 record 的 title 字段。
    const primary: systemShare.SharedRecord = {
      utd: 'general.text',
      content: hasText ? payload.text : '',
      title: titleValue
    }
    const data: systemShare.SharedData = new systemShare.SharedData(primary)
    // 非 file kind 但携带了 URI（如 image），追加 file-uri 记录。
    for (const uri of validUris) {
      data.addRecord({ utd: 'general.file-uri', uri: uri })
    }
    return data
  }
}
