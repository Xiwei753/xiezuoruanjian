// EditorStatsQueue — 把编辑事务按顺序异步写进 Core 写作统计。
//
// Issue #829 评论 #5995146525 第 5 项：写作统计上报不能卡住编辑主链，
// 也不要在 state listener 里同步跨 NAPI 写盘。所以这里做成「入队即返回、
// 单协程按序 drain」的串行队列，语义对齐 Android 的 WritingStatsRepository actor：
// 进程内只保留一个在跑的 drain 循环，事件严格按入队顺序送达 Core。
//
// 为什么需要它（也是上一版错在哪）：
// 旧实现只在保存时拿两份整章文本做一次 processWritingEvent(oldText, newText)，
// 而 Core 那条路径还有「净增 > 20 就当 paste」的启发式。默认自动保存 30 秒，
// 这 30 秒内真实敲的字不形成任何实时事件，状态栏左段一直显示 0。
//
// 平台端只透传编辑事实（cause + contentDelta），source 分类（HumanTyped /
// Pasted / Deleted / Unknown）由 Core 的 record_editor_change_stats 决定，
// 这里不猜也不拼 source 字符串。
//
// session 与时长口径：与 Android 一致，并与 Core 的 SESSION_GAP_MS 对齐——
// 距上次事件超过 5 分钟视为新会话，换新的 sessionId；durationSeconds 记为
// 距上次事件的秒数，首条为 0。这两个值属于统计口径，所以由本队列持有。

import { EditorTransactionCause } from '../../../corebridge/dto/EditorDtos'
import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'

// 与 Core 写入_stats::SESSION_GAP_MS（5 分钟）一致：超过这个间隔就算新会话。
const STATS_SESSION_GAP_MS: number = 5 * 60 * 1000
// 队列积压上限。正常输入速度下远达不到；超限说明 Core 侧持续失败，
// 再攒只会让内存涨，得丢掉最旧的并计数，交给上层看日志而不是静默膨胀。
const MAX_QUEUE_SIZE: number = 512

interface QueuedEditorChange {
  projectId: string
  volumeId: string
  chapterId: string
  cause: EditorTransactionCause
  insertedChars: number
  deletedChars: number
  durationSeconds: number
  sessionId: string
}

export class EditorStatsQueue {
  private readonly bridge: IWriterCoreBridge
  private readonly deviceId: string
  private readonly platform: string
  private queue: QueuedEditorChange[] = []
  private draining: boolean = false
  private sessionId: string = ''
  private lastEventMs: number = 0
  private droppedCount: number = 0

  constructor(bridge: IWriterCoreBridge, deviceId: string, platform: string) {
    this.bridge = bridge
    this.deviceId = deviceId
    this.platform = platform
  }

  // 编辑事务发生后调用。入队是纯内存操作，不跨 NAPI、不 await，
  // 所以 state listener 调它不会阻塞编辑主链。
  enqueueChange(projectId: string, volumeId: string, chapterId: string,
    cause: EditorTransactionCause, insertedChars: number, deletedChars: number): void {
    // 纯光标移动 / 选区变化：Core 那侧也不落盘，这里直接不进队列。
    if (insertedChars <= 0 && deletedChars <= 0) {
      return
    }
    // 统计事件必须带章节上下文，否则 Core 归不到 per_chapter 维度。
    if (chapterId.length === 0) {
      return
    }
    if (this.queue.length >= MAX_QUEUE_SIZE) {
      this.queue.shift()
      this.droppedCount++
    }
    const nowMs = Date.now()
    // 距上次事件超过 5 分钟算新会话（与 Core SESSION_GAP_MS 对齐）。
    if (this.lastEventMs === 0 || (nowMs - this.lastEventMs) > STATS_SESSION_GAP_MS) {
      this.sessionId = `hm-${chapterId}-${nowMs}`
    }
    const durationSeconds: number = this.lastEventMs > 0 ? Math.floor((nowMs - this.lastEventMs) / 1000) : 0
    this.lastEventMs = nowMs

    this.queue.push({
      projectId: projectId,
      volumeId: volumeId,
      chapterId: chapterId,
      cause: cause,
      insertedChars: insertedChars,
      deletedChars: deletedChars,
      durationSeconds: durationSeconds,
      sessionId: this.sessionId
    })
    this.startDrain()
  }

  // 队列里还有多少条没写完。退出前用它判断要不要等一下。
  pendingCount(): number {
    return this.queue.length
  }

  // 因积压上限丢弃过多少条。正常应恒为 0，非 0 说明 Core 侧持续写失败。
  dropped(): number {
    return this.droppedCount
  }

  private startDrain(): void {
    // 已有 drain 在跑就不要再起一个，保证严格串行。
    if (this.draining) {
      return
    }
    this.draining = true
    this.drain()
  }

  private async drain(): Promise<void> {
    try {
      while (this.queue.length > 0) {
        const item = this.queue.shift()
        if (item === undefined) {
          continue
        }
        await this.bridge.recordEditorChangeStats({
          deviceId: this.deviceId,
          platform: this.platform,
          projectId: item.projectId,
          volumeId: item.volumeId,
          chapterId: item.chapterId,
          cause: item.cause,
          insertedChars: item.insertedChars,
          deletedChars: item.deletedChars,
          durationSeconds: item.durationSeconds,
          sessionId: item.sessionId
        })
      }
    } catch (e) {
      // 单条失败不重试：统计少记一条不该让后续编辑上报全停，
      // 更不该把异常抛回编辑主链。下一条继续。
    } finally {
      this.draining = false
      // await 期间可能又入了新事件，补一次 drain 别漏掉。
      if (this.queue.length > 0) {
        this.startDrain()
      }
    }
  }
}