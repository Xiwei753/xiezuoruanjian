// PeerSyncSession — 一台附近设备的一次同步会话（Issue #988）。
//
// 附近设备对象同步全部在 ArkTS 完成：清单/版本来自现有 manifest.sync.json，
// 内容经 SHA-256 校验后暂存，再由 PeerObjectStore 按 LWW 和墓碑规则原子提交。
// 文本协议只负责搬运对象，不复制 Core 的云端 Provider，也不传 GitHub 凭据。

import {
  PeerSyncTransport,
  PeerTransportListener,
  PeerTransportPort,
  PeerWireMessage
} from './PeerSyncTransport'
import { PeerCrypto } from './PeerCrypto'
import { ContinuationSnapshotStore } from '../continuity/ContinuationSnapshotStore'
import {
  isPeerDocumentPath,
  isPeerStarMapObjectPath,
  isPeerStarMapSemanticPath,
  PeerObjectCommitResult,
  PeerObjectStore,
  PeerStoredContent,
  PeerStoredObject
} from './PeerObjectStore'
import {
  ActiveWritingSessionRegistry,
  SyncBarrierScope
} from '../../feature/editor/session/ActiveWritingSessionRegistry'
import { StarMapSyncBarrier } from '../../feature/starmap/platform/StarMapSyncBarrier'

export const PEER_PROTOCOL_VERSION: number = 1
const CONTINUATION_OBJECT_PREFIX: string = '__continuation__/'
const MANIFEST_OBJECT_PREFIX: string = '__manifest__/'
const ACK_TIMEOUT_MS: number = 5000
const MAX_SEND_ATTEMPTS: number = 3

export interface PeerObjectDescriptor {
  path: string
  contentHash: string
  modifiedAtMs: number
  deleted: boolean
  deviceId: string
}

export type PeerSyncSessionState = 'idle' | 'connecting' | 'ready' | 'syncing' | 'error' | 'closed'

export interface PeerSyncStatus {
  state: PeerSyncSessionState
  message: string
  applied: number
  remoteHasCloudRepo?: boolean
}

interface PeerHandshakePayload {
  protocolVersion: number
  hasCloudRepo: boolean
  objects: PeerObjectDescriptor[]
}

interface PendingAck {
  requestId: string
  attempts: number
  timer: number
  resend: () => Promise<boolean>
  resolve: (success: boolean) => void
}

interface PendingRead {
  requestId: string
  path: string
  contentHash: string
  attempts: number
  timer: number
}

export interface PeerSyncSessionListener {
  onStatus: (status: PeerSyncStatus) => void
}

export class PeerSyncSession {
  readonly deviceNetworkId: string
  readonly sessionId: number
  private localDeviceId: string
  private corePort: PeerObjectStore
  private transport: PeerSyncTransport
  private listener: PeerSyncSessionListener
  private state: PeerSyncSessionState = 'idle'
  private dataRootId: string
  private remoteObjects: Map<string, PeerObjectDescriptor> = new Map<string, PeerObjectDescriptor>()
  private pendingReads: Map<string, PendingRead> = new Map<string, PendingRead>()
  private pendingAcks: Map<string, PendingAck> = new Map<string, PendingAck>()
  private localHasCloudRepo: boolean
  private remoteHasCloudRepo: boolean | null = null
  private applied: number = 0
  private unresolvedConflicts: number = 0
  // 双向同步以双方清单都到达为准；只收到单方清单不能宣告收敛。
  private localManifestSent: boolean = false
  private remoteManifestReceived: boolean = false
  // 上一次提交成功后活跃写作会话未对齐：重试时即使对象已无 diff 也要继续回填。
  private pendingWritingReconcile: boolean = false

