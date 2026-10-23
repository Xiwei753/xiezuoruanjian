// PeerSyncSession — 一台附近设备的一次同步会话（Issue #988）。
// 协议只负责“把对象搬给 Core 同步事务”，不在 ArkTS 重写 LWW/墓碑规则：
// - hello 协商 protocolVersion/sourceDevice/targetDevice/requestId/dataRootId；
// - manifest 交换对象路径 + Core 稳定版本/内容哈希；
// - read 请求对象内容，writeBegin/writeChunk/writeEnd 传输并校验；
// - 收到对象后交给 PeerSyncCorePort 进入 Core 的 staging → merge → commit；
// - ack 只在 Core 接受后才返回，超时重发同一 requestId 由 Core 幂等处理。
// 连接状态只存在于本会话，不写 SyncConfig.provider_config，也不传 GitHub token。

import {
  PeerSyncTransport,
  PeerTransportListener,
  PeerTransportPort,
  PeerWireMessage
} from './PeerSyncTransport'
import { ContinuationSnapshotStore } from '../continuity/ContinuationSnapshotStore'

export const PEER_PROTOCOL_VERSION: number = 1
// 接续快照走同一文本对象通道，但使用保留路径，不进入 Core 的同步对象集合。
const CONTINUATION_OBJECT_PREFIX: string = '__continuation__/'

export interface PeerObjectDescriptor {
  path: string
  contentHash: string
}

export interface PeerSyncCommitResult {
  success: boolean
  applied: number
  message: string
}

// Core 合并后需要回传对端的对象变更。平台层只搬运，不据此改写同步规则。
export interface PeerSyncCoreMutation {
  kind: string
  path: string
  content: string
  version: string
}

// 由 Core 桥实现。list/read 读 Core 当前对象；apply/delete 只提交对象到 Core 同步事务，
// 不允许平台端直接把远端文件覆盖数据根。
export interface PeerSyncCorePort {
  listLocalObjects(): PeerObjectDescriptor[]
  readLocalObject(path: string): string | null
  applyRemoteObject(path: string, content: string, contentHash: string): PeerSyncCommitResult
  deleteRemoteObject(path: string, contentHash: string): PeerSyncCommitResult
  // 把已收到的对象交给 Core 的 staging → merge → commit；返回值是需要发回对端的变更。
  performCoreSync?(): Promise<PeerSyncCoreMutation[]>
}

export type PeerSyncSessionState = 'idle' | 'connecting' | 'ready' | 'syncing' | 'error' | 'closed'

export interface PeerSyncStatus {
  state: PeerSyncSessionState
  message: string
  applied: number
  // 对端设备是否配置了可选云备份仓库。只描述对端能力，不代表共享账号或凭据。
  remoteHasCloudRepo?: boolean
}

interface PeerHandshakePayload {
  protocolVersion: number
  hasCloudRepo: boolean
  objects: PeerObjectDescriptor[]
}

export interface PeerSyncSessionListener {
  onStatus: (status: PeerSyncStatus) => void
}

export class PeerSyncSession {
  readonly deviceNetworkId: string
  readonly sessionId: number
  private localDeviceId: string
  private corePort: PeerSyncCorePort
  private transport: PeerSyncTransport
  private listener: PeerSyncSessionListener
  private state: PeerSyncSessionState = 'idle'
  private dataRootId: string
  private remoteObjects: Map<string, string> = new Map<string, string>()
  private readRequests: Set<string> = new Set<string>()
  private coreSyncRunning: boolean = false
  private coreSyncRequested: boolean = false
  private localHasCloudRepo: boolean
  private remoteHasCloudRepo: boolean | null = null

  constructor(
    deviceNetworkId: string,
    sessionId: number,
    localDeviceId: string,
    port: PeerTransportPort,
    corePort: PeerSyncCorePort,
    listener: PeerSyncSessionListener,
    localHasCloudRepo: boolean = false
  ) {
    this.deviceNetworkId = deviceNetworkId
    this.sessionId = sessionId
    this.localDeviceId = localDeviceId
    this.corePort = corePort
    this.listener = listener
    this.localHasCloudRepo = localHasCloudRepo
    this.dataRootId = `${localDeviceId}:${Date.now()}`
    const transportListener: PeerTransportListener = {
      onMessage: (message: PeerWireMessage): void => {
        void this.onMessage(message)
      },
      onMalformed: (reason: string): void => this.fail(reason)
    }
    this.transport = new PeerSyncTransport(port, transportListener)
  }

  start(): void {
    this.setState('connecting', '正在协商附近设备同步')
    const hello: PeerWireMessage = {
      version: PEER_PROTOCOL_VERSION,
      type: 'hello',
      requestId: `hello:${this.localDeviceId}:${Date.now()}`,
      sourceDevice: this.localDeviceId,
      targetDevice: this.deviceNetworkId,
      dataRootId: this.dataRootId,
      payload: this.handshakePayload()
    }
    this.send(hello).catch((): void => this.fail('发送协商消息失败'))
  }

