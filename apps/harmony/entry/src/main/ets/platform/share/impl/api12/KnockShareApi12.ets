// KnockShareApi12 — 碰一碰分享 API12 facade（单例）。
// 直接 import @kit.ShareKit 的 harmonyShare/systemShare，把系统调用收进 impl/api12。
// 版本敏感的 @kit.* 系统调用收进 impl/apiXX；稳定 Service 不直接 import 版本敏感的高版本能力 Kit（@kit.AbilityKit 的 common 等基础平台类型除外）。
//
// 真实事件源：harmonyShare.on('knockShare', callback)（无 capability 参数的基础重载，since 5.0.0(12)）。
// 工程 compatibleSdkVersion=12，knockShare 基础重载在 API12 即可用，不是高版本能力；
// 收进 impl/api12 是为满足"稳定层不直接 import 版本敏感的高版本能力 Kit、不自行判断 API Level"的边界一致性。
// 注册失败返回 false，不伪造状态（callback 不触发）。
import { harmonyShare, systemShare } from '@kit.ShareKit'
import { PlatformApiResolver } from '../../../version/PlatformApiResolver'
import { SharePayload } from '../../SharePayload'

export class KnockShareApi12 {
  private static instance: KnockShareApi12

  // startListening 注册的 callback 引用；stopListening 用同一引用 off，保证成对。
  private knockCallback: ((target: harmonyShare.SharableTarget) => void) | null = null

  static getInstance(): KnockShareApi12 {
    if (!KnockShareApi12.instance) {
      KnockShareApi12.instance = new KnockShareApi12()
    }
    return KnockShareApi12.instance
  }

  // 查询系统能力是否支持碰一碰分享。
  // 需同时满足 API>=12 和 SystemCapability.Collaboration.HarmonyShare，经 PlatformApiResolver 统一。
  isSupported(): boolean {
    const r: PlatformApiResolver = PlatformApiResolver.getInstance()
    return r.isApiAtLeast(12) && r.hasSystemCapability('SystemCapability.Collaboration.HarmonyShare')
  }

  // 启动 harmonyShare.on('knockShare', callback) 监听（无 capability 基础重载，since 5.0.0(12)）。
  // 系统碰一碰事件触发时，callback 拿到 SharableTarget，用 getPayload() 构造 SharedData 并 share。
  // payload 为空或构造失败时跳过（不假装成功）。target.share 失败在 callback 内吞错。
  // 返回真实注册结果：只有 harmonyShare.on 真正注册成功才返回 true，失败返回 false。
  startListening(getPayload: () => SharePayload | null): boolean {
    const cb = (target: harmonyShare.SharableTarget): void => {
      const payload: SharePayload | null = getPayload()
      if (payload === null) {
        // 无 payload；碰一碰触发时不分享，不假装成功。
        return
      }
      const sharedData: systemShare.SharedData | null = this.buildSharedData(payload)
      if (sharedData === null) {
        return
      }
      // 调用官方 SharableTarget.share()；失败不抛（callback 内吞错，不假装成功）。
      try {
        target.share(sharedData)
      } catch (e) {
        // 系统调用失败；忽略，不假装成功。
      }
    }
    this.knockCallback = cb
    try {
      harmonyShare.on('knockShare', cb)
      return true
    } catch (e) {
      // 注册失败；清空 callback 引用，不伪造状态。
      this.knockCallback = null
      return false
    }
  }

  // 停止 harmonyShare 监听。用 startListening 时保存的同一 callback 引用 off，保证成对。
  // off 失败不阻塞；未注册或已停止时 knockCallback 为 null，跳过。
  stopListening(): void {
    const cb = this.knockCallback
    if (cb !== null) {
      try {
        harmonyShare.off('knockShare', cb)
      } catch (e) {
        // off 失败；忽略，仍清理引用。
      }
    }
    this.knockCallback = null
  }

  // —— private ——
  // 把业务 SharePayload 构造成 systemShare.SharedData。
  // 无非空内容时返回 null。与 SystemShareApi11.buildSharedData 同规格
  // （数据构造是共享工具，分享触发 API 不同：本类走 SharableTarget.share）。
  //
  // 按 payload.kind 选主记录（Issue #766）：
  //   kind='file'：第一条必须是 general.file-uri，title 写进该 record 的 title 字段；
  //                其余 URI addRecord；text 在文件记录之后。
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
        utd: 'general.file-uri',
        uri: validUris[0],
        title: titleValue
      }
      const data: systemShare.SharedData = new systemShare.SharedData(primary)
      for (let i = 1; i < validUris.length; i++) {
        data.addRecord({ utd: 'general.file-uri', uri: validUris[i], title: titleValue })
      }
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
    for (const uri of validUris) {
      data.addRecord({ utd: 'general.file-uri', uri: uri })
    }
    return data
  }
}
