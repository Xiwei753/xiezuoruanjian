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
  // 与原 AirTransferService.buildSharedData 同规格：
  // title 非空 addText，text 非空 addText，uris 非空过滤空串 addURI，都空返回 null。
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

    if (payload.title !== undefined && payload.title !== null && payload.title.length > 0) {
      records.push({ utd: 'general.text', content: payload.title })
    }
    if (hasText) {
      records.push({ utd: 'general.text', content: payload.text })
    }
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
