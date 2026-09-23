// KnockShareApi12 — 碰一碰分享 API12 facade（单例）。
// 直接 import @kit.ShareKit 的 harmonyShare/systemShare，把系统调用收进 impl/api12。
// 只有 impl/apiXX 文件可以 import @kit.*；Service 不直接 import @kit.*。
//
// 真实事件源：harmonyShare.on('knockShare', callback)（无 capability 参数的基础重载，since 5.0.0(12)）。
// 工程 compatibleSdkVersion=12，knockShare 基础重载在 API12 即可用，不是高版本能力；
// 收进 impl/api12 是为满足"稳定层不 import @kit.*、不认识 API Level"的边界一致性。
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
  // 无非空内容时返回 null。与 SystemShareService.buildSharedData 同规格
  // （数据构造是共享工具，分享触发 API 不同：本类走 SharableTarget.share）。
  private buildSharedData(payload: SharePayload): systemShare.SharedData | null {
    const hasText: boolean = payload !== undefined && payload !== null &&
      payload.text !== undefined && payload.text !== null && payload.text.length > 0
    const hasUris: boolean = payload !== undefined && payload !== null &&
      payload.uris !== undefined && payload.uris !== null && payload.uris.length > 0
    if (!hasText && !hasUris) {
      return null
    }

    const data: systemShare.SharedData = new systemShare.SharedData()

    if (payload.title !== undefined && payload.title !== null && payload.title.length > 0) {
      data.addText(payload.title)
    }
    if (hasText) {
      data.addText(payload.text)
    }
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
