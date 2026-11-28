// EditorStatsQueue.ets — 写作统计上报队列（Issue #829 评论 #5995146525 第 5 项起）。
//
// 这一层做两件事，它们各自有独立生命周期：
// 1. 统计发送：把编辑字数事件和实测写作片段组装成强类型写入命令，交给
//    SerialStatsDrain 串行送出；统计条目有界（积压时丢最旧一条并记脏）。
// 2. 正文证明事实：带逐字 changes 的编辑事务原始事实按 eventId + revision 有序保留，
//    保存时整批冻结交给 Core 生成证明批次，随正文一次提交；只有正文回执确认（ack）
//    才释放对应事实。统计发送失败/积压/丢弃都不影响这批事实，也不决定正文是否落盘。
//    带 changes 的统计与证明由 journal 决议后一次写入；没有 changes 的事件没有
//    可回放证明，统计在编辑当场写入。
//
// Issue #981 复核：两类事实必须共用同一条进程级队列，不能各自维护提交链，
// 否则统计页可能在字数事件之后、计时片段之前读到半套数据。
//
// 平台端只透传编辑事实（cause + contentDelta），source 分类（HumanTyped / Pasted /
// Deleted / Unknown）由 Core 的 record_editor_change_stats 决定，这里不猜也不拼 source 字符串。
// 设备身份（deviceId）同样由 Core 自己读/建：早期版本在页面字段初始化时读
// settings.statsDeviceId，那时 loadSettings() 还没跑，拿到的是 null，于是所有 Harmony 统计
// 都写进了同一个 'unknown' 设备桶，且因为是 readonly 再也改不回来。
//
// 串行调度、drain 粒度、barrier 等待条件都在 stats_drain_loop.ts（纯 .ts，可被 Node 单测
// 直接加载真实代码）。之所以要拆开：ArkTS 不允许 .ts import .ets（编译期报 10605999），
// 调度逻辑留在 .ets 里就没法写回归测试，只能写和生产代码分叉的镜像实现。
//
// 早期版本踩过的坑，保留在这里避免以后又走回去：
// - 只在保存时拿整章 old/new 文本比一次：默认自动保存 30 秒，这 30 秒内真实敲的字不形成
//   任何实时事件；而且 Core 的旧启发式「净增 > 20 就当 paste」会把连续敲 30 个字误判成粘贴。
// - flush 等「整轮队列清空」：保存期间用户继续输入，一次自动保存可能永远等不到。
//   现在保存只冻结自己的 proof barrier，不 await 统计成功。
// - drain 一次吃完整个队列：barrier 早就完成了也还在等后面的 seq（#5997254152 第 2 项）。
// - 证明事件跟着统计队列一起被 MAX_QUEUE_SIZE 丢弃（Issue #1069 之前的语义）：
//   统计失败本来就不该阻止正文落盘，证明事实必须保留到正文回执确认。

import { EditorChange, EditorChangeStatsInput, EditorTransactionCause } from '../../../corebridge/dto/EditorDtos'
import { WritingActivitySegmentInput } from '../../../corebridge/dto/StatsDtos'
import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import { StatsWriteBarrierRegistry } from '../../../corebridge/stats/StatsWriteBarrier'
import { SerialStatsDrain } from './stats_drain_loop'

enum QueuedStatsKind {
  EditorChange = 0,
  ActivitySegment = 1
}

/** 队列里一条待写命令：seq 交给调度层分配，业务字段由本队列持有。 */
interface QueuedStatsWrite {
  seq: number
  kind: QueuedStatsKind
  projectId: string
  volumeId: string
  chapterId: string
  eventId: string
  // 编辑字数事件：统计发送与证明批次共用的同一份冻结输入，不再各留一份。
  editorInput: EditorChangeStatsInput | null
  segment: WritingActivitySegmentInput | null
}

// 正文证明事实：按 eventId + revision 有序保留到正文回执确认。
// 与统计条目分开存放 —— 统计队列有界且会丢最旧一条，证明事实不能跟着丢。
interface PendingProofFact {
  seq: number
  eventId: string
  projectId: string
  volumeId: string
  chapterId: string
  // 与统计发送共用同一份输入对象：同一事件只有一份待提交事实。
  input: EditorChangeStatsInput
}

export class EditorStatsQueue {
  private readonly platform: string
  private readonly drain: SerialStatsDrain<QueuedStatsWrite>
  private readonly barrierFlusher: () => Promise<boolean>
  // 待提交的正文证明事实：只有正文回执确认过的 eventId 才从这里移除。
  // 与统计队列的有界积压/失败完全解耦，保存路径也不读统计屏障。
  private pendingProofFacts: PendingProofFact[] = []

