// HarmonyShareService — 系统分享/碰一碰/隔空传送的统一调度入口。
// 页面和星图/写作区不直接依赖 Share Kit，统一通过本服务。
// 不调 Core，只做平台分享桥接。
//
// 设计（Issue #629 评论 5306513458 问题 3）：
//   本类只调度和配置 payload/lifecycle，不直接构造 SharedData，也不直接调系统 API。
//   三个 channel 各自走独立后端 service：
//     system      委托 SystemShareService.share(payload)             （一次性，ShareController.show）
//     tap         委托 TapShareService 的 start/updatePayload/stop   （生命周期，knockShare 事件）
//     airTransfer 委托 AirTransferService 的 start/updatePayload/stop（生命周期，gesturesShare 事件）
//   三个 channel 不允许都落同一个 API。
//
// 一次性 share(payload, channel) 仅对 system 有效；
// tap/airTransfer 是生命周期通道，调用方应走 startTapSharing/startAirTransferSharing。
// 对 tap/airTransfer 调一次性 share 返回 false（不假装成功，引导调用方用生命周期接口）。
//
// UIAbilityContext 由 AppContext.setAbilityContext() 注入；本类转发给 SystemShareService，
// 并自己持有用于获取 windowId（隔空传送需要 windowId，从 window.getLastWindow 统一取得，
// 不让页面自己查 Window）。

import { common } from '@kit.AbilityKit'
import { window } from '@kit.ArkUI'
import { SharePayload, ShareChannel } from './SharePayload'
import { SystemShareService } from './SystemShareService'
import { TapShareService } from './TapShareService'
import { AirTransferService } from './AirTransferService'

export class HarmonyShareService {
  private static instance: HarmonyShareService

  private readonly systemShare: SystemShareService
  private readonly tapShare: TapShareService
  private readonly airTransfer: AirTransferService

  // UIAbilityContext，由 AppContext 注入。用于获取 windowId（隔空传送）。
  private abilityContext: common.UIAbilityContext | null = null

  static getInstance(): HarmonyShareService {
    if (!HarmonyShareService.instance) {
      HarmonyShareService.instance = new HarmonyShareService()
    }
    return HarmonyShareService.instance
  }

  private constructor() {
    this.systemShare = SystemShareService.getInstance()
    this.tapShare = TapShareService.getInstance()
    this.airTransfer = AirTransferService.getInstance()
  }

  // 由 AppContext.setAbilityContext() 调用：转发给 SystemShareService，自己持有用于 getWindowId。
  setAbilityContext(context: common.UIAbilityContext): void {
    this.abilityContext = context
    this.systemShare.setAbilityContext(context)
  }

  // 一次性分享入口：仅 system 通道有效。
  // tap/airTransfer 是生命周期通道，调用方应走 startTapSharing/startAirTransferSharing；
  // 此处对它们返回 false，不假装成功。未知 channel 返回 false。
  async share(payload: SharePayload, channel: ShareChannel): Promise<boolean> {
    if (channel === 'system') {
      return await this.systemShare.share(payload)
    }
    // tap/airTransfer 不能一次性 share；返回 false 引导调用方用生命周期接口。
    return false
  }

  // tap 生命周期
  // 进入可碰一碰分享页面：先 updatePayload 再 start。
  startTapSharing(payload: SharePayload): void {
    this.tapShare.updatePayload(payload)
    this.tapShare.start()
  }

  // 更新当前碰一碰 payload（页面内容变化时）。
  updateTapPayload(payload: SharePayload | null): void {
    this.tapShare.updatePayload(payload)
  }

  // 离开碰一碰可分享页面/退后台：成对 stop。
  stopTapSharing(): void {
    this.tapShare.stop()
  }

  // airTransfer 生命周期
  // 进入可隔空传送页面：先 updatePayload 再 start(windowId)。
  // windowId 可选：未提供时从 abilityContext + window.getLastWindow 统一获取。
  // 返回 true 表示成功注册；false 表示无 context/无 window/注册失败（不假装成功）。
  async startAirTransferSharing(payload: SharePayload, windowId?: number): Promise<boolean> {
    this.airTransfer.updatePayload(payload)
    let wid: number
    if (windowId !== undefined && windowId !== null) {
      wid = windowId
    } else {
      const resolved: number | null = await this.getWindowId()
      if (resolved === null) {
        // 无可用 windowId；不注册，不假装成功。
        return false
      }
      wid = resolved
    }
    this.airTransfer.start(wid)
    return true
  }

  // 更新当前隔空传送 payload。
  updateAirTransferPayload(payload: SharePayload | null): void {
    this.airTransfer.updatePayload(payload)
  }

  // 离开隔空传送页面/退后台：成对 stop。
  // windowId 可选：未提供时用 AirTransferService 内部记下的 activeWindowId 成对 off。
  async stopAirTransferSharing(windowId?: number): Promise<void> {
    if (windowId !== undefined && windowId !== null) {
      this.airTransfer.stop(windowId)
    } else {
      // 用 -1 哨兵；AirTransferService 内部以 activeWindowId 为准成对 off。
      this.airTransfer.stop(-1)
    }
  }

  // private
  // 从 abilityContext + window.getLastWindow 统一获取 windowId。
  // 不让页面自己查 Window；本方法是 windowId 的唯一获取点。
  // 失败返回 null（无 context/无 window/API 抛错）。
  private async getWindowId(): Promise<number | null> {
    if (this.abilityContext === null) {
      return null
    }
    try {
      const win: window.Window = await window.getLastWindow(this.abilityContext)
      // HarmonyOS window.Window.getWindowInstanceId() 返回窗口实例 id。
      const id: number = win.getWindowInstanceId()
      return id
    } catch (e) {
      return null
    }
  }
}