  constructor(
    deviceNetworkId: string,
    sessionId: number,
    localDeviceId: string,
    port: PeerTransportPort,
    corePort: PeerObjectStore,
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
      payload: JSON.stringify({ protocolVersion: PEER_PROTOCOL_VERSION,
        hasCloudRepo: this.localHasCloudRepo })
    }
    this.send(hello).then((sent: boolean): void => {
      if (!sent) {
        this.fail('发送协商消息失败')
        return
      }
      // 发起方不能只等对端清单；先把自己的清单也送过去，首次连接才是双向交换。
      void this.sendManifest()
    })
  }

  receive(raw: string): void {
    this.transport.receive(raw)
  }

  requestSync(): void {
    if (this.state === 'closed' || this.state === 'error') {
      return
    }
    void this.sendManifest()
  }

  publishContinuationSnapshot(snapshotId: string, content: string): void {
    if (this.state === 'closed' || this.state === 'error' || snapshotId.length === 0) {
      return
    }
    const bytes: Uint8Array = PeerCrypto.utf8Encode(content)
    const hash: string = PeerCrypto.sha256Hex(bytes)
    const requestId: string = `continuation:${snapshotId}:${hash}`
    void this.sendObjectWithAck(`${CONTINUATION_OBJECT_PREFIX}${snapshotId}`, bytes, hash,
      requestId, Date.now(), false, this.localDeviceId)
  }

  // 目标设备按 payload 中的 sourceNetworkId 建立连接后，主动请求源设备的接续快照。
  requestContinuationSnapshot(snapshotId: string, snapshotHash: string): void {
    if (this.state === 'closed' || this.state === 'error' ||
      snapshotId.length === 0 || snapshotHash.length === 0) {
      return
    }
    const request: PeerWireMessage = {
      version: PEER_PROTOCOL_VERSION,
      type: 'read',
      requestId: `continuation-read:${snapshotId}:${snapshotHash}`,
      path: `${CONTINUATION_OBJECT_PREFIX}${snapshotId}`,
      contentHash: snapshotHash,
      sourceDevice: this.localDeviceId,
      targetDevice: this.deviceNetworkId,
      dataRootId: this.dataRootId
    }
    void this.send(request)
  }

  close(): void {
    this.clearPending()
    this.state = 'closed'
    this.listener.onStatus({ state: 'closed', message: '附近设备会话已关闭', applied: this.applied })
  }

  private async onMessage(message: PeerWireMessage): Promise<void> {
    if (message.type === 'error') {
      this.fail(message.message ?? '附近设备同步失败')
      return
    }
    if (message.type === 'hello') {
      if (message.payload === undefined) {
        this.fail('协商消息缺少能力信息')
        return
      }
      try {
        const hello: PeerHandshakePayload = JSON.parse(message.payload) as PeerHandshakePayload
        if (hello.protocolVersion !== PEER_PROTOCOL_VERSION) {
          this.fail(`附近设备协议版本不一致：${hello.protocolVersion}`)
          return
        }
        this.remoteHasCloudRepo = hello.hasCloudRepo === true
      } catch (_e) {
        this.fail('协商消息格式错误')
        return
      }
      this.setState('syncing', '附近设备已连接，正在交换双方清单')
      await this.sendManifest()
      return
    }
    if (message.type === 'manifest') {
      if (message.payload !== undefined) {
        this.mergeRemoteManifest(message.payload)
        this.remoteManifestReceived = true
        await this.requestMissingObjects()
      }
      return
    }
    if (message.type === 'read') {
      if (message.path !== undefined) {
        if (message.path.startsWith(CONTINUATION_OBJECT_PREFIX)) {
          const snapshotId: string = message.path.substring(CONTINUATION_OBJECT_PREFIX.length)
          const snapshot: string | null = ContinuationSnapshotStore.getInstance().get(snapshotId)
          if (snapshot !== null) {
            const snapshotBytes: Uint8Array = PeerCrypto.utf8Encode(snapshot)
            await this.sendObjectWithAck(message.path, snapshotBytes,
              PeerCrypto.sha256Hex(snapshotBytes), message.requestId, Date.now(), false,
              this.localDeviceId)
          }
          return
        }
        const content: PeerStoredContent | null = this.corePort.readLocalObject(message.path)
        if (content !== null) {
          await this.sendObjectWithAck(content.object.path, content.bytes,
            content.object.contentHash, message.requestId, content.object.modifiedAtMs, false,
            content.object.deviceId)
        }
      }
      return
    }
    if (message.type === 'writeEnd') {
      await this.acceptWriteEnd(message)
      return
    }
    if (message.type === 'delete') {
      await this.acceptDelete(message)
      return
    }
    if (message.type === 'ack') {
      this.completeAck(message.requestId, message.payload === 'true', message.message ?? '')
    }
  }

  private async commitStagedObjects(): Promise<PeerObjectCommitResult> {
    const paths: string[] = this.corePort.stagedPaths()
    if (paths.length === 0) {
      return { success: true, applied: 0, conflicts: 0, message: '没有待提交对象' }
    }
    try {
      return await StarMapSyncBarrier.getInstance().withSyncBarrier(paths,
        async (): Promise<PeerObjectCommitResult> => {
          return await ActiveWritingSessionRegistry.getInstance().withSyncBarrier(
            async (scope: SyncBarrierScope): Promise<PeerObjectCommitResult> => {
              const committed: PeerObjectCommitResult =
                await this.corePort.commitRemoteObjects()
              if (!committed.success) {
                return committed
              }
              if (committed.applied > 0) {
                this.pendingWritingReconcile = true
              }
              if (this.pendingWritingReconcile) {
                const reconciled: boolean = await scope.reconcileAfterSync()
                if (!reconciled) {
                  return {
                    success: false,
                    applied: committed.applied,
                    conflicts: committed.conflicts,
                    message: '活跃写作会话未能与附近设备新内容对齐，本次提交未确认'
                  }
                }
                this.pendingWritingReconcile = false
              }
              return committed
            })
        })
    } catch (_e) {
      return {
        success: false,
        applied: 0,
        conflicts: 0,
        message: '活跃写作会话仍在对齐中，附近设备对象暂未提交'
      }
    }
  }

  private async acceptWriteEnd(message: PeerWireMessage): Promise<void> {
    if (message.chunk === undefined || message.path === undefined ||
      message.contentHash === undefined) {
      this.fail('对象消息不完整')
      return
    }
    if (this.transport.isDuplicate(message.requestId)) {
      await this.sendAck(message.requestId, true, 'duplicate')
      return
    }
    let bytes: Uint8Array
    try {
      bytes = PeerCrypto.base64Decode(message.chunk)
    } catch (_e) {
      await this.sendAck(message.requestId, false, 'invalid base64')
      return
    }
    if (PeerCrypto.sha256Hex(bytes) !== message.contentHash) {
      await this.sendAck(message.requestId, false, 'content hash mismatch')
      return
    }
    if (message.path.startsWith(MANIFEST_OBJECT_PREFIX)) {
      const text: string = PeerCrypto.utf8Decode(bytes)
      this.mergeRemoteManifest(text)
      this.remoteManifestReceived = true
      this.transport.rememberRequest(message.requestId)
      if (!await this.sendAck(message.requestId, true, 'manifest')) {
        this.fail('发送附近设备清单确认失败')
        return
      }
      await this.requestMissingObjects()
      return
    }
    if (message.path.startsWith(CONTINUATION_OBJECT_PREFIX)) {
      const snapshotId: string = message.path.substring(CONTINUATION_OBJECT_PREFIX.length)
      ContinuationSnapshotStore.getInstance().save(snapshotId, PeerCrypto.utf8Decode(bytes))
      this.transport.rememberRequest(message.requestId)
      if (!await this.sendAck(message.requestId, true, 'continuation snapshot')) {
        this.fail('发送续接快照确认失败')
      }
      return
    }
    const ownerDeviceId: string = message.ownerDeviceId ?? message.sourceDevice ?? 'unknown'
    const staged: PeerObjectCommitResult = this.corePort.stageRemoteObject(
      message.path, bytes, message.versionToken ?? message.contentHash,
      message.modifiedAtMs ?? Date.now(), false, ownerDeviceId)
    if (!staged.success) {
      await this.sendAck(message.requestId, false, staged.message)
      return
    }
    const committed: PeerObjectCommitResult = await this.commitStagedObjects()
    if (!committed.success) {
      // 不 remember 这笔 requestId：发送端会用同一 requestId 重试，
      // 接收端重新暂存/提交，不能在持久化失败后回成功 ACK。
      await this.sendAck(message.requestId, false, committed.message)
      return
    }
    this.transport.rememberRequest(message.requestId)
    this.applied += committed.applied
    if (committed.conflicts > 0) {
      this.unresolvedConflicts += committed.conflicts
    }
    this.clearReadForPath(message.path)
    this.remoteObjects.set(message.path, {
      path: message.path,
      contentHash: message.versionToken ?? message.contentHash,
      modifiedAtMs: message.modifiedAtMs ?? Date.now(),
      deleted: false,
      deviceId: ownerDeviceId
    })
    // 只有 ACK 真正发送成功后才能推进会话状态；发送失败必须保持可重试的失败状态。
    if (!await this.sendAck(message.requestId, true, committed.message)) {
      this.fail('发送附近设备确认失败')
      return
    }
    this.maybeReady(committed.message)
  }

  private async acceptDelete(message: PeerWireMessage): Promise<void> {
    if (message.path === undefined || message.contentHash === undefined) {
      this.fail('删除消息不完整')
      return
    }
    if (this.transport.isDuplicate(message.requestId)) {
      await this.sendAck(message.requestId, true, 'duplicate')
      return
    }
    const ownerDeviceId: string = message.ownerDeviceId ?? message.sourceDevice ?? 'unknown'
    const staged: PeerObjectCommitResult = this.corePort.stageRemoteObject(
      message.path, new Uint8Array(0), message.versionToken ?? message.contentHash,
      message.modifiedAtMs ?? Date.now(), true, ownerDeviceId)
    if (!staged.success) {
      await this.sendAck(message.requestId, false, staged.message)
      return
    }
    const committed: PeerObjectCommitResult = await this.commitStagedObjects()
    if (!committed.success) {
      // 不 remember 这笔 requestId：发送端会用同一 requestId 重试，
      // 接收端重新暂存/提交，不能在持久化失败后回成功 ACK。
      await this.sendAck(message.requestId, false, committed.message)
      return
    }
    this.transport.rememberRequest(message.requestId)
    this.applied += committed.applied
    if (committed.conflicts > 0) {
      this.unresolvedConflicts += committed.conflicts
    }
    this.remoteObjects.delete(message.path)
    if (!await this.sendAck(message.requestId, true, committed.message)) {
      this.fail('发送附近设备确认失败')
      return
    }
    this.maybeReady(committed.message)
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
      this.remoteObjects.clear()
      handshake.objects.forEach((object: PeerObjectDescriptor): void => {
        this.remoteObjects.set(object.path, object)
      })
    } catch (_e) {
      this.fail('远端清单格式错误')
    }
  }

  private async sendManifest(): Promise<boolean> {
    const objects: PeerObjectDescriptor[] = this.corePort.listLocalObjects().map(
      (object: PeerStoredObject): PeerObjectDescriptor => {
        return {
          path: object.path,
          contentHash: object.contentHash,
          modifiedAtMs: object.modifiedAtMs,
          deleted: object.deleted,
          deviceId: object.deviceId
        }
      })
    const handshake: PeerHandshakePayload = {
      protocolVersion: PEER_PROTOCOL_VERSION,
      hasCloudRepo: this.localHasCloudRepo,
      objects: objects
    }
    const bytes: Uint8Array = PeerCrypto.utf8Encode(JSON.stringify(handshake))
    const requestId: string = `manifest:${this.localDeviceId}:${Date.now()}`
    const sent: boolean = await this.sendObjectWithAck(
      `${MANIFEST_OBJECT_PREFIX}${requestId}`, bytes,
      PeerCrypto.sha256Hex(bytes), requestId, Date.now(), false, this.localDeviceId)
    if (sent) {
      this.localManifestSent = true
      this.maybeReady('附近设备双方清单已交换')
    } else {
      this.fail('发送附近设备清单失败')
    }
    return sent
  }

  private async requestMissingObjects(): Promise<void> {
    const local: Map<string, PeerStoredObject> = new Map<string, PeerStoredObject>()
    this.corePort.listLocalObjects().forEach((object: PeerStoredObject): void => {
      local.set(object.path, object)
    })
    this.setState('syncing', '正在同步附近设备对象')
    let requested: number = 0
    const entries: Array<[string, PeerObjectDescriptor]> =
      Array.from(this.remoteObjects.entries())
    // 先拿节点/边/嵌入对象，再合 GraphMeta，最后合 index：单条消息仍按对象 ACK，
    // 但请求顺序保证活跃星图重载时依赖对象已经落盘。
    entries.sort((left: [string, PeerObjectDescriptor],
      right: [string, PeerObjectDescriptor]): number =>
      this.requestOrder(left[0]) - this.requestOrder(right[0]))
    for (const entry of entries) {
      const path: string = entry[0]
      const remote: PeerObjectDescriptor = entry[1]
      const current: PeerStoredObject | undefined = local.get(path)
      if (remote.deleted) {
        // 墓碑不能因为“本地 mtime 更新”就被跳过：正文/星图对象的删除-修改
        // 必须交给 PeerObjectStore 记录冲突；普通元数据由 store 按 LWW 决定。
        const staged: PeerObjectCommitResult = this.corePort.stageRemoteObject(
          path, new Uint8Array(0), remote.contentHash, remote.modifiedAtMs, true,
          remote.deviceId ?? 'unknown')
        if (!staged.success) {
          this.fail(staged.message)
          return
        }
        const committed: PeerObjectCommitResult = await this.commitStagedObjects()
        if (!committed.success) {
          this.fail(committed.message)
          return
        }
        if (committed.conflicts > 0) {
          this.unresolvedConflicts += committed.conflicts
        }
        continue
      }
      if (current !== undefined && !current.deleted &&
        current.contentHash === remote.contentHash) {
        continue
      }
      // 用户正文、星图索引/GraphMeta、已有共同基线的星图对象必须拉取远端内容：
      // 只在 store 里做三路判断，发送端不能自己按 mtime 提前跳过冲突。
      const mustFetch: boolean = current === undefined ||
        isPeerDocumentPath(path) || isPeerStarMapSemanticPath(path) ||
        (isPeerStarMapObjectPath(path) && this.corePort.hasKnownBase(path))
      if (!mustFetch) {
        if (current !== undefined && current.modifiedAtMs > remote.modifiedAtMs) {
          continue
        }
        if (current !== undefined && current.deleted &&
          current.modifiedAtMs > remote.modifiedAtMs) {
          continue
        }
      }
      if (this.requestObject(path, remote.contentHash)) {
        requested++
      }
    }
    if (requested === 0 && this.pendingReads.size === 0) {
      this.maybeReady('附近设备清单已收敛')
    } else {
      this.setState('syncing', `正在接收 ${this.pendingReads.size} 个附近设备对象`)
    }
  }

  private requestOrder(path: string): number {
    if (path === 'app/starmaps/index.json') {
      return 3
    }
    if (isPeerStarMapSemanticPath(path)) {
      return 2
    }
    return path.startsWith('app/starmaps/') ? 1 : 0
  }

  private requestObject(path: string, contentHash: string): boolean {
    if (this.pendingReads.has(path)) {
      return false
    }
    const requestId: string = `read:${path}:${contentHash}`
    const pending: PendingRead = {
      requestId: requestId,
      path: path,
      contentHash: contentHash,
      attempts: 0,
      timer: -1
    }
    this.pendingReads.set(path, pending)
    void this.sendRead(pending)
    return true
  }

  private async sendRead(pending: PendingRead): Promise<void> {
    pending.attempts++
    const request: PeerWireMessage = {
      version: PEER_PROTOCOL_VERSION,
      type: 'read',
      requestId: pending.requestId,
      path: pending.path,
      contentHash: pending.contentHash,
      sourceDevice: this.localDeviceId,
      targetDevice: this.deviceNetworkId,
      dataRootId: this.dataRootId
    }
    const sent: boolean = await this.send(request)
    if (!sent) {
      this.pendingReads.delete(pending.path)
      this.fail('发送附近设备对象读取请求失败')
      return
    }
    pending.timer = setTimeout((): void => {
      if (!this.pendingReads.has(pending.path)) {
        return
      }
      if (pending.attempts >= MAX_SEND_ATTEMPTS) {
        this.pendingReads.delete(pending.path)
        this.fail(`附近设备对象读取超时：${pending.path}`)
        return
      }
      void this.sendRead(pending)
    }, ACK_TIMEOUT_MS)
  }

  private clearReadForPath(path: string): void {
    const pending: PendingRead | undefined = this.pendingReads.get(path)
    if (pending !== undefined) {
      clearTimeout(pending.timer)
      this.pendingReads.delete(path)
    }
  }

  private async sendObjectWithAck(path: string, bytes: Uint8Array, contentHash: string,
    requestId: string, modifiedAtMs: number, deleted: boolean,
    ownerDeviceId: string): Promise<boolean> {
    const resend = async (): Promise<boolean> => {
      return this.transport.sendObject(path, bytes, contentHash, requestId,
        this.localDeviceId, this.deviceNetworkId, modifiedAtMs, deleted, ownerDeviceId)
    }
    return new Promise<boolean>((resolve: (success: boolean) => void): void => {
      const pending: PendingAck = {
        requestId: requestId,
        attempts: 0,
        timer: -1,
        resend: resend,
        resolve: resolve
      }
      this.pendingAcks.set(requestId, pending)
      void this.sendPending(pending)
    })
  }

  private async sendPending(pending: PendingAck): Promise<void> {
    pending.attempts++
    const sent: boolean = await pending.resend()
    if (!sent) {
      this.completeAck(pending.requestId, false, '对象分片发送失败')
      return
    }
    // 最后一次发送也必须给它完整的确认窗口，不能刚发出就宣告超时。
    pending.timer = setTimeout((): void => {
      if (!this.pendingAcks.has(pending.requestId)) {
        return
      }
      if (pending.attempts >= MAX_SEND_ATTEMPTS) {
        this.completeAck(pending.requestId, false, '附近设备确认超时')
        return
      }
      void this.sendPending(pending)
    }, ACK_TIMEOUT_MS)
  }

  private completeAck(requestId: string, success: boolean, message: string): void {
    const pending: PendingAck | undefined = this.pendingAcks.get(requestId)
    if (pending === undefined) {
      return
    }
    clearTimeout(pending.timer)
    this.pendingAcks.delete(requestId)
    if (!success) {
      this.fail(message.length > 0 ? message : '附近设备拒绝了对象')
      pending.resolve(false)
      return
    }
    this.transport.markAck(requestId)
    this.clearReadForPath(this.pendingReadPath(requestId))
    pending.resolve(true)
    this.maybeReady('附近设备对象已确认接收')
  }

  private maybeReady(message: string): void {
    if (this.state === 'closed' || this.state === 'error') {
      return
    }
    if (this.localManifestSent && this.remoteManifestReceived && this.pendingReads.size === 0) {
      if (this.unresolvedConflicts > 0) {
        this.setState('ready',
          `附近设备同步完成，有 ${this.unresolvedConflicts} 个冲突待处理`)
      } else {
        this.setState('ready', message)
      }
      return
    }
    const detail: string = this.remoteManifestReceived
      ? '正在向附近设备发送本机清单' : '正在等待附近设备清单'
    this.setState('syncing', detail)
  }

  private pendingReadPath(requestId: string): string {
    for (const pending of this.pendingReads.values()) {
      if (pending.requestId === requestId) {
        return pending.path
      }
    }
    return ''
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

  private clearPending(): void {
    this.pendingAcks.forEach((pending: PendingAck): void => {
      clearTimeout(pending.timer)
      pending.resolve(false)
    })
    this.pendingAcks.clear()
    this.pendingReads.forEach((pending: PendingRead): void => {
      clearTimeout(pending.timer)
    })
    this.pendingReads.clear()
  }

  private fail(message: string): void {
    if (this.state === 'closed') {
      return
    }
    this.clearPending()
    this.state = 'error'
    this.listener.onStatus({ state: 'error', message: message, applied: this.applied })
  }

  private setState(state: PeerSyncSessionState, message: string): void {
    if (this.state === 'closed') {
      return
    }
    this.state = state
    const status: PeerSyncStatus = { state: state, message: message, applied: this.applied }
    if (this.remoteHasCloudRepo !== null) {
      status.remoteHasCloudRepo = this.remoteHasCloudRepo
    }
    this.listener.onStatus(status)
  }
}