  constructor(bridge: IWriterCoreBridge, platform: string) {
    this.platform = platform
    this.barrierFlusher = (): Promise<boolean> => this.flushThrough(this.currentSeq())
    this.drain = new SerialStatsDrain<QueuedStatsWrite>(
      (item: QueuedStatsWrite): Promise<boolean> => {
        if (item.kind === QueuedStatsKind.ActivitySegment) {
          const segment = item.segment
          if (segment === null) {
            return Promise.resolve(false)
          }
          return bridge.recordWritingActivitySegment(segment).then((res) => res.success)
        }
        const input = item.editorInput
        if (input === null) {
          return Promise.resolve(false)
        }
        // 这里 await 的 Promise 背后是 Node-API async work：Rust 调用跑在线程池，
        // 磁盘 I/O 不在 ArkUI 主线程上。串行保证同时只有一条在飞。
        // Issue #1069：走不写 pending 证明的变体 —— 本事件的证明由保存请求的
        // proofBatchJson 一次性提交，Core 不再为同一事件保留第二条 pending 生命周期；
        // 带 changes 时统计也随该批次由 journal 写入（避免同一 eventId 两份业务事实）。
        return bridge.recordEditorChangeStatsWithoutProof(input).then((res) => res.success)
      }
    )
    StatsWriteBarrierRegistry.getInstance().register(this.barrierFlusher)
  }

  /**
   * 编辑事务发生后调用。入队是纯内存操作，不跨 NAPI、不 await，
   * 所以 state listener 调它不会阻塞编辑主链。
   */
  enqueueChange(projectId: string, volumeId: string, chapterId: string,
    cause: EditorTransactionCause, insertedChars: number, deletedChars: number,
    eventId: string, occurredAtMs: number,
    changes: EditorChange[] = [], sessionId: string = '',
    baseRevision: number = 0, newRevision: number = 0,
    oldSelectionAnchor: number = 0, oldSelectionHead: number = 0,
    newSelectionAnchor: number = 0, newSelectionHead: number = 0,
    baselineText: string = '', beforeTextSha256: string = '', afterTextSha256: string = ''): void {
    // 纯光标移动 / 选区变化：Core 那侧也不落盘，这里直接不进队列。
    if (insertedChars <= 0 && deletedChars <= 0) {
      return
    }
    // 统计事件必须带章节上下文，否则 Core 归不到 per_chapter 维度。
    if (chapterId.length === 0) {
      return
    }

    // 同一份输入既是统计发送内容，也是证明批次的来源：只构造一份。
    const input: EditorChangeStatsInput = {
      platform: this.platform,
      projectId: projectId,
      volumeId: volumeId,
      chapterId: chapterId,
      cause: cause,
      insertedChars: insertedChars,
      deletedChars: deletedChars,
      eventId: eventId,
      occurredAtMs: occurredAtMs,
      changes: changes,
      sessionId: sessionId,
      baselineText: baselineText,
      baseRevision: baseRevision,
      newRevision: newRevision,
      oldSelectionAnchor: oldSelectionAnchor,
      oldSelectionHead: oldSelectionHead,
      newSelectionAnchor: newSelectionAnchor,
      newSelectionHead: newSelectionHead,
      beforeTextSha256: beforeTextSha256,
      afterTextSha256: afterTextSha256
    }
    const item: QueuedStatsWrite = {
      seq: 0, // 占位：入队时由调度层从账本分配并覆盖
      kind: QueuedStatsKind.EditorChange,
      projectId: projectId,
      volumeId: volumeId,
      chapterId: chapterId,
      eventId: eventId,
      editorInput: input,
      segment: null
    }
    this.drain.enqueue(item)
    // 只有带逐字 changes 的编辑事务才可能回放成证明；没有 changes 的事件
    // 走 `record_editor_change_stats_without_proof` 立即写统计，不产生证明事实。
    if (changes.length === 0) {
      return
    }
    // 入队后 item.seq 已由调度层分配；证明事实按这个 seq 冻结，与统计条目共享输入。
    // 统计条目可能被积压上限丢弃，但这里的事实保留到正文回执确认。
    this.pendingProofFacts.push({
      seq: item.seq,
      eventId: eventId,
      projectId: projectId,
      volumeId: volumeId,
      chapterId: chapterId,
      input: input
    })
  }

