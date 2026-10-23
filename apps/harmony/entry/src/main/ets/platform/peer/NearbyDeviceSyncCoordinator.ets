// NearbyDeviceSyncCoordinator — 附近设备会话生命周期（Issue #988）。
//
// 系统可信设备连接只提供本次会话的 sessionId/networkId；这里把文本消息接到
// PeerSyncSession，把对象暂存到 ETS PeerObjectStore 并按 LWW/墓碑提交。
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
import { PeerObjectStore } from './PeerObjectStore'
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
  private stores: Map<number, PeerObjectStore> = new Map<number, PeerObjectStore>()
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

  // 只读取本机是否配置了可选云备份，用于设置页展示对端能力；不读取或发送 token。
  async refreshLocalCapabilities(): Promise<void> {
    try {
      const result = await new NativeSyncBridge(new NativeCoreModule()).loadSyncConfig()
      this.localHasCloudRepo = result.success && result.data !== undefined &&
        result.data.enabled && result.data.providerConfig !== null
    } catch (_e) {
      this.localHasCloudRepo = false
    }
  }

  startInitiator(sessionId: number, device: TrustedDeviceInfo): void {
    if (this.sessions.has(sessionId)) {
      return
    }
    this.createSession(sessionId, device.networkId, true)
  }

  onPeerMessage(sessionId: number, message: string): void {
    let session: PeerSyncSession | undefined = this.sessions.get(sessionId)
    if (session === undefined) {
      const remote: string = this.readSourceDevice(message)
      session = this.createSession(sessionId, remote, false)
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

  // 前台恢复、网络恢复或本地保存后主动让已连接会话交换清单。
  requestAllSessionsSync(): void {
    this.sessions.forEach((session: PeerSyncSession): void => {
      session.requestSync()
    })
  }

  publishContinuationSnapshot(snapshotId: string, content: string): void {
    this.sessions.forEach((session: PeerSyncSession): void => {
      session.publishContinuationSnapshot(snapshotId, content)
    })
  }

  // Issue #988：目标设备按 payload 的 sourceNetworkId 找回源设备，主动请求冻结快照。
  async requestContinuationFromDevice(networkId: string, snapshotId: string,
    snapshotHash: string): Promise<boolean> {
    if (networkId.length === 0 || snapshotId.length === 0 || snapshotHash.length === 0) {
      return false
    }
    const existing: PeerSyncSession | undefined = this.sessionForNetwork(networkId)
    if (existing !== undefined) {
      existing.requestContinuationSnapshot(snapshotId, snapshotHash)
      return true
    }
    const context = getAppContext().getAbilityContext()
    if (context === null) {
      return false
    }
    const service: TrustedDeviceService = TrustedDeviceService.getInstance()
    if (!service.isSupported() || !await service.requestPermission(context)) {
      return false
    }
    let device: TrustedDeviceInfo | null = null
    for (const candidate of service.listTrustedDevices()) {
      if (candidate.networkId === networkId) {
        device = candidate
      }
    }
    if (device === null) {
      return false
    }
    const sessionId: number = await service.connect(device, context)
    if (sessionId <= 0) {
      return false
    }
    this.startInitiator(sessionId, device)
    const session: PeerSyncSession | undefined = this.sessions.get(sessionId)
    if (session === undefined) {
      return false
    }
    session.requestContinuationSnapshot(snapshotId, snapshotHash)
    return true
  }

  private sessionForNetwork(networkId: string): PeerSyncSession | undefined {
    for (const session of this.sessions.values()) {
      if (session.deviceNetworkId === networkId) {
        return session
      }
    }
    return undefined
  }

  private createSession(sessionId: number, networkId: string, initiator: boolean): PeerSyncSession {
    const store: PeerObjectStore = new PeerObjectStore()
    this.stores.set(sessionId, store)

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
      networkId, sessionId, localDeviceId, port, store, listener, this.localHasCloudRepo)
    this.sessions.set(sessionId, session)
    if (initiator) {
      session.start()
    }
    return session
  }

  private cleanup(sessionId: number): void {
    const store: PeerObjectStore | undefined = this.stores.get(sessionId)
    if (store !== undefined) {
      store.clearStaged()
    }
    this.stores.delete(sessionId)
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
