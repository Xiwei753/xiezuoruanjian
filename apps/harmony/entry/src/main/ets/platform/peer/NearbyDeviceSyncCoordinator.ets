// NearbyDeviceSyncCoordinator — 附近设备会话生命周期（Issue #988）。
//
// 系统可信设备连接只提供本次会话的 sessionId/networkId；这里把文本消息接到
// PeerSyncSession，把收到的对象交给隔离镜像和 Core，再把 Core 产生的变更发回对端。
// 不登录华为账号、不要求本机有 GitHub 仓库，也不传递任何云端凭据。

import { getAppContext } from '../../app/AppContext'
import { NativeCoreModule } from '../../corebridge/native/NativeCoreModule'
import { NativeSyncBridge } from '../../corebridge/native/NativeSyncBridge'
import { HarmonyDeviceIdentity } from '../identity/HarmonyDeviceIdentity'
import {
  PeerMessageObserver,
  TrustedDeviceInfo,
  TrustedDeviceService
} from '../device/TrustedDeviceService'
import { PeerSessionMirror } from './PeerSessionMirror'
import {
  PeerSyncSession,
  PeerSyncSessionListener,
  PeerSyncStatus
} from './PeerSyncSession'
import { PeerTransportPort, PeerWireMessage } from './PeerSyncTransport'

export type PeerStatusHandler = (sessionId: number, status: PeerSyncStatus) => void

class NearbyTransportPort implements PeerTransportPort {
  private service: TrustedDeviceService
  private sessionId: number

  constructor(service: TrustedDeviceService, sessionId: number) {
    this.service = service
    this.sessionId = sessionId
  }

  sendText(text: string): Promise<boolean> {
    return this.service.sendMessage(this.sessionId, text)
  }
}

export class NearbyDeviceSyncCoordinator implements PeerMessageObserver {
  private static instance: NearbyDeviceSyncCoordinator
  private sessions: Map<number, PeerSyncSession> = new Map<number, PeerSyncSession>()
  private mirrors: Map<number, PeerSessionMirror> = new Map<number, PeerSessionMirror>()
  private statusHandler: PeerStatusHandler | null = null
  private attached: boolean = false
  private localHasCloudRepo: boolean = false

  static getInstance(): NearbyDeviceSyncCoordinator {
    if (!NearbyDeviceSyncCoordinator.instance) {
      NearbyDeviceSyncCoordinator.instance = new NearbyDeviceSyncCoordinator()
    }
    return NearbyDeviceSyncCoordinator.instance
  }

  attach(): void {
    if (this.attached) {
      return
    }
    this.attached = true
    TrustedDeviceService.getInstance().addObserver(this)
    void this.refreshLocalCapabilities()
  }

  setStatusHandler(handler: PeerStatusHandler): void {
    this.statusHandler = handler
  }

  // 只读取本机是否配置了可选云备份，用于在设置里展示对端能力；不读取或发送 token。
  async refreshLocalCapabilities(): Promise<void> {
    try {
      const result = await new NativeSyncBridge(new NativeCoreModule()).loadSyncConfig()
      this.localHasCloudRepo = result.success && result.data !== undefined &&
        result.data.enabled && result.data.providerConfig !== null
    } catch (_e) {
      this.localHasCloudRepo = false
    }
  }

  // 发起端在系统连接成功后调用，立即开始协议协商。
  startInitiator(sessionId: number, device: TrustedDeviceInfo): void {
    if (this.sessions.has(sessionId)) {
      return
    }
    this.createSession(sessionId, device.networkId, device.deviceName, true)
  }

  onPeerMessage(sessionId: number, message: string): void {
    let session: PeerSyncSession | undefined = this.sessions.get(sessionId)
    if (session === undefined) {
      // 接收端由系统 onCollaborate 唤起；第一条消息来自发起端 hello。
      const remote: string = this.readSourceDevice(message)
      session = this.createSession(sessionId, remote, remote, false)
    }
    session.receive(message)
  }

  onPeerDisconnected(sessionId: number, _reason: string): void {
    const session: PeerSyncSession | undefined = this.sessions.get(sessionId)
    if (session !== undefined) {
      session.close()
    }
    this.cleanup(sessionId)
  }

  disconnect(sessionId: number): void {
    TrustedDeviceService.getInstance().disconnect(sessionId)
    this.cleanup(sessionId)
  }

  // Issue #988：附近设备会话独立于 GitHub 自动同步；前台恢复、网络恢复或
  // 本地保存后主动让已连接会话走一次 Core 对象同步。Core 内部用同一事务锁，
  // 不复制 LWW/墓碑规则，也不依赖任何云端仓库。
  requestAllSessionsSync(): void {
    this.sessions.forEach((session: PeerSyncSession): void => {
      session.requestSync()
    })
  }

  // Issue #988：接续点击时把当前未保存正文快照送给已连接设备。发送是异步的，
  // 不阻塞同步的 onContinue；目标端若尚未收到，会明确提示内容版本不可用。
  publishContinuationSnapshot(snapshotId: string, content: string): void {
    this.sessions.forEach((session: PeerSyncSession): void => {
      session.publishContinuationSnapshot(snapshotId, content)
    })
  }

  private createSession(sessionId: number, networkId: string, deviceName: string,
    initiator: boolean): PeerSyncSession {
    const context = getAppContext().getAbilityContext()
    const filesDir: string = context === null ? '' : context.filesDir
    const root: string = `${filesDir}/peer-sessions/${sessionId}-${Date.now()}`
    const mirror: PeerSessionMirror =
      new PeerSessionMirror(root, new NativeSyncBridge(new NativeCoreModule()))
    this.mirrors.set(sessionId, mirror)

    const service: TrustedDeviceService = TrustedDeviceService.getInstance()
    const port: PeerTransportPort = new NearbyTransportPort(service, sessionId)
    const listener: PeerSyncSessionListener = {
      onStatus: (status: PeerSyncStatus): void => {
        if (this.statusHandler !== null) {
          this.statusHandler(sessionId, status)
        }
        if (status.state === 'closed' || status.state === 'error') {
          this.cleanup(sessionId)
        }
      }
    }
    const localDeviceId: string = this.localDeviceId()
    const session: PeerSyncSession = new PeerSyncSession(
      networkId, sessionId, localDeviceId, port, mirror, listener, this.localHasCloudRepo)
    this.sessions.set(sessionId, session)
    if (initiator) {
      session.start()
    }
    return session
  }

  private cleanup(sessionId: number): void {
    const mirror: PeerSessionMirror | undefined = this.mirrors.get(sessionId)
    if (mirror !== undefined) {
      mirror.clear()
    }
    this.mirrors.delete(sessionId)
    this.sessions.delete(sessionId)
  }

  private readSourceDevice(message: string): string {
    try {
      const parsed: PeerWireMessage = JSON.parse(message) as PeerWireMessage
      return parsed.sourceDevice ?? 'unknown'
    } catch (_e) {
      return 'unknown'
    }
  }

  private localDeviceId(): string {
    const context = getAppContext().getAbilityContext()
    if (context === null) {
      return 'unknown'
    }
    return HarmonyDeviceIdentity.getOrCreateDeviceId(context)
  }
}
