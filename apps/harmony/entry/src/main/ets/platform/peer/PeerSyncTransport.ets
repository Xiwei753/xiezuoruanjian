// PeerSyncTransport — 附近设备同步的文本消息传输层（Issue #988）。
// AbilityConnectionManager 对第三方只保证文本消息，因此对象内容必须切片成有界消息：
// 每片带 requestId/sequence/total/contentHash，接收端按 requestId 重组并校验哈希；
// 超时由 PeerSyncSession 重发同一 requestId，接收端按 requestId 幂等去重。
// 这里不把二进制 sendData 当既定能力，也不在平台层复制 Core 的 LWW/删除规则。

const PROTOCOL_VERSION: number = 1
const MAX_TEXT_CHUNK_CHARS: number = 12000

export type PeerMessageType =
  'hello' | 'manifest' | 'read' | 'writeBegin' | 'writeChunk' | 'writeEnd' | 'delete' | 'ack' | 'error'

export interface PeerWireMessage {
  version: number
  type: PeerMessageType
  requestId: string
  sourceDevice?: string
  targetDevice?: string
  dataRootId?: string
  path?: string
  contentHash?: string
  versionToken?: string
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
  chunks: string[]
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
    if (message.type === 'writeBegin' || message.type === 'writeChunk' || message.type === 'writeEnd') {
      return this.sendWriteFrame(message)
    }
    return this.port.sendText(JSON.stringify(message))
  }

  // 写对象：先发声明，再按 MAX_TEXT_CHUNK_CHARS 切片，最后发 end；每片都可用同一
  // requestId 重发。接收端只有收齐 total 且哈希一致才交给同步会话。
  async sendObject(
    path: string,
    content: string,
    contentHash: string,
    requestId: string,
    sourceDevice: string,
    targetDevice: string
  ): Promise<boolean> {
    const total: number = Math.max(1, Math.ceil(content.length / MAX_TEXT_CHUNK_CHARS))
    const begin: PeerWireMessage = {
      version: PROTOCOL_VERSION,
      type: 'writeBegin',
      requestId: requestId,
      path: path,
      contentHash: contentHash,
      total: total,
      sourceDevice: sourceDevice,
      targetDevice: targetDevice
    }
    if (!await this.port.sendText(JSON.stringify(begin))) {
      return false
    }
    for (let i = 0; i < total; i++) {
      const start: number = i * MAX_TEXT_CHUNK_CHARS
      const end: number = Math.min(content.length, start + MAX_TEXT_CHUNK_CHARS)
      const chunkMessage: PeerWireMessage = {
        version: PROTOCOL_VERSION,
        type: 'writeChunk',
        requestId: requestId,
        path: path,
        sequence: i,
        total: total,
        chunk: content.substring(start, end)
      }
      if (!await this.port.sendText(JSON.stringify(chunkMessage))) {
        return false
      }
    }
    const finish: PeerWireMessage = {
      version: PROTOCOL_VERSION,
      type: 'writeEnd',
      requestId: requestId,
      path: path,
      contentHash: contentHash,
      total: total
    }
    return this.port.sendText(JSON.stringify(finish))
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

  // 已收过的 requestId 只允许重发 ack，不重复应用对象/删除。
  rememberRequest(requestId: string): void {
    this.seenRequests.add(requestId)
  }

  private async sendWriteFrame(message: PeerWireMessage): Promise<boolean> {
    // write 帧已经由 PeerSyncSession 拆成有界文本；这里只负责传输。
    return this.port.sendText(JSON.stringify(message))
  }

  private beginAssembly(message: PeerWireMessage): void {
    if (message.path === undefined || message.contentHash === undefined ||
      message.total === undefined || message.total <= 0) {
      this.listener.onMalformed('invalid writeBegin')
      return
    }
    this.assemblies.set(message.requestId, {
      requestId: message.requestId,
      path: message.path,
      contentHash: message.contentHash,
      total: message.total,
      received: 0,
      chunks: new Array<string>(message.total).fill('')
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
    assembly.chunks[message.sequence] = message.chunk
  }

  private finishAssembly(message: PeerWireMessage): void {
    const assembly: ChunkAssembly | undefined = this.assemblies.get(message.requestId)
    if (assembly === undefined || assembly.received < assembly.total) {
      return
    }
    const content: string = assembly.chunks.join('')
    if (PeerSyncTransport.hashText(content) !== assembly.contentHash) {
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
      total: assembly.total,
      chunk: content
    }
    this.listener.onMessage(complete)
  }

  static hashText(text: string): string {
    let hash: number = 0x811c9dc5
    for (let i = 0; i < text.length; i++) {
      hash ^= text.charCodeAt(i)
      hash = Math.imul(hash, 0x01000193) >>> 0
    }
    return hash.toString(16).padStart(8, '0')
  }
}
