// AirTransferService — 隔空传送分享后端（稳定 facade）。
// 独立类，独立后端：按 HarmonyOS Share Kit 隔空传送官方 harmonyShare.on('gesturesShare') 流程。
//
// 关键约束（Issue #629 评论 5306513458 问题 3）：
//   隔空传送同样是"先注册可分享内容，等系统隔空手势触发"，不是用户点按钮主动发起。
//   所以本 service 是生命周期接口（start(windowId)/updatePayload/stop(windowId)）。
//   windowId 由外部注入（WindowPrivacyService/AppContext 统一获取），本类不自己查 Window。
//
// 版本边界（Issue #742）：gesturesShare 是 @since 6.0.0(20) 高版本能力，工程 compatibleSdkVersion=12。
// 真实事件源经 GesturesShareApi20 facade 调 @kit.ShareKit 的 harmonyShare，本 Service 不直接 import @kit.ShareKit，
// 系统调用和数据构造都在 impl/api20 facade。API12~19 下 isSupported() 返回 false、start() 返回 false，
// 不伪造状态。
//
// 三个 channel 不允许都落同一个 API：本类只走 SharableTarget.share()（gesturesShare 事件），
// 与 TapShareService 同走 SharableTarget.share() 但事件源不同（gesturesShare vs knockShare），
// 与 SystemShareService 走 ShareController.show() 不同。

import { GesturesShareApi20 } from './impl/api20/GesturesShareApi20'
import { SharePayload } from './SharePayload'

export class AirTransferService {
  private static instance: AirTransferService

  // 当前持有的可分享 payload。
  private currentPayload: SharePayload | null = null
  // 当前注册的 windowId（start 时记下，stop 时校验同一 windowId）。
  private activeWindowId: number = -1
  // 是否已 start。
  private listening: boolean = false
  // 隔空传送能力缓存：start() 注册成功即 true。CapabilityRegistry 从此读。
  private supported: boolean = false

  static getInstance(): AirTransferService {
    if (!AirTransferService.instance) {
      AirTransferService.instance = new AirTransferService()
    }
    return AirTransferService.instance
  }

  // 隔空传送能力：未监听时查 impl 真实能力；监听后返回缓存（start 成功过即 true）。
  // CapabilityRegistry 汇总时读此方法。API12~19 设备 impl 返回 false。
  isSupported(): boolean {
    if (!this.listening) {
      this.supported = GesturesShareApi20.getInstance().isSupported()
    }
    return this.supported
  }

  // 进入可隔空传送页面/状态时调用：委托 GesturesShareApi20 注册 harmonyShare.on('gesturesShare', { windowId }, callback)。
  // windowId 由外部注入（WindowPrivacyService/AppContext 统一获取），本类不自己查 Window。
  // 重复 start 是幂等的：已 listening 且 windowId 一致时直接返回 true。
  // 返回 boolean 真实反映注册结果（Issue #629 评论5 第7部分）：成功 true，失败 false。
  // API12~19 设备 impl.isSupported() 返回 false，本方法返回 false（真实 unsupported，不假装）。
  start(windowId: number): boolean {
    if (this.listening) {
      // 已在监听。若 windowId 一致则幂等返回 true；若不一致则先 stop 旧的再注册新的。
      if (this.activeWindowId === windowId) {
        return true
      }
      this.stopInternal()
    }
    // 先查 impl 真实能力；API12~19 unsupported 直接返回 false，不假装。
    if (!GesturesShareApi20.getInstance().isSupported()) {
      this.listening = false
      this.activeWindowId = -1
      this.supported = false
      return false
    }
    const ok: boolean = GesturesShareApi20.getInstance().startListening(windowId, (): SharePayload | null => {
      return this.currentPayload
    })
    if (ok) {
      this.listening = true
      this.activeWindowId = windowId
      this.supported = true
      return true
    }
    // 注册失败；不假装 listening/supported。
    this.listening = false
    this.activeWindowId = -1
    this.supported = false
    return false
  }

  // 更新当前持有的 payload。可在 start 之后、手势触发之前多次调用。
  updatePayload(payload: SharePayload | null): void {
    if (payload === undefined || payload === null) {
      this.currentPayload = null
      return
    }
    this.currentPayload = payload
  }

  // 离开页面/退后台时调用：委托 GesturesShareApi20 成对 harmonyShare.off('gesturesShare', { windowId })。
  // windowId 应与 start 时一致；不一致时仍尝试 off 当前 activeWindowId 以保证成对清理。
  stop(windowId: number): void {
    if (!this.listening) {
      // 未在监听；不调用 off。
      this.activeWindowId = -1
      return
    }
    // 用 start 时记下的 activeWindowId 成对 off（保证 on/off 配对）。
    this.stopInternal()
    // windowId 参数用于调用方表达意图；本类以 activeWindowId 为准保证成对。
    void windowId
  }

  // —— private ——
  private stopInternal(): void {
    if (!this.listening) {
      return
    }
    GesturesShareApi20.getInstance().stopListening(this.activeWindowId)
    this.listening = false
    this.activeWindowId = -1
  }
}
