// TapShareService — 碰一碰分享后端。
// 独立类，独立后端：按 HarmonyOS Share Kit 碰一碰官方 harmonyShare.on('knockShare') 流程。
//
// 关键约束（Issue #629 评论 5306513458 问题 3）：
//   碰一碰不是用户点按钮后"主动发起"，而是先注册当前页面可分享内容，
//   等系统碰一碰事件触发。所以本 service 是生命周期接口（start/updatePayload/stop），
//   不是一次性的 share(payload): Promise<boolean>。
//
// 官方流程：
//   import { harmonyShare, systemShare } from '@kit.ShareKit'
//   harmonyShare.on('knockShare', (sharableTarget: harmonyShare.SharableTarget) => {
//     const data = buildSharedData(currentPayload)
//     sharableTarget.share(data)
//   })
//   // 离开页面/退后台：
//   harmonyShare.off('knockShare', callback)
//
// 三个 channel 不允许都落同一个 API：本类只走 SharableTarget.share()，
// 不走 systemShare.ShareController.show（那是 SystemShareService 的路径）。

import { harmonyShare, systemShare } from '@kit.ShareKit'
import { SharePayload } from './SharePayload'

export class TapShareService {
  private static instance: TapShareService

  // 当前持有的可分享 payload。由 updatePayload 更新；start 注册的 callback 读此字段。
  private currentPayload: SharePayload | null = null
  // 已注册的 knockShare callback 引用；stop 时用同一引用 off，保证成对。
  private knockCallback: ((target: harmonyShare.SharableTarget) => void) | null = null
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

  // 碰一碰能力：start() 注册成功过即 true。CapabilityRegistry 汇总时读此方法。
  isSupported(): boolean {
    return this.supported
  }

  // 进入可分享页面/状态时调用：注册 harmonyShare.on('knockShare', ...)。
  // 系统碰一碰事件触发时，callback 拿到 SharableTarget，用 currentPayload 构造 SharedData 并 share。
  // 若当前 payload 为空，callback 内跳过（不假装成功）。
  // 重复 start 是幂等的：已 listening 时直接返回 true，不重复注册。
  // 返回 boolean 真实反映注册结果（Issue #629 评论5 第7部分）：成功 true，失败 false。
  start(): boolean {
    if (this.listening) {
      // 已在监听；不重复注册，避免泄漏。返回当前真实 listening 状态。
      return true
    }
    const cb = (target: harmonyShare.SharableTarget): void => {
      const payload: SharePayload | null = this.currentPayload
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
      this.listening = true
      this.supported = true
      return true
    } catch (e) {
      // 注册失败；清空 callback 引用，不假装 listening/supported。
      this.knockCallback = null
      this.listening = false
      this.supported = false
      return false
    }
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

  // 离开页面/应用退后台时调用：成对 harmonyShare.off('knockShare', callback)。
  // 用 start 时保存的同一 callback 引用 off，保证成对。
  // 未 start 时 stop 是幂等的：直接返回。
  stop(): void {
    if (!this.listening) {
      // 未在监听；不调用 off，避免无对应 on。
      this.knockCallback = null
      return
    }
    const cb = this.knockCallback
    if (cb !== null) {
      try {
        harmonyShare.off('knockShare', cb)
      } catch (e) {
        // off 失败；忽略，仍清理本地状态。
      }
    }
    this.knockCallback = null
    this.listening = false
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