  receive(raw: string): void {
    this.transport.receive(raw)
  }

  /** 前台/网络恢复或本地保存后，主动把本机会话对象送回对端。 */
  requestSync(): void {
    if (this.state === 'closed' || this.state === 'error') {
      return
    }
    void this.requestCoreSync()
  }

  /**
   * Issue #988：接续时把源设备冻结的未保存正文送达已连接设备。
   * 正文只作为临时快照对象，不进入 Core 数据根；目标端校验哈希后按 snapshotId 取用。
   */
  publishContinuationSnapshot(snapshotId: string, content: string): void {
    if (this.state === 'closed' || this.state === 'error' || snapshotId.length === 0) {
      return
    }
    const hash: string = PeerSyncTransport.hashText(content)
    const requestId: string = `continuation:${snapshotId}:${hash}`
    this.transport.sendObject(`${CONTINUATION_OBJECT_PREFIX}${snapshotId}`, content, hash,
      requestId, this.localDeviceId, this.deviceNetworkId).catch((): void => {
      // 接续是尽力送达：对端不在线时保留本地快照，不能错误宣称已同步。
    })
  }

  close(): void {
    this.state = 'closed'
    this.listener.onStatus({ state: 'closed', message: '附近设备会话已关闭', applied: 0 })
  }

  private async onMessage(message: PeerWireMessage): Promise<void> {
    if (message.type === 'error') {
      this.fail(message.message ?? '附近设备同步失败')
      return
    }
    if (message.type === 'hello') {
      if (message.payload === undefined) {
        this.fail('协商消息缺少清单')
        return
      }
      this.mergeRemoteManifest(message.payload)
      this.setState('ready', '附近设备已连接')
      await this.sendManifest()
      void this.requestCoreSync()
      return
    }
    if (message.type === 'manifest') {
      if (message.payload !== undefined) {
        this.mergeRemoteManifest(message.payload)
      }
      await this.requestMissingObjects()
      return
    }
    if (message.type === 'read') {
      if (message.path !== undefined) {
        const content: string | null = this.corePort.readLocalObject(message.path)
        if (content !== null) {
          const hash: string = PeerSyncTransport.hashText(content)
          await this.transport.sendObject(
            message.path, content, hash, message.requestId, this.localDeviceId, this.deviceNetworkId)
        }
      }
      return
    }
    if (message.type === 'writeEnd') {
      if (message.chunk === undefined || message.path === undefined || message.contentHash === undefined) {
        this.fail('对象消息不完整')
        return
      }
      if (this.transport.isDuplicate(message.requestId)) {
        await this.sendAck(message.requestId, true, 'duplicate')
        return
      }
      // 接续快照是编辑会话临时对象，不交给 Core 同步事务，也不能覆盖作品数据根。
      if (message.path.startsWith(CONTINUATION_OBJECT_PREFIX)) {
        const snapshotId: string = message.path.substring(CONTINUATION_OBJECT_PREFIX.length)
        ContinuationSnapshotStore.getInstance().save(snapshotId, message.chunk)
        this.transport.rememberRequest(message.requestId)
        await this.sendAck(message.requestId, true, 'continuation snapshot')
        return
      }
      const result: PeerSyncCommitResult =
        this.corePort.applyRemoteObject(message.path, message.chunk, message.contentHash)
      if (result.success) {
        this.transport.rememberRequest(message.requestId)
        await this.sendAck(message.requestId, true, result.message)
        this.remoteObjects.set(message.path, message.contentHash)
        void this.requestCoreSync()
      } else {
        await this.sendAck(message.requestId, false, result.message)
      }
      return
    }
    if (message.type === 'delete') {
      if (message.path === undefined || message.contentHash === undefined) {
        this.fail('删除消息不完整')
        return
      }
      if (this.transport.isDuplicate(message.requestId)) {
        await this.sendAck(message.requestId, true, 'duplicate')
        return
      }
      const result: PeerSyncCommitResult =
        this.corePort.deleteRemoteObject(message.path, message.contentHash)
      if (result.success) {
        this.transport.rememberRequest(message.requestId)
        this.remoteObjects.delete(message.path)
        void this.requestCoreSync()
      }
      await this.sendAck(message.requestId, result.success, result.message)
      return
    }
    if (message.type === 'ack') {
      this.transport.markAck(message.requestId)
    }
  }

  private mergeRemoteManifest(payload: string): void {
    try {
      const handshake: PeerHandshakePayload = JSON.parse(payload) as PeerHandshakePayload
      if (handshake.protocolVersion !== PEER_PROTOCOL_VERSION) {
        this.fail(`附近设备协议版本不一致：${handshake.protocolVersion}`)
        return
      }
      if (handshake.objects === undefined || !Array.isArray(handshake.objects)) {
        this.fail('远端清单格式错误')
        return
      }
      this.remoteHasCloudRepo = handshake.hasCloudRepo === true
      handshake.objects.forEach((object: PeerObjectDescriptor): void => {
        this.remoteObjects.set(object.path, object.contentHash)
      })
    } catch (_e) {
      this.fail('远端清单格式错误')
    }
  }

