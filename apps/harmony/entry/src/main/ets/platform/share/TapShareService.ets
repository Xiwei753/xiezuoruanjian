// TapShareService — 碰一碰分享后端。
// 独立类，独立后端：按 HarmonyOS Share Kit 碰一碰官方 harmonyShare.on('knockShare') 流程。
//
// 关键约束（Issue #629 评论 5306513458 问题 3）：
//   碰一碰不是用户点按钮后"主动发起"，而是先注册当前页面可分享内容，
//   等系统碰一碰事件触发。所以本 service 是生命周期接口（start/updatePayload/stop），
//   不是一次性的 share(payload): Promise<boolean>。
//
// 真实事件源经 KnockShareApi12 facade 调 @kit.ShareKit 的 harmonyShare
// （knockShare 基础重载 since 5.0.0(12)）。Service 不直接 import @kit.ShareKit，
// 系统调用和数据构造都在 impl/api12 facade。
//
// 三个 channel 不允许都落同一个 API：本类只走 SharableTarget.share()，
// 不走 systemShare.ShareController.show（那是 SystemShareService 的路径）。

import { KnockShareApi12 } from './impl/api12/KnockShareApi12'
import { SharePayload } from './SharePayload'

export class TapShareService {
  private static instance: TapShareService

  // 当前持有的可分享 payload。由 updatePayload 更新；start 注册的 callback 经 facade 读此字段。
  private currentPayload: SharePayload | null = null
  // 是否已 start（避免重复注册 / 未注册就 stop）。
  private listening: boolean = false
  // 碰一碰能力缓存：start() 注册成功即 true。CapabilityRegistry 从此读。
  private supported: boolean = false

  static getInstance(): TapShareService {
    if (!TapShareService.instance) {
      TapShareService.instance = new TapShareService()
    }
    return TapShareService.instance
  }

  // 碰一碰能力：未监听时查询 facade 真实能力；监听后返回缓存。
  // CapabilityRegistry 汇总时读此方法。
  isSupported(): boolean {
    if (!this.listening) {
      this.supported = KnockShareApi12.getInstance().isSupported()
    }
    return this.supported
  }

  // 进入可分享页面/状态时调用：经 KnockShareApi12 facade 注册 harmonyShare.on('knockShare', ...)。
  // 系统碰一碰事件触发时，facade callback 拿到 SharableTarget，用 currentPayload 构造 SharedData 并 share。
  // 若当前 payload 为空，facade callback 内跳过（不假装成功）。
  // 重复 start 是幂等的：已 listening 时直接返回 true，不重复注册。
  // 返回 boolean 真实反映注册结果（Issue #629 评论5 第7部分）：成功 true，失败 false。
  start(): boolean {
    if (this.listening) {
      // 已在监听；不重复注册，避免泄漏。返回当前真实 listening 状态。
      return true
    }
    const api: KnockShareApi12 = KnockShareApi12.getInstance()
    // 先查 facade 真实能力；不支持时不尝试注册，不伪造状态。
    if (!api.isSupported()) {
      this.listening = false
      this.supported = false
      return false
    }
    // 以 startListening() 的真实返回值为准，不因 isSupported 就伪造已启动。
    const ok: boolean = api.startListening((): SharePayload | null => {
      return this.currentPayload
    })
    if (ok) {
      this.listening = true
      this.supported = true
      return true
    }
    // 注册失败；不假装 listening/supported。
    this.listening = false
    this.supported = false
    return false
  }

  // 更新当前持有的 payload。可在 start 之后、碰一碰触发之前多次调用。
  // payload 为 null/undefined 时清空（碰一碰触发时不分享）。
  updatePayload(payload: SharePayload | null): void {
    if (payload === undefined || payload === null) {
      this.currentPayload = null
      return
    }
    this.currentPayload = payload
  }

  // 离开页面/应用退后台时调用：经 KnockShareApi12 facade 成对 harmonyShare.off('knockShare', callback)。
  // 未 start 时 stop 是幂等的：直接返回。
  stop(): void {
    if (!this.listening) {
      // 未在监听；不调用 off，避免无对应 on。
      return
    }
    KnockShareApi12.getInstance().stopListening()
    this.listening = false
  }
}
