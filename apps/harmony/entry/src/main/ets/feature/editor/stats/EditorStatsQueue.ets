// EditorStatsQueue — 把编辑事务按顺序异步写进 Core 写作统计。
//
// Issue #829 评论 #5995146525 第 5 项、#5996028848、#5996577737：
// 写作统计上报不能卡住编辑主链，所以做成「入队即返回、单协程按序 drain」的串行队列，
// 语义对齐 Android 的 WritingStatsRepository actor。
//
// 为什么需要它（也是最早那版错在哪）：
// 旧实现只在保存时拿两份整章文本做一次 processWritingEvent(oldText, newText)，
// 而 Core 那条路径还有「净增 > 20 就当 paste」的启发式。默认自动保存 30 秒，
// 这 30 秒内真实敲的字不形成任何实时事件，状态栏左段一直显示 0。
//
// 沿途修掉的三个坑：
// 1. **假异步**（#5996028848）。ArkTS 的 async 函数不会自己开线程，函数会一路同步
//    执行到第一个真正的挂起点。上一版 drain() 里 await 的 recordEditorChangeStats()
//    在返回 Promise 之前就同步调了 nativeRecordEditorChangeStats()，NAPI 又同步进
//    Rust，于是每个编辑事务的磁盘 I/O（read 当日统计 + write + rename）都落在
//    ArkUI 主线程上。现在切线程点下沉到 C++：nativeRecordEditorChangeStats 返回
//    Node-API async work 的 Promise，execute 回调在线程池里调 Rust。这里 drain()
//    必须 await 那个 Promise 才是真异步。
// 2. **flush 没等完 + 失败会被抹掉**（#5996577737 第 2 项）。上一版用
//    `flushFailures` 这个全局计数器判成败，但 flush() 一进来就把它清零：某条统计在
//    flush 之前已经失败且 drain 已结束，那次失败就被这次 flush 抹掉了；flush 在 drain
//    途中调用也会把已累积的失败数清零。所以这个 bool 根本不能代表「这批统计都写成功了」。
//    现在改成按序号记账（见 editor_stats_barrier.ts），失败记 seq 而不是记个数。
// 3. **保存可能永远等不到**（#5996577737 第 2 项）。上一版保存成功后调全局 flush()，
//    flush 会一直追到「队列空且无 in-flight」；但保存期间用户还在继续输入，新编辑不断
//    enqueue，一次自动保存理论上可以一直追着「保存快照之后的新事件」跑，activeSave
//    也就一直挂着，后面的保存全在排队。现在保存时记 barrier seq，保存后只
//    flushThrough(barrierSeq) —— 只等这份正文之前的统计，新输入的更大 seq 与这次保存
//    无关，让这次保存能正常结束。
//
// 平台端只透传编辑事实（cause + contentDelta），source 分类（HumanTyped /
// Pasted / Deleted / Unknown）由 Core 的 record_editor_change_stats 决定，
// 这里不猜也不拼 source 字符串。设备身份（deviceId）同样由 Core 自己读/建：
// 早期版本在页面字段初始化时读 settings.statsDeviceId，那时 loadSettings() 还没跑，
// 拿到的是 null，于是所有 Harmony 统计都写进了同一个 'unknown' 设备桶，且因为
// 是 readonly 再也改不回来。
//
// session 与时长口径：与 Android 一致，并与 Core 的 SESSION_GAP_MS 对齐——
// 距上次事件超过 5 分钟视为新会话，换新的 sessionId；durationSeconds 记为
// 距上次事件的秒数，首条为 0。这两个值属于统计口径，所以由本队列持有。

import { EditorTransactionCause } from '../../../corebridge/dto/EditorDtos'
import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import { StatsBarrierLedger } from './editor_stats_barrier'

// 与 Core 写入_stats::SESSION_GAP_MS（5 分钟）一致：超过这个间隔就算新会话。
const STATS_SESSION_GAP_MS: number = 5 * 60 * 1000
// 队列积压上限。正常输入速度下远达不到；超限说明 Core 侧持续失败，
// 再攒只会让内存涨，得丢掉最旧的并计数，交给上层看日志而不是静默膨胀。
const MAX_QUEUE_SIZE: number = 512

interface QueuedEditorChange {
  // 单调递增序号。drain 按入队顺序处理，所以队列本身始终有序，序号不会跳。
  seq: number
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
  private readonly platform: string
  private readonly ledger: StatsBarrierLedger = new StatsBarrierLedger()
  private queue: QueuedEditorChange[] = []
  // 当前这轮 drain 的 Promise。非空表示「有 in-flight 或正在跑」。
  // flushThrough 等的就是它，所以不能用 queue.length 那种会漏掉 in-flight 的判据。
  private drainPromise: Promise<boolean> | null = null
  private sessionId: string = ''
  private lastEventMs: number = 0
  private droppedCount: number = 0

