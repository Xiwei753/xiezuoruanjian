// PeerSyncTransport — 附近设备同步的文本消息传输层（Issue #988）。
//
// AbilityConnectionManager 对第三方只保证约 1KB 的文本消息，因此这里按“序列化后的
// UTF-8 JSON 帧字节数”切片，而不是按字符数。对象正文先转 UTF-8 字节，再对每片做
// Base64；接收端按 requestId 重组字节并校验 SHA-256。二进制 sendData 不作为既定能力。

import { PeerCrypto } from './PeerCrypto'

const PROTOCOL_VERSION: number = 1
// 单帧硬上限。系统侧限制约 1KB，这里给 JSON 字段和长路径留出余量。
const MAX_FRAME_BYTES: number = 960
const OBJECT_CHUNK_BYTES: number = 384

export type PeerMessageType =
  'hello' | 'manifest' | 'read' | 'writeBegin' | 'writeChunk' | 'writeEnd' | 'delete' | 'ack' | 'error'

export interface PeerWireMessage {
  version: number
  type: PeerMessageType
  requestId: string
  // 本次连接的发送端；ownerDeviceId 才是对象清单里的真实写入者。
  sourceDevice?: string
  targetDevice?: string
  ownerDeviceId?: string
  dataRootId?: string
  path?: string
  contentHash?: string
  versionToken?: string
  modifiedAtMs?: number
  deleted?: boolean
  sequence?: number
  total?: number
  chunk?: string
  payload?: string
  message?: string
}

interface ChunkAssembly {
  requestId: string
  path: string
  contentHash: string
  total: number
  received: number
  chunks: Uint8Array[]
}

export interface PeerTransportPort {
  sendText(message: string): Promise<boolean>
}

export interface PeerTransportListener {
  onMessage: (message: PeerWireMessage) => void
  onMalformed: (reason: string) => void
}

export class PeerSyncTransport {
  private port: PeerTransportPort
  private listener: PeerTransportListener
  private assemblies: Map<string, ChunkAssembly> = new Map<string, ChunkAssembly>()
  private seenRequests: Set<string> = new Set<string>()

  constructor(port: PeerTransportPort, listener: PeerTransportListener) {
    this.port = port
    this.listener = listener
  }

  async send(message: PeerWireMessage): Promise<boolean> {
    const frame: PeerWireMessage = message
    frame.version = PROTOCOL_VERSION
    const encoded: string = JSON.stringify(frame)
    if (PeerCrypto.utf8Encode(encoded).length > MAX_FRAME_BYTES) {
      this.listener.onMalformed('text frame exceeds nearby message limit')
      return false
    }
    return this.port.sendText(encoded)
  }

  /**
   * 发送完整对象字节。每个 writeChunk 的 JSON 帧都在 MAX_FRAME_BYTES 内；
   * 返回 false 表示至少有一帧未送达，调用方必须保留待重试状态。
   */
  async sendObject(
    path: string,
    content: Uint8Array,
    versionToken: string,
    requestId: string,
    sourceDevice: string,
    targetDevice: string,
    modifiedAtMs: number = Date.now(),
    deleted: boolean = false,
    ownerDeviceId: string = sourceDevice
  ): Promise<boolean> {
    const contentHash: string = PeerCrypto.sha256Hex(content)
    const total: number = Math.max(1, Math.ceil(content.length / OBJECT_CHUNK_BYTES))
    const begin: PeerWireMessage = {
      version: PROTOCOL_VERSION,
      type: 'writeBegin',
      requestId: requestId,
      path: path,
      contentHash: contentHash,
      versionToken: versionToken,
      modifiedAtMs: modifiedAtMs,
      deleted: deleted,
      total: total,
      sourceDevice: sourceDevice,
      targetDevice: targetDevice,
      ownerDeviceId: ownerDeviceId
    }
    if (!await this.send(begin)) {
      return false
    }
    for (let i = 0; i < total; i++) {
      const start: number = i * OBJECT_CHUNK_BYTES
      const end: number = Math.min(content.length, start + OBJECT_CHUNK_BYTES)
      const part: Uint8Array = content.slice(start, end)
      const chunkMessage: PeerWireMessage = {
        version: PROTOCOL_VERSION,
        type: 'writeChunk',
        requestId: requestId,
        path: path,
        sequence: i,
        total: total,
        chunk: PeerCrypto.base64Encode(part)
      }
      if (!await this.send(chunkMessage)) {
        return false
      }
    }
    const finish: PeerWireMessage = {
      version: PROTOCOL_VERSION,
      type: 'writeEnd',
      requestId: requestId,
      path: path,
      contentHash: contentHash,
      versionToken: versionToken,
      modifiedAtMs: modifiedAtMs,
      deleted: deleted,
      total: total,
      sourceDevice: sourceDevice,
      targetDevice: targetDevice,
      ownerDeviceId: ownerDeviceId
    }
    return this.send(finish)
  }

