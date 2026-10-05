// EditorStatsQueue — 把编辑事务按顺序异步写进 Core 写作统计。
//
// Issue #829 评论 #5995146525 第 5 项 + 评论 #5996028848：
// 写作统计上报不能卡住编辑主链，所以做成「入队即返回、单协程按序 drain」的串行队列，
// 语义对齐 Android 的 WritingStatsRepository actor。
//
// 为什么需要它（也是上一版错在哪）：
// 旧实现只在保存时拿两份整章文本做一次 processWritingEvent(oldText, newText)，
// 而 Core 那条路径还有「净增 > 20 就当 paste」的启发式。默认自动保存 30 秒，
// 这 30 秒内真实敲的字不形成任何实时事件，状态栏左段一直显示 0。
//
// 上一版队列的两个坑（本版修掉）：
// 1. **假异步**。ArkTS 的 async 函数不会自己开线程，函数会一路同步执行到第一个
//    真正的挂起点。上一版 drain() 里 await 的 recordEditorChangeStats() 在返回
//    Promise 之前就同步调了 nativeRecordEditorChangeStats()，NAPI 又同步进 Rust，
//    于是每个编辑事务的磁盘 I/O（read 当日统计 + write + rename）都落在 ArkUI
//    主线程上。现在切线程点下沉到 C++：nativeRecordEditorChangeStats 返回
//    Node-API async work 的 Promise，execute 回调在线程池里调 Rust。
//    这里 drain() 必须 await 那个 Promise 才是真异步。
// 2. **flush 没等完**。上一版 pendingCount() 只返回 queue.length，而 drain() 在
//    await 之前已经 shift() 掉了当前 in-flight 项 —— 最后一条正在写盘时
//    pendingCount() 已经是 0，退出前的等待会提前判定「已排空」。本版改用
//    drainPromise：只有队列空**且**没有 in-flight 才 resolve。
//
// 平台端只透传编辑事实（cause + contentDelta），source 分类（HumanTyped /
// Pasted / Deleted / Unknown）由 Core 的 record_editor_change_stats 决定，
// 这里不猜也不拼 source 字符串。设备身份（deviceId）同样由 Core 自己读/建：
// 上一版在页面字段初始化时读 settings.statsDeviceId，那时 loadSettings() 还没跑，
// 拿到的是 null，于是所有 Harmony 统计都写进了同一个 'unknown' 设备桶，且因为
// 是 readonly 再也改不回来。
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
  private readonly platform: string
  private queue: QueuedEditorChange[] = []
  // 当前这轮 drain 的 Promise。非空表示「有 in-flight 或正在跑」。
  // flush() 等的就是它，所以不能用 queue.length 那种会漏掉 in-flight 的判据。
  private drainPromise: Promise<boolean> | null = null
  private sessionId: string = ''
  private lastEventMs: number = 0
  private droppedCount: number = 0
  // 本次 flush 期间有几条没写进 Core（Core 返回 false 或 Promise reject）。
  private flushFailures: number = 0

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
    this.ensureDraining()
  }

  /**
   * 等队列真正排空：队列空**且**没有 in-flight 时才 resolve。
   *
   * 返回 false 表示这次等待期间有事件没写进 Core（Core 返回 false 或 Promise
   * reject）。调用方（优雅退出、后台保存）据此能知道统计没落盘，而不是把失败当成功。
   *
   * 注意这是「等到写完」，不是「等一段时间」：上一版 20 × 25ms 的轮询在最后
   * 一条正在写盘时就会判定排空，那不是正确的生命周期。
   */
  async flush(): Promise<boolean> {
    this.flushFailures = 0
    // await 当前这轮；drain 结束后可能会补下一轮（await 期间又入了新事件），
    // 所以循环追到 drainPromise 真正为 null 为止。
    while (this.drainPromise !== null) {
      await this.drainPromise
    }
    return this.flushFailures === 0
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
    const running: Promise<boolean> = this.drain().then((ok: boolean) => {
      // 清掉 in-flight 标记。drainPromise 只可能被 ensureDraining 在为 null 时
      // 替换，所以这里无条件置空不会踩掉别的一轮。
      this.drainPromise = null
      if (this.queue.length > 0) {
        this.ensureDraining()
      }
      return ok
    })
    this.drainPromise = running
  }

  private async drain(): Promise<boolean> {
    try {
      while (this.queue.length > 0) {
        const item = this.queue.shift()
        if (item === undefined) {
          continue
        }
        // 这里 await 的 Promise 背后是 Node-API async work：Rust 调用跑在线程池，
        // 磁盘 I/O 不在 ArkUI 主线程上。串行 drain 保证同时只有一条在飞。
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
        // Core 返回 false / Promise reject 都算这条没写进去。
        // 单条失败不重试：统计少记一条不该让后续上报全停，
        // 更不该把异常抛回编辑主链。下一条继续。
        if (!res.success) {
          this.flushFailures++
        }
      }
    } catch (e) {
      this.flushFailures++
    }
    return this.flushFailures === 0
  }
}