  private handshakePayload(): string {
    const handshake: PeerHandshakePayload = {
      protocolVersion: PEER_PROTOCOL_VERSION,
      hasCloudRepo: this.localHasCloudRepo,
      objects: this.corePort.listLocalObjects()
    }
    return JSON.stringify(handshake)
  }

  private async sendManifest(): Promise<void> {
    const manifest: PeerWireMessage = {
      version: PEER_PROTOCOL_VERSION,
      type: 'manifest',
      requestId: `manifest:${this.localDeviceId}:${Date.now()}`,
      dataRootId: this.dataRootId,
      payload: this.handshakePayload()
    }
    await this.send(manifest)
  }

  private async requestMissingObjects(): Promise<void> {
    const local: PeerObjectDescriptor[] = this.corePort.listLocalObjects()
    const localHashes: Map<string, string> = new Map<string, string>()
    local.forEach((object: PeerObjectDescriptor): void => {
      localHashes.set(object.path, object.contentHash)
    })
    this.setState('syncing', '正在同步附近设备对象')
    const pending: Promise<boolean>[] = []
    this.remoteObjects.forEach((hash: string, path: string): void => {
      if (localHashes.get(path) === hash || this.readRequests.has(path)) {
        return
      }
      this.readRequests.add(path)
      const requestId: string = `read:${path}:${hash}`
      const request: PeerWireMessage = {
        version: PEER_PROTOCOL_VERSION,
        type: 'read',
        requestId: requestId,
        path: path,
        contentHash: hash,
        sourceDevice: this.localDeviceId,
        targetDevice: this.deviceNetworkId,
        dataRootId: this.dataRootId
      }
      pending.push(this.send(request))
    })
    if (pending.length === 0) {
      this.setState('ready', '附近设备没有新对象')
      return
    }
    await Promise.all(pending)
    this.setState('ready', '附近设备对象已交给 Core')
  }

  private async requestCoreSync(): Promise<void> {
    if (this.corePort.performCoreSync === undefined) {
      return
    }
    if (this.coreSyncRunning) {
      this.coreSyncRequested = true
      return
    }
    this.coreSyncRunning = true
    try {
      do {
        this.coreSyncRequested = false
        const mutations: PeerSyncCoreMutation[] = await this.corePort.performCoreSync!()
        for (const mutation of mutations) {
          await this.sendMutation(mutation)
        }
      } while (this.coreSyncRequested)
      this.setState('ready', '附近设备对象已由 Core 合并')
    } catch (e) {
      this.fail(`Core 合并附近设备对象失败：${String(e)}`)
    } finally {
      this.coreSyncRunning = false
    }
  }

  private async sendMutation(mutation: PeerSyncCoreMutation): Promise<void> {
    if (mutation.kind === 'put') {
      const requestId: string = `put:${mutation.path}:${mutation.version}`
      await this.transport.sendObject(mutation.path, mutation.content,
        PeerSyncTransport.hashText(mutation.content), requestId,
        this.localDeviceId, this.deviceNetworkId)
      return
    }
    const request: PeerWireMessage = {
      version: PEER_PROTOCOL_VERSION,
      type: 'delete',
      requestId: `delete:${mutation.path}:${mutation.version}`,
      path: mutation.path,
      contentHash: mutation.version,
      versionToken: mutation.version,
      sourceDevice: this.localDeviceId,
      targetDevice: this.deviceNetworkId,
      dataRootId: this.dataRootId
    }
    await this.send(request)
  }

  private sendAck(requestId: string, success: boolean, message: string): Promise<boolean> {
    const ack: PeerWireMessage = {
      version: PEER_PROTOCOL_VERSION,
      type: 'ack',
      requestId: requestId,
      message: success ? 'ok' : message,
      payload: success ? 'true' : 'false'
    }
    return this.send(ack)
  }

  private async send(message: PeerWireMessage): Promise<boolean> {
    if (this.state === 'closed') {
      return false
    }
    return this.transport.send(message)
  }

  private fail(message: string): void {
    this.state = 'error'
    this.listener.onStatus({ state: 'error', message: message, applied: 0 })
  }

  private setState(state: PeerSyncSessionState, message: string): void {
    if (this.state === 'closed') {
      return
    }
    this.state = state
    const status: PeerSyncStatus = { state: state, message: message, applied: 0 }
    if (this.remoteHasCloudRepo !== null) {
      status.remoteHasCloudRepo = this.remoteHasCloudRepo
    }
    this.listener.onStatus(status)
  }
}
