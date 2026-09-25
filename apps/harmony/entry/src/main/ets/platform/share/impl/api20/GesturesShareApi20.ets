// GesturesShareApi20 — 隔空传送 gesturesShare API20 facade（单例）。
// 直接 import @kit.ShareKit 的 harmonyShare / systemShare，承接 harmonyShare.on/off('gesturesShare')。
// 版本敏感的 @kit.* 系统调用收进 impl/apiXX；稳定 Service 不直接 import 版本敏感的高版本能力 Kit（@kit.AbilityKit 的 common 等基础平台类型除外）。
//
// 真实事件源：harmonyShare.on('gesturesShare', { windowId }, callback)（since 6.0.0(20)）。
// API12~19 设备上 isSupported() 返回 false、startListening() 返回 false（真实 unsupported，不假装）。
// 错误码 801 = 能力不支持；注册失败时不伪造状态（返回 false）。
import { harmonyShare, systemShare } from '@kit.ShareKit'
import { PlatformApiResolver } from '../../../version/PlatformApiResolver'
import { SharePayload } from '../../SharePayload'

export class GesturesShareApi20 {
  private static instance: GesturesShareApi20

  // 已注册的 gesturesShare callback 引用；stop 时用同一引用 off，保证成对。
  private gesturesCallback: ((target: harmonyShare.SharableTarget) => void) | null = null

  static getInstance(): GesturesShareApi20 {
    if (!GesturesShareApi20.instance) {
      GesturesShareApi20.instance = new GesturesShareApi20()
    }
    return GesturesShareApi20.instance
  }

  // 查询系统能力是否支持隔空传送 gesturesShare。
  // 需同时满足 API>=20 和 SystemCapability.Collaboration.HarmonyShare，经 PlatformApiResolver 统一。
  isSupported(): boolean {
    const r: PlatformApiResolver = PlatformApiResolver.getInstance()
    return r.isApiAtLeast(20) && r.hasSystemCapability('SystemCapability.Collaboration.HarmonyShare')
  }

  // 启动 harmonyShare.on('gesturesShare', { windowId }, callback) 监听。
  // callback 内：调 getPayload() 取当前 payload，null 跳过；buildSharedData 构造数据，null 跳过；
  // target.share(sharedData) 失败吞错。
  // 返回真实注册结果：只有 harmonyShare.on 真正注册成功才返回 true，失败返回 false。
  // 注意保存 callback 引用供 stopListening 用。
  startListening(windowId: number, getPayload: () => SharePayload | null): boolean {
    const cb = (target: harmonyShare.SharableTarget): void => {
      const payload: SharePayload | null = getPayload()
      if (payload === null) {
        return
      }
      const sharedData: systemShare.SharedData | null = this.buildSharedData(payload)
      if (sharedData === null) {
        return
      }
      try {
        target.share(sharedData)
      } catch (e) {
        // 系统调用失败；忽略，不假装成功。
      }
    }
    this.gesturesCallback = cb
    try {
      harmonyShare.on('gesturesShare', { windowId: windowId }, cb)
      return true
    } catch (e) {
      // 注册失败；清空引用，不伪造状态。
      this.gesturesCallback = null
      return false
    }
  }

  // 停止 harmonyShare.off('gesturesShare', { windowId }, callback) 监听。
  // 用 startListening 时记下的 callback 引用成对 off。try/catch 吞错。
  stopListening(windowId: number): void {
    const cb = this.gesturesCallback
    try {
      harmonyShare.off('gesturesShare', { windowId: windowId }, cb)
    } catch (e) {
      // off 失败不阻塞。
    }
    this.gesturesCallback = null
  }

  // —— private ——
  // 把业务 SharePayload 构造成 systemShare.SharedData。无内容返回 null。
  // 与 SystemShareApi11.buildSharedData 同规格：
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