  receive(raw: string): void {
    let message: PeerWireMessage
    try {
      message = JSON.parse(raw) as PeerWireMessage
    } catch (_e) {
      this.listener.onMalformed('invalid json')
      return
    }
    if (message.version !== PROTOCOL_VERSION || message.requestId === undefined) {
      this.listener.onMalformed('unsupported protocol version')
      return
    }
    if (message.type === 'writeBegin') {
      this.beginAssembly(message)
      return
    }
    if (message.type === 'writeChunk') {
      this.acceptChunk(message)
      return
    }
    if (message.type === 'writeEnd') {
      this.finishAssembly(message)
      return
    }
    this.listener.onMessage(message)
  }

  markAck(requestId: string): void {
    this.seenRequests.add(requestId)
  }

  isDuplicate(requestId: string): boolean {
    return this.seenRequests.has(requestId)
  }

  rememberRequest(requestId: string): void {
    this.seenRequests.add(requestId)
  }

  private beginAssembly(message: PeerWireMessage): void {
    if (message.path === undefined || message.contentHash === undefined ||
      message.total === undefined || message.total <= 0) {
      this.listener.onMalformed('invalid writeBegin')
      return
    }
    const chunks: Uint8Array[] = []
    for (let i = 0; i < message.total; i++) {
      chunks.push(new Uint8Array(0))
    }
    this.assemblies.set(message.requestId, {
      requestId: message.requestId,
      path: message.path,
      contentHash: message.contentHash,
      total: message.total,
      received: 0,
      chunks: chunks
    })
  }

  private acceptChunk(message: PeerWireMessage): void {
    const assembly: ChunkAssembly | undefined = this.assemblies.get(message.requestId)
    if (assembly === undefined || message.sequence === undefined ||
      message.chunk === undefined || message.sequence < 0 || message.sequence >= assembly.total) {
      this.listener.onMalformed('invalid writeChunk')
      return
    }
    if (assembly.chunks[message.sequence].length === 0) {
      assembly.received++
    }
    try {
      assembly.chunks[message.sequence] = PeerCrypto.base64Decode(message.chunk)
    } catch (_e) {
      this.listener.onMalformed('invalid base64 chunk')
    }
  }

  private finishAssembly(message: PeerWireMessage): void {
    const assembly: ChunkAssembly | undefined = this.assemblies.get(message.requestId)
    if (assembly === undefined || assembly.received < assembly.total) {
      return
    }
    let size: number = 0
    assembly.chunks.forEach((chunk: Uint8Array): void => { size += chunk.length })
    const content: Uint8Array = new Uint8Array(size)
    let offset: number = 0
    assembly.chunks.forEach((chunk: Uint8Array): void => {
      content.set(chunk, offset)
      offset += chunk.length
    })
    if (PeerCrypto.sha256Hex(content) !== assembly.contentHash) {
      this.listener.onMalformed('content hash mismatch')
      this.assemblies.delete(message.requestId)
      return
    }
    this.assemblies.delete(message.requestId)
    const complete: PeerWireMessage = {
      version: PROTOCOL_VERSION,
      type: 'writeEnd',
      requestId: message.requestId,
      path: assembly.path,
      contentHash: assembly.contentHash,
      versionToken: message.versionToken,
      modifiedAtMs: message.modifiedAtMs,
      deleted: message.deleted === true,
      total: assembly.total,
      sourceDevice: message.sourceDevice,
      targetDevice: message.targetDevice,
      ownerDeviceId: message.ownerDeviceId ?? message.sourceDevice,
      chunk: PeerCrypto.base64Encode(content)
    }
    this.listener.onMessage(complete)
  }
}