  constructor(bridge: IWriterCoreBridge, platform: string) {
    this.bridge = bridge
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
      const dropped = this.queue.shift()
      this.droppedCount++
      // 被丢弃的这条也按 seq 记一笔：调用方 flushThrough 到它之后时应当看到不干净，
      // 而不是以为「统计都写进去了」。失败只上报一次，不重试。
      if (dropped !== undefined) {
        this.ledger.dropped(dropped.seq)
      }
    }
    const nowMs = Date.now()
    // 距上次事件超过 5 分钟算新会话（与 Core SESSION_GAP_MS 对齐）。
    if (this.lastEventMs === 0 || (nowMs - this.lastEventMs) > STATS_SESSION_GAP_MS) {
      this.sessionId = `hm-${chapterId}-${nowMs}`
    }
    const durationSeconds: number = this.lastEventMs > 0 ? Math.floor((nowMs - this.lastEventMs) / 1000) : 0
    this.lastEventMs = nowMs

    const seq: number = this.ledger.nextSeq()
    this.queue.push({
      seq: seq,
      projectId: projectId,
      volumeId: volumeId,
      chapterId: chapterId,
      cause: cause,
      insertedChars: insertedChars,
      deletedChars: deletedChars,
      durationSeconds: durationSeconds,
      sessionId: this.sessionId
    })
    this.ensureDraining()
  }

  /**
   * 当前入队过的最后一个序号。
   *
   * 保存流程拿它当 barrier：保存这一刻之前入队的统计就是这份正文的统计，
   * 保存后 `flushThrough(barrier)` 只等这些。之后的新输入 seq 更大，与这次保存无关，
   * 不会让这次保存一直等下去。
   */
  currentSeq(): number {
    return this.ledger.currentSeq()
  }

  /**
   * 等到 barrier（含）之前入队的统计都已被 Core 处理完。
   *
   * 注意这是「等到写完」，不是「等一段时间」：队列在 await 期间被新事件填满也不会
   * 影响返回，只要 barrier 之前的那批处理完就 resolve。
   *
   * 返回 false 表示 barrier 以内有事件没写进 Core（Core 返回 success=false、
   * Promise reject，或被积压上限丢弃）。判据是账本里「失败/丢弃的最大序号 <= barrier」，
   * 与调用时机无关 —— 失败是在事件被处理的那一刻按 seq 记下的，不会被之后的 flush 抹掉。
   */
  async flushThrough(barrier: number): Promise<boolean> {
    // barrier 之前的事件已经处理完就直接返回，别再去 await 一轮 drain。
    while (!this.ledger.isCompleteThrough(barrier)) {
      if (this.drainPromise === null) {
        // 没有 drain 在跑但 barrier 还没完成：说明剩余的都是被丢弃的（不推进完成序号）
        // 或者入队后异常中断。为避免死等，这里认为已经尽力了，交给上面的判据报不干净。
        break
      }
      // 队列可能被新事件持续填满，所以每轮 drain 之后重新判断 barrier，而不是
      // 「一次性 await 到队列空」。
      await this.drainPromise
    }
    return this.ledger.isCleanThrough(barrier)
  }

  // 因积压上限丢弃过多少条。正常应恒为 0，非 0 说明 Core 侧持续写失败。
  dropped(): number {
    return this.droppedCount
  }

  private ensureDraining(): void {
    // 已有 drain 在跑就不要再起一个，保证严格串行。
    if (this.drainPromise !== null) {
      return
    }
    const running: Promise<boolean> = this.drain().then((clean: boolean) => {
      // 清掉 in-flight 标记。drainPromise 只可能被 ensureDraining 在为 null 时
      // 替换，所以这里无条件置空不会踩掉别的一轮。
      this.drainPromise = null
      if (this.queue.length > 0) {
        this.ensureDraining()
      }
      return clean
    })
    this.drainPromise = running
  }

  private async drain(): Promise<boolean> {
    while (this.queue.length > 0) {
      const item = this.queue.shift()
      if (item === undefined) {
        continue
      }
      // 这里 await 的 Promise 背后是 Node-API async work：Rust 调用跑在线程池，
      // 磁盘 I/O 不在 ArkUI 主线程上。串行 drain 保证同时只有一条在飞。
      let ok = false
      try {
        const res = await this.bridge.recordEditorChangeStats({
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
        // Core 返回 false 也算这条没写进去。
        ok = res.success
      } catch (e) {
        // 单条失败不重试：统计少记一条不该让后续上报全停，
        // 更不该把异常抛回编辑主链。下一条继续。
        ok = false
      }
      // 无论成败都要推进完成序号：否则 flushThrough 会一直等这条。
      this.ledger.complete(item.seq, ok)
    }
    return true
  }
}
