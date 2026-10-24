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
  PeerAgreementEntry,
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
  // 发送端收到对端持久化成功 ACK 后，用这些字段把该路径和版本写入发件方 Peer agreement。
  path?: string
  contentHash?: string
  deleted?: boolean
}

// 已确认 requestId 的完整 ACK 结果，用于超时重传时原样重发，
// 而不是无条件回 'true' 丢失第一次的 conflict/落盘 hash。
interface AckOutcome {
  success: boolean
  message: string
  committedHash: string
  conflict: boolean
}

interface PendingRead {
  requestId: string
  path: string
  contentHash: string
  attempts: number
  timer: number
  resolve: (success: boolean) => void
}

export interface PeerSyncSessionListener {
  onStatus: (status: PeerSyncStatus) => void
}

export class PeerSyncSession {
  readonly deviceNetworkId: string
  readonly sessionId: number
  private localDeviceId: string
  private remoteDeviceId: string
  private corePort: PeerObjectStore
  private transport: PeerSyncTransport
  private listener: PeerSyncSessionListener
  private state: PeerSyncSessionState = 'idle'
  private dataRootId: string
  private remoteObjects: Map<string, PeerObjectDescriptor> = new Map<string, PeerObjectDescriptor>()
  private pendingReads: Map<string, PendingRead> = new Map<string, PendingRead>()
  private pendingAcks: Map<string, PendingAck> = new Map<string, PendingAck>()
  // 已确认 requestId 的完整 ACK 结果：超时重传相同 requestId 时原样重发，
  // 不丢失第一次的 conflict 标记或接收端实际落盘 hash。
  private rememberedAcks: Map<string, AckOutcome> = new Map<string, AckOutcome>()
  private localHasCloudRepo: boolean
  private remoteHasCloudRepo: boolean | null = null
  private applied: number = 0
  private unresolvedConflicts: number = 0
  // 双向同步以双方清单都到达为准；只收到单方清单不能宣告收敛。
  private localManifestSent: boolean = false
  private remoteManifestReceived: boolean = false
  // 上一次提交成功后活跃写作会话未对齐：重试时即使对象已无 diff 也要继续回填。
  private pendingWritingReconcile: boolean = false
  // 附近设备对象按“接收 → 提交 → ACK → 基线推进”的顺序逐条处理；
  // 星图成员/GraphMeta/index 在一批结束后只通知活跃页面一次。
  private peerBatchActive: boolean = false
  private peerBatchPaths: string[] = []
  private peerBatchApplied: number = 0
  private peerBatchConflicts: number = 0

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
    this.remoteDeviceId = deviceNetworkId
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
    this.rememberedAcks.clear()
    this.state = 'closed'
    this.listener.onStatus({ state: 'closed', message: '附近设备会话已关闭', applied: this.applied })
  }

  private async onMessage(message: PeerWireMessage): Promise<void> {
    if (message.sourceDevice !== undefined && message.sourceDevice.length > 0 &&
      message.sourceDevice !== this.localDeviceId) {
      // sourceDevice 是发送端的稳定设备 ID（不是 networkId），用作附近设备基线键。
      this.remoteDeviceId = message.sourceDevice
    }
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
      this.completeAck(message.requestId, message.payload ?? '', message.message ?? '')
    }
  }

  private async commitStagedObjects(): Promise<PeerObjectCommitResult> {
    const paths: string[] = this.corePort.stagedPaths()
    if (paths.length === 0) {
      return { success: true, applied: 0, conflicts: 0, message: '没有待提交对象' }
    }
    const committed: PeerObjectCommitResult = this.peerBatchActive
      // 清单批处理已经由 StarMapSyncBarrier 持有整批闸门；这里只提交、
      // 不再按单个对象重复关闭星图缓存或释放屏障。
      ? await this.commitStagedWithWritingBarrier()
      : await StarMapSyncBarrier.getInstance().withSyncBarrier(paths,
        async (): Promise<PeerObjectCommitResult> => {
          return await this.commitStagedWithWritingBarrier()
        }, true)
    if (committed.applied > 0 || committed.conflicts > 0) {
      this.notePeerBatch(paths, committed)
    }
    return committed
  }

  private async commitStagedWithWritingBarrier(): Promise<PeerObjectCommitResult> {
    try {
      return await ActiveWritingSessionRegistry.getInstance().withSyncBarrier(
        async (scope: SyncBarrierScope): Promise<PeerObjectCommitResult> => {
          const result: PeerObjectCommitResult =
            await this.corePort.commitRemoteObjects(this.remoteDeviceId)
          if (!result.success) {
            return result
          }
          if (result.applied > 0) {
            this.pendingWritingReconcile = true
          }
          if (this.pendingWritingReconcile) {
            const reconciled: boolean = await scope.reconcileAfterSync()
            if (!reconciled) {
              return {
                success: false,
                applied: result.applied,
                conflicts: result.conflicts,
                message: '活跃写作会话未能与附近设备新内容对齐，本次提交未确认',
                appliedObjects: result.appliedObjects
              }
            }
            this.pendingWritingReconcile = false
          }
          return result
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

  private notePeerBatch(paths: string[], committed: PeerObjectCommitResult): void {
    if (!this.peerBatchActive) {
      return
    }
    paths.forEach((path: string): void => {
      if (!this.peerBatchPaths.includes(path)) {
        this.peerBatchPaths.push(path)
      }
    })
    this.peerBatchApplied += committed.applied
    this.peerBatchConflicts += committed.conflicts
  }

  private resetPeerBatch(): void {
    this.peerBatchActive = false
    this.peerBatchPaths = []
    this.peerBatchApplied = 0
    this.peerBatchConflicts = 0
  }

  // 接收端落盘成功后推进附近设备共同基线。只有实际落盘版本与发送端版本
  // 一致时才写入 agreement；星图语义合并产出第三个 hash M 时 M !== 发送端版本，
  // 两边都暂不推进，留待下一轮清单交换使文件真的一致。
  private confirmCommitted(committed: PeerObjectCommitResult,
    senderVersion: string = ''): boolean {
    const entries = committed.appliedObjects
    if (entries === undefined || entries.length === 0) {
      return true
    }
    const agreed: PeerAgreementEntry[] = senderVersion.length === 0
      ? entries
      : entries.filter((entry: PeerAgreementEntry): boolean =>
        entry.contentHash === senderVersion)
    if (agreed.length === 0) {
      return true
    }
    try {
      this.corePort.confirmPeerObjects(this.remoteDeviceId, agreed)
      return true
    } catch (_e) {
      return false
    }
  }

  // 两端清单中同一 path 的版本一致（含同版本 tombstone）时，把该版本记为
  // 附近设备共同基线。先核对本机仍是该版本，避免把用户刚修改的内容记成基线。
  // 只写 app-meta/peer/agreements.local.json，绝不写 Git 的 known_files。
  private establishEqualPeerAgreement(path: string, expectedHash: string,
    deleted: boolean): void {
    if (expectedHash.length === 0) {
      return
    }
    if (!deleted) {
      const current: PeerStoredContent | null = this.corePort.readLocalObject(path)
      if (current === null || current.object.deleted ||
        current.object.contentHash !== expectedHash) {
        return
      }
    }
    try {
      this.corePort.confirmPeerObjects(this.remoteDeviceId,
        [{ path: path, contentHash: expectedHash, deleted: deleted }])
    } catch (_e) {
      // 基线写入失败不阻断同步，下一轮会再次尝试建立。
    }
  }

  // 从提交结果中找到该路径实际落盘的版本 hash。星图语义合并可能生成
  // 第三个 hash，ACK 需携带它让发送端写入正确的共同基线。
  private committedHashFor(path: string, committed: PeerObjectCommitResult): string {
    if (committed.appliedObjects === undefined) {
      return ''
    }
    for (const entry of committed.appliedObjects) {
      if (entry.path === path) {
        return entry.contentHash
      }
    }
    return ''
  }

  private async acceptWriteEnd(message: PeerWireMessage): Promise<void> {
    if (message.chunk === undefined || message.path === undefined ||
      message.contentHash === undefined) {
      this.fail('对象消息不完整')
      return
    }
    if (this.transport.isDuplicate(message.requestId)) {
      await this.replayAck(message.requestId)
      return
    }
    let bytes: Uint8Array
    try {
      bytes = PeerCrypto.base64Decode(message.chunk)
    } catch (_e) {
      this.clearReadForPath(message.path, false)
      await this.sendAck(message.requestId, false, 'invalid base64')
      return
    }
    if (PeerCrypto.sha256Hex(bytes) !== message.contentHash) {
      this.clearReadForPath(message.path, false)
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
      this.clearReadForPath(message.path, false)
      await this.sendAck(message.requestId, false, staged.message)
      return
    }
    const committed: PeerObjectCommitResult = await this.commitStagedObjects()
    if (!committed.success) {
      // 不 remember 这笔 requestId：发送端会用同一 requestId 重试，
      // 接收端重新暂存/提交，不能在持久化失败后回成功 ACK。
      this.clearReadForPath(message.path, false)
      await this.sendAck(message.requestId, false, committed.message)
      this.fail(committed.message)
      return
    }
    this.applied += committed.applied
    if (committed.conflicts > 0) {
      this.unresolvedConflicts += committed.conflicts
    }
    this.remoteObjects.set(message.path, {
      path: message.path,
      contentHash: message.versionToken ?? message.contentHash,
      modifiedAtMs: message.modifiedAtMs ?? Date.now(),
      deleted: false,
      deviceId: ownerDeviceId
    })
    // 先持久化本轮本地基线，再 remember + 发 ACK：不能先 remember/ACK 后
    // confirmPeerObjects()，确认后存储失败会造成两端成功状态不一致，
    // 且重试会被 isDuplicate 误判为已成功。只有接收端落盘版本与发送端版本
    // 一致时才推进 agreement；合并出第三个 hash M 时两边都暂不推进。
    const senderVersion: string = message.versionToken ?? message.contentHash
    if (!this.confirmCommitted(committed, senderVersion)) {
      this.clearReadForPath(message.path, false)
      this.fail('保存附近设备共同基线失败')
      return
    }
    this.transport.rememberRequest(message.requestId)
    const committedHash: string = this.committedHashFor(message.path, committed)
    // 冲突时接收端保留本地版本，ACK 报告"已保留冲突副本"让发送端结束本次传输，
    // 但不推进共同版本；非冲突时 ACK 携带接收端实际落盘 hash。
    const hasConflict: boolean = committed.conflicts > 0 && committedHash.length === 0
    if (!await this.sendAck(message.requestId, true, committed.message, committedHash, hasConflict)) {
      this.clearReadForPath(message.path, false)
      this.fail('发送附近设备确认失败')
      return
    }
    this.rememberedAcks.set(message.requestId,
      { success: true, message: committed.message, committedHash: committedHash,
        conflict: hasConflict })
    this.clearReadForPath(message.path, true)
    this.maybeReady(committed.message)
  }

  private async acceptDelete(message: PeerWireMessage): Promise<void> {
    if (message.path === undefined || message.contentHash === undefined) {
      this.fail('删除消息不完整')
      return
    }
    if (this.transport.isDuplicate(message.requestId)) {
      await this.replayAck(message.requestId)
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
      this.fail(committed.message)
      return
    }
    this.applied += committed.applied
    if (committed.conflicts > 0) {
      this.unresolvedConflicts += committed.conflicts
    }
    this.remoteObjects.delete(message.path)
    const senderVersion: string = message.versionToken ?? message.contentHash
    if (!this.confirmCommitted(committed, senderVersion)) {
      this.fail('保存附近设备共同基线失败')
      return
    }
    this.transport.rememberRequest(message.requestId)
    const committedHash: string = this.committedHashFor(message.path, committed)
    const hasConflict: boolean = committed.conflicts > 0 && committedHash.length === 0
    if (!await this.sendAck(message.requestId, true, committed.message, committedHash, hasConflict)) {
      this.fail('发送附近设备确认失败')
      return
    }
    this.rememberedAcks.set(message.requestId,
      { success: true, message: committed.message, committedHash: committedHash,
        conflict: hasConflict })
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
    if (this.peerBatchActive) {
      return
    }
    const local: Map<string, PeerStoredObject> = new Map<string, PeerStoredObject>()
    this.corePort.listLocalObjects().forEach((object: PeerStoredObject): void => {
      local.set(object.path, object)
    })
    this.setState('syncing', '正在同步附近设备对象')
    this.peerBatchActive = true
    this.peerBatchPaths = []
    this.peerBatchApplied = 0
    this.peerBatchConflicts = 0
    const entries: Array<[string, PeerObjectDescriptor]> =
      Array.from(this.remoteObjects.entries())
    // 按依赖顺序逐个等待“接收 → 提交 → ACK”；不能只排序后并发发送。
    entries.sort((left: [string, PeerObjectDescriptor],
      right: [string, PeerObjectDescriptor]): number =>
      this.requestOrder(left[0]) - this.requestOrder(right[0]))
    const paths: string[] = entries.map((entry: [string, PeerObjectDescriptor]): string => entry[0])
    let result: PeerObjectCommitResult
    try {
      // 整批持有一个 StarMap barrier：成员对象、graph、index 全部落盘并完成
      // 活跃页面重载后才释放；中途不允许新的星图写入插进来。
      result = await StarMapSyncBarrier.getInstance().withSyncBarrier(paths,
        async (): Promise<PeerObjectCommitResult> => {
          return await this.processMissingObjects(local, entries)
        }, true)
    } catch (_e) {
      result = this.peerBatchResult(false, '附近设备对象同步中断')
    }
    this.resetPeerBatch()
    if (!result.success) {
      this.fail(result.message)
      return
    }
    this.maybeReady('附近设备清单已收敛')
  }

  private async processMissingObjects(local: Map<string, PeerStoredObject>,
    entries: Array<[string, PeerObjectDescriptor]>): Promise<PeerObjectCommitResult> {
    for (const entry of entries) {
      const path: string = entry[0]
      const remote: PeerObjectDescriptor = entry[1]
      const current: PeerStoredObject | undefined = local.get(path)
      if (remote.deleted) {
        // 两端都是同版本墓碑：建立附近设备共同基线，不传输。
        // 这样之后一端恢复创建时不会把另一端的 tombstone 误判成 both_changed。
        if (current !== undefined && current.deleted &&
          current.contentHash === remote.contentHash) {
          this.establishEqualPeerAgreement(path, current.contentHash, true)
          continue
        }
        // 墓碑不能因为“本地 mtime 更新”就被跳过：正文/星图对象的删除-修改
        // 必须交给 PeerObjectStore 记录冲突；普通元数据由 store 按 LWW 决定。
        const staged: PeerObjectCommitResult = this.corePort.stageRemoteObject(
          path, new Uint8Array(0), remote.contentHash, remote.modifiedAtMs, true,
          remote.deviceId ?? 'unknown')
        if (!staged.success) {
          return this.peerBatchResult(false, staged.message)
        }
        const committed: PeerObjectCommitResult = await this.commitStagedObjects()
        if (!committed.success) {
          return this.peerBatchResult(false, committed.message)
        }
        if (committed.conflicts > 0) {
          this.unresolvedConflicts += committed.conflicts
        }
        if (!this.confirmCommitted(committed, remote.contentHash)) {
          return this.peerBatchResult(false, '保存附近设备共同基线失败')
        }
        continue
      }
      if (current !== undefined && !current.deleted && !remote.deleted &&
        current.contentHash === remote.contentHash) {
        // 两端文件版本一致但可能从未互传过、也没有 Git known_files：
        // 把当前版本记为对这台设备已认可的 Peer 基线，避免之后一端编辑到 V2
        // 时 knownBase() 读不到基线，把 V1 误判成"本机也修改"制造假 both_changed。
        // 只修改 app-meta/peer/agreements.local.json，绝不写 Git 的 known_files。
        this.establishEqualPeerAgreement(path, current.contentHash, false)
        continue
      }
      // 用户正文、星图索引/GraphMeta、星图对象文件必须拉取远端内容：
      // 只在 store 里做三路判断，发送端不能自己按 mtime 提前跳过冲突。
      // 普通 LWW 元数据才保留时间预过滤。
      const mustFetch: boolean = current === undefined ||
        isPeerDocumentPath(path) || isPeerStarMapSemanticPath(path) ||
        isPeerStarMapObjectPath(path)
      if (!mustFetch && current !== undefined &&
        current.modifiedAtMs > remote.modifiedAtMs) {
        continue
      }
      const fetched: boolean = await this.requestObjectAndWait(path, remote.contentHash)
      if (!fetched) {
        return this.peerBatchResult(false, '附近设备对象读取失败')
      }
    }
    return this.peerBatchResult(true, '附近设备清单已收敛')
  }

  private peerBatchResult(success: boolean, message: string): PeerObjectCommitResult {
    return {
      success: success,
      applied: this.peerBatchApplied,
      conflicts: this.peerBatchConflicts,
      message: message
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

  private requestObjectAndWait(path: string, contentHash: string): Promise<boolean> {
    if (this.pendingReads.has(path)) {
      return Promise.resolve(false)
    }
    const requestId: string = `read:${path}:${contentHash}`
    return new Promise<boolean>((resolve: (success: boolean) => void): void => {
      const pending: PendingRead = {
        requestId: requestId,
        path: path,
        contentHash: contentHash,
        attempts: 0,
        timer: -1,
        resolve: resolve
      }
      this.pendingReads.set(path, pending)
      void this.sendRead(pending)
    })
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
      pending.resolve(false)
      this.fail('发送附近设备对象读取请求失败')
      return
    }
    pending.timer = setTimeout((): void => {
      if (!this.pendingReads.has(pending.path)) {
        return
      }
      if (pending.attempts >= MAX_SEND_ATTEMPTS) {
        this.pendingReads.delete(pending.path)
        pending.resolve(false)
        this.fail(`附近设备对象读取超时：${pending.path}`)
        return
      }
      void this.sendRead(pending)
    }, ACK_TIMEOUT_MS)
  }

  private clearReadForPath(path: string, success: boolean): void {
    const pending: PendingRead | undefined = this.pendingReads.get(path)
    if (pending !== undefined) {
      clearTimeout(pending.timer)
      this.pendingReads.delete(path)
      pending.resolve(success)
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
        resolve: resolve,
        path: path,
        contentHash: contentHash,
        deleted: deleted
      }
      this.pendingAcks.set(requestId, pending)
      void this.sendPending(pending)
    })
  }

  private async sendPending(pending: PendingAck): Promise<void> {
    pending.attempts++
    const sent: boolean = await pending.resend()
    if (!sent) {
      this.completeAck(pending.requestId, 'false', '对象分片发送失败')
      return
    }
    // 最后一次发送也必须给它完整的确认窗口，不能刚发出就宣告超时。
    pending.timer = setTimeout((): void => {
      if (!this.pendingAcks.has(pending.requestId)) {
        return
      }
      if (pending.attempts >= MAX_SEND_ATTEMPTS) {
        this.completeAck(pending.requestId, 'false', '附近设备确认超时')
        return
      }
      void this.sendPending(pending)
    }, ACK_TIMEOUT_MS)
  }

  private completeAck(requestId: string, payload: string, message: string): void {
    const pending: PendingAck | undefined = this.pendingAcks.get(requestId)
    if (pending === undefined) {
      return
    }
    clearTimeout(pending.timer)
    this.pendingAcks.delete(requestId)
    const success: boolean = payload !== 'false' && payload.length > 0
    if (!success) {
      this.fail(message.length > 0 ? message : '附近设备拒绝了对象')
      pending.resolve(false)
      return
    }
    this.transport.markAck(requestId)
    // 发送端收到 ACK 后，只有当 ACK 携带的接收端实际落盘 hash 与发送端
    // 本地该对象版本一致时，才写入发件方 Peer agreement：
    // - 'conflict'：接收端保留本地版本，不推进共同基线。
    // - 'true'：manifest/continuation 等非业务对象，不涉及共同基线。
    // - 具体 hash 但与发送端版本不同：星图语义合并产出第三个 hash M，
    //   此时 M !== 发送端版本，不推进共同基线，下一轮清单差异交换继续收敛。
    // 双方只在同一个 path 两端真实落盘哈希一致（删除时两端同版本 tombstone）
    // 的情况下更新 Peer agreement。
    if (pending.path !== undefined && pending.path.length > 0 &&
      !pending.path.startsWith(MANIFEST_OBJECT_PREFIX) &&
      !pending.path.startsWith(CONTINUATION_OBJECT_PREFIX) &&
      payload !== 'conflict' && payload !== 'true' &&
      pending.contentHash !== undefined && payload === pending.contentHash) {
      try {
        this.corePort.confirmPeerObjects(this.remoteDeviceId,
          [{ path: pending.path, contentHash: payload,
            deleted: pending.deleted ?? false }])
      } catch (_e) {
        // 基线写入失败不阻断已成功的对象传输，但会让下一轮重复检测失效。
      }
    }
    pending.resolve(true)
    this.maybeReady('附近设备对象已确认接收')
  }

  private maybeReady(message: string): void {
    if (this.state === 'closed' || this.state === 'error') {
      return
    }
    if (this.peerBatchActive) {
      // 批量同步期间单个对象/墓碑 ACK 成功后不能提前标记“已同步”，
      // 必须等成员对象、graph、index 全部按序提交并由活跃星图完成重载。
      this.setState('syncing', '正在按顺序提交附近设备对象')
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

  // 超时重传相同 requestId 时原样重发第一次的完整 ACK 结果，
  // 不丢失 conflict 标记或接收端实际落盘 hash。不重新提交对象。
  private async replayAck(requestId: string): Promise<void> {
    const outcome: AckOutcome | undefined = this.rememberedAcks.get(requestId)
    if (outcome !== undefined) {
      await this.sendAck(requestId, outcome.success, outcome.message,
        outcome.committedHash, outcome.conflict)
    } else {
      await this.sendAck(requestId, true, 'duplicate')
    }
  }

  // ACK payload 语义：
  // - 'false'：接收端提交失败，发送端应重试。
  // - 'conflict'：接收端保留本地版本（冲突未采用），本次传输结束但不推进共同基线。
  // - 'true'：manifest/continuation 等非业务对象成功，不涉及共同基线。
  // - 具体 hash：接收端实际落盘的 contentHash，发送端据此判断双方是否已收敛。
  private sendAck(requestId: string, success: boolean, message: string,
    committedHash: string = '', conflict: boolean = false): Promise<boolean> {
    let payload: string
    if (!success) {
      payload = 'false'
    } else if (conflict) {
      payload = 'conflict'
    } else {
      payload = committedHash.length > 0 ? committedHash : 'true'
    }
    const ack: PeerWireMessage = {
      version: PEER_PROTOCOL_VERSION,
      type: 'ack',
      requestId: requestId,
      message: success ? 'ok' : message,
      payload: payload
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
      pending.resolve(false)
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