  /** Issue #981：实测写作片段与编辑字数事件进入同一串行队列，稳定 eventId 由平台预先固定。 */
  enqueueActivitySegment(segment: WritingActivitySegmentInput): void {
    if (segment.sessionId.length === 0 || segment.chapterId.length === 0) {
      return
    }
    this.drain.enqueue({
      seq: 0,
      kind: QueuedStatsKind.ActivitySegment,
      projectId: segment.projectId,
      volumeId: segment.volumeId,
      chapterId: segment.chapterId,
      eventId: segment.eventId,
      editorInput: null,
      segment: segment
    })
  }

  /**
   * 当前入队过的最后一个序号。
   *
   * 保存流程拿它当证明事实的冻结上界：这一刻之前入队的编辑事实就是这份正文
   * 随带的证明批次，之后的新输入 seq 更大，留给下一次保存。
   */
  currentSeq(): number {
    return this.drain.currentSeq()
  }

  /**
   * 冻结到 barrier 为止、属于该章节的正文证明事实（按入队顺序，即 revision 递增顺序）。
   *
   * 只包含带逐字 changes、可回放成证明的编辑事务；没有 changes 的事件没有证明，
   * 统计已在编辑当场写入。保存请求把这份冻结输入交给 Core 生成
   * `ChapterProofProjection` 批次，再随正文一起提交。冻结不会移除事实：
   * 保存失败/重试时重取同一批，只有正文回执确认之后才由
   * {@link acknowledgeProofEvents} 释放。
   *
   * 统计发送失败、积压丢弃或有界队列的 `shift()` 都不会让这里少一条事实。
   */
  freezeProofBatchThrough(barrier: number, projectId: string, volumeId: string,
    chapterId: string): EditorChangeStatsInput[] {
    const inputs: EditorChangeStatsInput[] = []
    for (let i = 0; i < this.pendingProofFacts.length; i++) {
      const fact = this.pendingProofFacts[i]
      if (fact.seq <= barrier && fact.projectId === projectId
        && fact.volumeId === volumeId && fact.chapterId === chapterId) {
        inputs.push(fact.input)
      }
    }
    return inputs
  }

  /**
   * 正文 durable 回执确认后，只移除这次请求实际提交的 event ID 及其输入副本。
   * 未确认事件继续保留；旧会话/其他章节的 ID 不受影响。
   */
  acknowledgeProofEvents(eventIds: string[]): void {
    if (eventIds.length === 0) {
      return
    }
    const acknowledged: Set<string> = new Set<string>()
    for (let i = 0; i < eventIds.length; i++) {
      acknowledged.add(eventIds[i])
    }
    const remaining: PendingProofFact[] = []
    for (let i = 0; i < this.pendingProofFacts.length; i++) {
      const fact = this.pendingProofFacts[i]
      if (!acknowledged.has(fact.eventId)) {
        remaining.push(fact)
      }
    }
    this.pendingProofFacts = remaining
  }

  /**
   * 章节已被 Core 删除：丢弃该章节再也不会被提交的证明事实。
   * 只按精确路径丢弃，别的章节（含同卷兄弟章节）的事实原样保留。
   */
  discardProofFacts(projectId: string, volumeId: string, chapterId: string): void {
    const remaining: PendingProofFact[] = []
    for (let i = 0; i < this.pendingProofFacts.length; i++) {
      const fact = this.pendingProofFacts[i]
      if (fact.projectId !== projectId || fact.volumeId !== volumeId
        || fact.chapterId !== chapterId) {
        remaining.push(fact)
      }
    }
    this.pendingProofFacts = remaining
  }

  /**
   * 等到 barrier（含）之前入队的统计都已被 Core 处理完。
   *
   * 只服务统计读取（统计页/回放前的写入屏障）与页面关闭时的尾部刷新；
   * **正文保存不依赖它**：证明事实由 {@link freezeProofBatchThrough} 单独冻结，
   * 统计失败或积压不影响正文落盘。语义见 SerialStatsDrain.flushThrough。
   */
  async flushThrough(barrier: number): Promise<boolean> {
    return this.drain.flushThrough(barrier)
  }

  /** 页面关闭并确认尾部写入后解除查询屏障，避免已销毁页面的队列常驻。 */
  releaseWriteBarrier(): void {
    StatsWriteBarrierRegistry.getInstance().release(this.barrierFlusher)
  }

  /**
   * 因积压上限丢弃过多少条统计。正常应恒为 0，非 0 说明 Core 侧持续写失败。
   * 丢弃的只是统计条目，证明事实仍保留到正文回执确认。
   */
  dropped(): number {
    return this.drain.dropped()
  }
}
