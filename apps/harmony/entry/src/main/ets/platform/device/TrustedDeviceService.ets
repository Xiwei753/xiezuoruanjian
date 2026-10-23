// TrustedDeviceService — 系统可信设备与附近设备连接的稳定平台入口。
// Issue #988：跨设备同步/接续优先使用 HarmonyOS 已建立的系统可信设备关系
// （DistributedServiceKit 的分布式设备网络）。这里只返回本次会话可用的 networkId、
// 显示名、设备类型和在线状态；networkId 不是永久设备身份，不写数据格式，也不把
// 华为账号“已登录”当作同账号证明。没有 GitHub 仓库、没有登录应用内账号的设备
// 仍可通过附近设备同步。
//
// API18+ 的 AbilityConnection 能力集中在 impl/api18，Service 不直接 import 高版本 Kit。

import { common } from '@kit.AbilityKit'
import { TrustedDeviceApi18 } from './impl/api18/TrustedDeviceApi18'

export interface TrustedDeviceInfo {
  // 仅当前系统会话有效；连接/发现变化后重新查询。
  networkId: string
  deviceId: string
  deviceName: string
  deviceType: string
  online: boolean
}

export interface PeerMessageObserver {
  onPeerMessage?: (sessionId: number, message: string) => void
  onPeerDisconnected?: (sessionId: number, reason: string) => void
}

export class TrustedDeviceService {
  private static instance: TrustedDeviceService
  private observers: Set<PeerMessageObserver> = new Set<PeerMessageObserver>()

  static getInstance(): TrustedDeviceService {
    if (!TrustedDeviceService.instance) {
      TrustedDeviceService.instance = new TrustedDeviceService()
    }
    return TrustedDeviceService.instance
  }

  isSupported(): boolean {
    return TrustedDeviceApi18.getInstance().isSupported()
  }

  // 分布式设备发现需要用户授权的 DISTRIBUTED_DATASYNC。只在用户打开/刷新附近设备
  // 或主动连接时申请，不在启动阶段打扰用户；拒绝后由上层显示真实不可用状态。
  async requestPermission(context: common.UIAbilityContext): Promise<boolean> {
    return TrustedDeviceApi18.getInstance().requestPermission(context)
  }

  // 查询系统可信设备。无权限/能力不可用时返回空数组，并让上层显示“不可用”，
  // 不伪造在线设备。
  listTrustedDevices(): TrustedDeviceInfo[] {
    return TrustedDeviceApi18.getInstance().listTrustedDevices()
  }

  // 建立附近设备连接。返回 <=0 表示失败；连接状态只存在于当前会话。
  async connect(device: TrustedDeviceInfo, context: common.UIAbilityContext): Promise<number> {
    TrustedDeviceApi18.getInstance().setContext(context)
    return TrustedDeviceApi18.getInstance().connect(device)
  }

  // 接收端在 UIAbility.onCollaborate 中调用；token 由系统附带。
  acceptCollaboration(wantParam: Record<string, Object>, context: common.UIAbilityContext): boolean {
    TrustedDeviceApi18.getInstance().setContext(context)
    return TrustedDeviceApi18.getInstance().acceptCollaboration(wantParam)
  }

  async sendMessage(sessionId: number, message: string): Promise<boolean> {
    return TrustedDeviceApi18.getInstance().sendMessage(sessionId, message)
  }

  disconnect(sessionId: number): void {
    TrustedDeviceApi18.getInstance().disconnect(sessionId)
  }

  addObserver(observer: PeerMessageObserver): void {
    this.observers.add(observer)
    this.ensureApiObserver()
  }

  removeObserver(observer: PeerMessageObserver): void {
    this.observers.delete(observer)
  }

  private ensureApiObserver(): void {
    TrustedDeviceApi18.getInstance().setObservers(
      (sessionId: number, message: string): void => {
        this.observers.forEach((observer: PeerMessageObserver): void => {
          if (observer.onPeerMessage !== undefined) {
            observer.onPeerMessage(sessionId, message)
          }
        })
      },
      (sessionId: number, reason: string): void => {
        this.observers.forEach((observer: PeerMessageObserver): void => {
          if (observer.onPeerDisconnected !== undefined) {
            observer.onPeerDisconnected(sessionId, reason)
          }
        })
      }
    )
  }
}
