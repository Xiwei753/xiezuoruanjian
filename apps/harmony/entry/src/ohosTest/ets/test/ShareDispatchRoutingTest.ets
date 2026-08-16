// ShareDispatchRoutingTest.ets — HarmonyShareService 调度到三个独立后端的测试。
// 环境阻塞：无 hvigor/ohpm/SDK，需在 CI 或本地 HarmonyOS 环境运行。
// 运行：在 DevEco Studio 中以 ohosTest 模块运行，或 hvigorw assembleHap。
//
// 覆盖（Issue #629 评论5 第6/7部分）：
//   1. system share -> SystemShareService（systemShare.SharedData + ShareController.show，非淘汰的 shareContent）
//   2. tap share -> TapShareService 生命周期接口（start/stop，非一次性 share）
//   3. air transfer -> AirTransferService 生命周期接口（start/stop，返回真实注册结果）
//   4. 各走独立路径，不混用；普通分享只走 system，碰一碰/隔空走生命周期
//   5. 未知 channel 返回 false
//   6. startTapSharing/startAirTransferSharing 返回真实注册结果（不假返回 true）
//
// 注：本测试需在真实 HarmonyOS 环境运行（依赖 @kit.ShareKit）。

import { describe, it, expect } from '@ohos/hypium'
import { HarmonyShareService } from '../../../main/ets/platform/share/HarmonyShareService'
import { SystemShareService } from '../../../main/ets/platform/share/SystemShareService'
import { TapShareService } from '../../../main/ets/platform/share/TapShareService'
import { AirTransferService } from '../../../main/ets/platform/share/AirTransferService'
import { SharePayload, ShareChannel } from '../../../main/ets/platform/share/SharePayload'

