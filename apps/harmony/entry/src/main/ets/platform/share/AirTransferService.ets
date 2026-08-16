// AirTransferService — 隔空传送分享后端。
// 独立类，独立后端：按 HarmonyOS Share Kit 隔空传送官方 harmonyShare.on('gesturesShare') 流程。
//
// 关键约束（Issue #629 评论 5306513458 问题 3）：
//   隔空传送同样是"先注册可分享内容，等系统隔空手势触发"，不是用户点按钮主动发起。
//   所以本 service 是生命周期接口（start(windowId)/updatePayload/stop(windowId)）。
//   windowId 由外部注入（WindowPrivacyService/AppContext 统一获取），本类不自己查 Window。
//
// 官方流程：
//   import { harmonyShare, systemShare } from '@kit.ShareKit'
//   harmonyShare.on('gesturesShare', { windowId }, (sharableTarget) => {
//     const data = buildSharedData(currentPayload)
//     sharableTarget.share(data)
//   })
//   // 离开页面/退后台：
//   harmonyShare.off('gesturesShare', { windowId })
//
// 三个 channel 不允许都落同一个 API：本类只走 SharableTarget.share()（gesturesShare 事件），
// 与 TapShareService 同走 SharableTarget.share() 但事件源不同（gesturesShare vs knockShare），
// 与 SystemShareService 走 ShareController.show() 不同。

import { harmonyShare, systemShare } from '@kit.ShareKit'
import { SharePayload } from './SharePayload'

export class AirTransferService {
  private static instance: AirTransferService

  // 当前持有的可分享 payload。
  private currentPayload: SharePayload | null = null
  // 已注册的 gesturesShare callback 引用；stop 时用同一引用 off，保证成对。
  private gesturesCallback: ((target: harmonyShare.SharableTarget) => void) | null = null
  // 当前注册的 windowId（start 时记下，stop 时校验同一 windowId）。
  private activeWindowId: number = -1
  // 是否已 start。
  private listening: boolean = false

  static getInstance(): AirTransferService {
    if (!AirTransferService.instance) {
      AirTransferService.instance = new AirTransferService()
    }
    return AirTransferService.instance
  }

  // 进入可隔空传送页面/状态时调用：注册 harmonyShare.on('gesturesShare', { windowId }, callback)。
  // windowId 由外部注入（WindowPrivacyService/AppContext 统一获取），本类不自己查 Window。
  // 重复 start 是幂等的：已 listening 且 windowId 一致时直接返回。
  start(windowId: number): void {
    if (this.listening) {
      // 已在监听。若 windowId 一致则幂等返回；若不一致则先 stop 旧的再注册新的。
      if (this.activeWindowId === windowId) {
        return
      }
      this.stopInternal()
    }
    const cb = (target: harmonyShare.SharableTarget): void => {
      const payload: SharePayload | null = this.currentPayload
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
      this.listening = true
      this.activeWindowId = windowId
    } catch (e) {
      // 注册失败；清空引用，不假装 listening。
      this.gesturesCallback = null
      this.listening = false
      this.activeWindowId = -1
    }
  }

  // 更新当前持有的 payload。可在 start 之后、手势触发之前多次调用。
  updatePayload(payload: SharePayload | null): void {
    if (payload === undefined || payload === null) {
      this.currentPayload = null
      return
    }
    this.currentPayload = payload
  }

  // 离开页面/退后台时调用：成对 harmonyShare.off('gesturesShare', { windowId })。
  // windowId 应与 start 时一致；不一致时仍尝试 off 当前 activeWindowId 以保证成对清理。
  stop(windowId: number): void {
    if (!this.listening) {
      // 未在监听；不调用 off。
      this.gesturesCallback = null
      this.activeWindowId = -1
      return
    }
    // 用 start 时记下的 activeWindowId 成对 off（保证 on/off 配对）。
    const offWindowId: number = this.activeWindowId
    const cb = this.gesturesCallback
    try {
      harmonyShare.off('gesturesShare', { windowId: offWindowId }, cb)
    } catch (e) {
      // off 失败；忽略，仍清理本地状态。
    }
    this.gesturesCallback = null
    this.listening = false
    this.activeWindowId = -1
    // windowId 参数用于调用方表达意图；本类以 activeWindowId 为准保证成对。
    void windowId
  }

  // —— private ——
  private stopInternal(): void {
    if (!this.listening) {
      return
    }
    const offWindowId: number = this.activeWindowId
    const cb = this.gesturesCallback
    try {
      harmonyShare.off('gesturesShare', { windowId: offWindowId }, cb)
    } catch (e) {
      // 忽略。
    }
    this.gesturesCallback = null
    this.listening = false
    this.activeWindowId = -1
  }

  // 把业务 SharePayload 构造成 systemShare.SharedData。无内容返回 null。
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