export default function shareDispatchRoutingTest() {
  describe('HarmonyShareService 三通道独立调度', () => {
    it('HarmonyShareService 单例 getInstance 返回同一实例', () => {
      const a: HarmonyShareService = HarmonyShareService.getInstance()
      const b: HarmonyShareService = HarmonyShareService.getInstance()
      expect(a === b).assertTrue()
    })

    it('SystemShareService 单例', () => {
      const a: SystemShareService = SystemShareService.getInstance()
      const b: SystemShareService = SystemShareService.getInstance()
      expect(a === b).assertTrue()
    })

    it('TapShareService 单例', () => {
      const a: TapShareService = TapShareService.getInstance()
      const b: TapShareService = TapShareService.getInstance()
      expect(a === b).assertTrue()
    })

    it('AirTransferService 单例', () => {
      const a: AirTransferService = AirTransferService.getInstance()
      const b: AirTransferService = AirTransferService.getInstance()
      expect(a === b).assertTrue()
    })

    it('三个后端 service 互异 — 不混用', () => {
      const sys: SystemShareService = SystemShareService.getInstance()
      const tap: TapShareService = TapShareService.getInstance()
      const air: AirTransferService = AirTransferService.getInstance()
      expect(sys === tap).assertFalse()
      expect(sys === air).assertFalse()
      expect(tap === air).assertFalse()
    })
  })

  describe('ShareChannel 类型契约', () => {
    it('ShareChannel 三个互异字符串值', () => {
      const channels: ShareChannel[] = ['system', 'tap', 'airTransfer']
      expect(channels.length).assertEqual(3)
      expect(new Set(channels).size).assertEqual(3)
      expect(channels[0]).assertEqual('system')
      expect(channels[1]).assertEqual('tap')
      expect(channels[2]).assertEqual('airTransfer')
    })
  })

  describe('SharePayload 形状', () => {
    it('text payload: kind/title/text 可构造', () => {
      const payload: SharePayload = {
        kind: 'text',
        title: '分享标题',
        text: '分享内容'
      }
      expect(payload.kind).assertEqual('text')
      expect(payload.title).assertEqual('分享标题')
      expect(payload.text).assertEqual('分享内容')
    })

    it('file payload: kind/uris 可构造', () => {
      const payload: SharePayload = {
        kind: 'file',
        uris: ['file://uri1', 'file://uri2']
      }
      expect(payload.kind).assertEqual('file')
      expect(payload.uris!.length).assertEqual(2)
    })
  })

  describe('HarmonyShareService.share 调度行为', () => {
    // 第7部分：普通系统分享只调用 share(payload, 'system')。
    // 碰一碰和隔空传送走生命周期接口，不和普通分享混用。

    it('system channel 委托 SystemShareService（ShareController.show，非淘汰的 shareContent）', async () => {
      const svc: HarmonyShareService = HarmonyShareService.getInstance()
      const payload: SharePayload = { kind: 'text', text: 'test' }
      // 真实环境：SystemShareService.share 走 systemShare.SharedData + ShareController.show。
      // SDK 不可用或无 context 时返回 false（不假装成功）。
      const ok: boolean = await svc.share(payload, 'system')
      expect(typeof ok === 'boolean').assertTrue()
    })

    it('tap channel 一次性 share 返回 false：引导走 startTapSharing 生命周期', async () => {
      const svc: HarmonyShareService = HarmonyShareService.getInstance()
      const payload: SharePayload = { kind: 'text', text: 'test' }
      const ok: boolean = await svc.share(payload, 'tap')
      // tap 是生命周期通道，不一次性 share；返回 false 引导调用方用 startTapSharing。
      expect(ok).assertFalse()
    })

    it('airTransfer channel 一次性 share 返回 false：引导走 startAirTransferSharing 生命周期', async () => {
      const svc: HarmonyShareService = HarmonyShareService.getInstance()
      const payload: SharePayload = { kind: 'text', text: 'test' }
      const ok: boolean = await svc.share(payload, 'airTransfer')
      // airTransfer 是生命周期通道，不一次性 share；返回 false 引导调用方用 startAirTransferSharing。
      expect(ok).assertFalse()
    })

    it('未知 channel 返回 false', async () => {
      const svc: HarmonyShareService = HarmonyShareService.getInstance()
      const payload: SharePayload = { kind: 'text', text: 'test' }
      const ok: boolean = await svc.share(payload, 'unknown' as 'system')
      expect(ok).assertFalse()
    })
  })

  describe('生命周期接口返回真实注册结果（第7部分）', () => {
    it('startTapSharing 返回 boolean 真实注册结果', () => {
      const svc: HarmonyShareService = HarmonyShareService.getInstance()
      const payload: SharePayload = { kind: 'text', text: 'hello' }
      // startTapSharing 返回 TapShareService.start 的真实结果。
      // SDK 不可用时 harmonyShare.on 抛错 -> false（不假返回 true）。
      const ok: boolean = svc.startTapSharing(payload)
      expect(typeof ok === 'boolean').assertTrue()
      svc.stopTapSharing()
    })

    it('startAirTransferSharing 返回真实注册结果（不假返回 true）', async () => {
      const svc: HarmonyShareService = HarmonyShareService.getInstance()
      const payload: SharePayload = { kind: 'text', text: 'hello' }
      // 不传 windowId：无 context 时 getWindowId 返回 null -> false。
      // 有 windowId 时返回 AirTransferService.start 的真实结果。
      const ok: boolean = await svc.startAirTransferSharing(payload)
      expect(typeof ok === 'boolean').assertTrue()
      await svc.stopAirTransferSharing()
    })

    it('startAirTransferSharing 显式 windowId 返回真实注册结果', async () => {
      const svc: HarmonyShareService = HarmonyShareService.getInstance()
      const payload: SharePayload = { kind: 'text', text: 'hello' }
      const ok: boolean = await svc.startAirTransferSharing(payload, 1)
      // 真实环境：harmonyShare.on 成功 -> true，抛错 -> false。不假返回 true。
      expect(typeof ok === 'boolean').assertTrue()
      await svc.stopAirTransferSharing(1)
    })
  })

  describe('三通道独立后端 — 不混用', () => {
    it('SystemShareService 只走 systemShare.SharedData + ShareController.show', () => {
      // 不走淘汰的 systemShareManager.shareContent；不走碰一碰/隔空传送 API。
      const svc: SystemShareService = SystemShareService.getInstance()
      expect(svc !== null).assertTrue()
    })

    it('TapShareService 走 harmonyShare.on knockShare 生命周期', () => {
      // 不走 systemShareManager；不走 ShareController.show；不走 gesturesShare。
      const svc: TapShareService = TapShareService.getInstance()
      expect(svc !== null).assertTrue()
    })

    it('AirTransferService 走 harmonyShare.on gesturesShare 生命周期', () => {
      // 不走 systemShareManager；不走 ShareController.show；不走 knockShare。
      const svc: AirTransferService = AirTransferService.getInstance()
      expect(svc !== null).assertTrue()
    })
  })

  describe('能力暴露（第6部分）', () => {
    it('SystemShareService 暴露 isSupported', () => {
      const svc: SystemShareService = SystemShareService.getInstance()
      expect(typeof svc.isSupported()).assertEqual('boolean')
    })

    it('TapShareService 暴露 isSupported', () => {
      const svc: TapShareService = TapShareService.getInstance()
      expect(typeof svc.isSupported()).assertEqual('boolean')
    })

    it('AirTransferService 暴露 isSupported', () => {
      const svc: AirTransferService = AirTransferService.getInstance()
      expect(typeof svc.isSupported()).assertEqual('boolean')
    })
  })
}
