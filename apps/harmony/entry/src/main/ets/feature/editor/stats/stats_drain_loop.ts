// stats_drain_loop.ts — 写作统计队列的串行调度层（Issue #829 评论 #5996577737 第 2 项、
// 评论 #5997254152 第 2 项）。
//
// 为什么要单独抽一层：串行保证、drain 粒度、barrier 等待条件是这一层最容易写错的地方，
// 而它们**与 ArkUI / NAPI 无关**——发送动作只是注入进来的一个回调。但队列本体因为要给
// bridge 传强类型的 EditorChangeStatsInput，被迫是 .ets；ArkTS 不允许 .ts import .ets
// （编译期直接报 10605999），所以 .ets 的队列就无法被 Node 直接加载，写不了回归测试。
// 把「调度」和「发什么」分开之后，本文件零 .ets 依赖，测试直接跑真实代码，
// 而不是跑一份和生产代码分叉的镜像实现（镜像分叉等于没测）。
//
// 队列本体（EditorStatsQueue.ets）只负责把事件组装成强类型 payload 交给这里的 send 回调。
//
// 本文件承载并锁死的行为：
// 1. 严格串行：同一时刻只有一条在飞（ensureDraining 不并发起第二轮）。
// 2. **drain 一次只写一条**（#5997254152 第 2 项）。上一版是
//    `while (queue.length > 0)` 把整轮队列一次吃完，drainPromise 只有「整个队列终于空了」
//    才 resolve；而 flushThrough await 的正是这个 drainPromise，于是即使 barrier=10 早就
//    完成了，flush 仍在等同一轮把 seq11、12、13…… 全吃完。用户持续输入时新事件不断塞进
//    同一轮 while，「保存只等自己的 barrier」实际上根本没成立。一条一轮之后，
//    drainPromise 只覆盖「当前这一条」，barrier 一到就能立刻返回。仍然严格串行，
//    也不会增加 Native 调用次数。
// 3. flushThrough(barrier) 只等 barrier 及之前的事件，并重试其中失败的项；只有全部成功才返回 true。
//
// 运行测试：node --experimental-strip-types __tests__/editor_stats_drain.test.mjs

import { StatsBarrierLedger } from './editor_stats_barrier.ts'

/** 队列里一条待写事件：至少带账本需要的 seq，具体业务字段由调用方自己定义。 */
export interface SequencedPayload {
  seq: number
}

/** 积压上限。超过就丢最旧的一条，并按 seq 记一笔「脏」，调用方 flushThrough 时能看到。 */
const MAX_QUEUE_SIZE: number = 512

/** 单条写入失败后的重发次数（Issue #981：eventId 稳定，重发由 Core 幂等去重）。 */
const MAX_SEND_ATTEMPTS: number = 3

/** 一次 barrier 触发的失败重试；done 在这一次重试尝试结束后 resolve。 */
interface RetryJob<T> {
  item: T
  done: Promise<void>
  finish: () => void
}

/**
 * 串行 drain + barrier 等待的调度层。
 *
 * @param ledger 序号账本（唯一事实源：seq 分配、完成推进、脏序号）
 * @param send 把一条事件发给 Core，返回是否成功。失败时本层用同一 item（eventId
 *             已由调用方固定）短暂退避后重发；仍失败才记脏，不让异常抛回编辑主链。
 */
export class SerialStatsDrain<T extends SequencedPayload> {
  private readonly ledger: StatsBarrierLedger
  private readonly send: (item: T) => Promise<boolean>
  private pending: T[] = []
  // 已失败但必须保留原命令/eventId 的项；后续 barrier/查询会重试，成功后才移除。
  private failed: T[] = []
  // 失败重试也必须经过唯一的 drain 执行器：这里只是优先队列，不是第二条发送链。
  private retryQueue: RetryJob<T>[] = []
  // seq -> 当前这次重试任务；并发的 flushThrough 共享同一个 done，不能在第二条发送链上重跑。
  private retryJobs: Map<number, RetryJob<T>> = new Map<number, RetryJob<T>>()
  // 当前那一轮的 Promise。null 表示没有 drain 在跑。
  // 只覆盖「当前这一条」，这是 flushThrough 能按 barrier 提前返回的前提。
  private drainPromise: Promise<void> | null = null
  private droppedCount: number = 0

  // 不用 TS 的构造函数参数属性（private readonly ledger: X 那种写法）：
  // Node 的 --experimental-strip-types 是「仅擦除类型」模式，不支持参数属性这种需要
  // 真实代码生成的语法，这里显式声明字段，保证同一份源码既能被 ArkTS 编译也能被 Node 跑。
  // 账本也由本层持有：seq 分配、完成推进、脏序号是一体的，交给调用方传进来就等于
  // 把「序号是唯一事实源」这件事拆成两半。
  constructor(send: (item: T) => Promise<boolean>) {
    this.ledger = new StatsBarrierLedger()
    this.send = send
  }

  /** 当前积压条数。 */
  pendingCount(): number {
    return this.pending.length
  }

  /** 因积压上限丢弃过多少条。正常应恒为 0，非 0 说明 Core 侧持续写失败。 */
  dropped(): number {
    return this.droppedCount
  }

  /** 当前入队过的最后一个序号（保存流程拿它当 barrier）。 */
  currentSeq(): number {
    return this.ledger.currentSeq()
  }

  /**
   * 入队一条。纯内存操作，调用方（编辑主链）可以放心同步调，不会阻塞。
   *
   * seq 由本层从账本分配（序号是账本的职责，调用方不该自己编号 —— 自己编号就会漏掉
   * 「被丢弃的那条」的记账口径），入队时就写进 item。
   * 入队后立即驱动一轮 drain；已有 drain 在跑就等下一轮，严格串行。
   */
  enqueue(item: T): void {
    if (this.pending.length >= MAX_QUEUE_SIZE) {
      const dropped: T | undefined = this.pending.shift()
      // 被丢弃的这条也按 seq 记一笔：调用方 flushThrough 到它之后时应当看到不干净，
      // 而不是以为「统计都写进去了」。丢弃不重试。
      if (dropped !== undefined) {
        this.droppedCount++
        this.ledger.dropped(dropped.seq)
      }
    }
    item.seq = this.ledger.nextSeq()
    this.pending.push(item)
    this.ensureDraining()
  }

  /**
   * 等到 barrier（含）之前入队的统计都已被 Core 处理完。
   *
   * 注意这是「等到写完」，不是「等一段时间」：drain 一条一轮，所以这里每 await 完一条就
   * 重新看 barrier —— 队列在 await 期间被新事件填满也不影响返回，barrier 之前的那批
   * 处理完就 resolve，不会追着后面的 seq 跑。
   *
   * 返回 false 表示 barrier 以内仍有事件没写进 Core（重试后仍失败，或被积压上限
   * 丢弃）。失败项保留原命令与稳定 eventId，下一次查询/关闭会继续重试。
   */
  async flushThrough(barrier: number): Promise<boolean> {
    // 把此前失败、且属于本次查询范围的项交给同一个 drain 执行器优先处理。
    // 不是另开 send 链：这些重试仍由 ensureDraining/drainOne 串行消费。
    const retryJobs: RetryJob<T>[] = this.scheduleRetriesThrough(barrier)
    if (retryJobs.length > 0) {
      this.ensureDraining()
      await Promise.all(retryJobs.map((job: RetryJob<T>): Promise<void> => job.done))
    }
    while (!this.ledger.isCompleteThrough(barrier)) {
      if (this.drainPromise === null) {
        // 没有 drain 在跑但 barrier 还没完成：说明剩余的都是被丢弃的（不推进完成序号）
        // 或者入队后异常中断。为避免死等，这里认为已经尽力了，交给上面的判据报不干净。
        break
      }
      await this.drainPromise
    }
    return this.ledger.isCleanThrough(barrier)
  }

  /** 把 barrier 内的失败项移入优先重试队列；同一 seq 不会被重复调度。 */
  private scheduleRetriesThrough(barrier: number): RetryJob<T>[] {
    const jobs: RetryJob<T>[] = []
    const retained: T[] = []
    for (const item of this.failed) {
      if (item.seq > barrier) {
        retained.push(item)
        continue
      }
      // 失败项在成功前一直留在 failed 里；并发的查询只共享已存在的任务，
      // 不能因为 failed 被移走就误以为“没有待重试项”并提前返回。
      retained.push(item)
      const existing: RetryJob<T> | undefined = this.retryJobs.get(item.seq)
      if (existing !== undefined) {
        jobs.push(existing)
        continue
      }
      let finish: () => void = (): void => {}
      const done: Promise<void> = new Promise<void>((resolve: () => void): void => {
        finish = resolve
      })
      const job: RetryJob<T> = {
        item: item,
        done: done,
        finish: (): void => {
          this.retryJobs.delete(item.seq)
          finish()
        }
      }
      this.retryJobs.set(item.seq, job)
      this.retryQueue.push(job)
      jobs.push(job)
    }
    this.failed = retained
    return jobs
  }

  /** 同一 payload/eventId 最多尝试 MAX_SEND_ATTEMPTS 次，失败后交由 failed 保留。 */
  private async sendWithRetry(item: T): Promise<boolean> {
    let ok: boolean = false
    for (let attempt: number = 1; attempt <= MAX_SEND_ATTEMPTS && !ok; attempt++) {
      try {
        ok = await this.send(item)
      } catch (e) {
        ok = false
      }
      if (!ok && attempt < MAX_SEND_ATTEMPTS) {
        // 稳定 eventId 让重发是幂等的；短暂退避避免 Core 短暂不可用时立刻丢事实。
        await new Promise<void>((resolve: () => void): void => {
          setTimeout(resolve, attempt * 100)
        })
      }
    }
    return ok
  }

  private ensureDraining(): void {
    // 已有 drain 在跑就不要再起一个，保证严格串行。
    if (this.drainPromise !== null) {
      return
    }
    const running: Promise<void> = this.drainOne().then(() => {
      // 清掉 in-flight 标记。drainPromise 只可能被 ensureDraining 在为 null 时
      // 替换，所以这里无条件置空不会踩掉别的一轮。
      this.drainPromise = null
      if (this.retryQueue.length > 0 || this.pending.length > 0) {
        this.ensureDraining()
      }
    })
    this.drainPromise = running
  }

  /** 只处理一条就返回（#5997254152 第 2 项）。见文件头第 2 条。 */
  private async drainOne(): Promise<void> {
    // 失败重试优先于新输入，但同样走这唯一一条串行执行路径。
    const retryJob: RetryJob<T> | undefined = this.retryQueue.shift()
    const item: T | undefined = retryJob !== undefined ? retryJob.item : this.pending.shift()
    if (item === undefined) {
      return
    }
    const ok: boolean = await this.sendWithRetry(item)
    // 无论成败都要推进完成序号：否则 flushThrough 会一直等这条。
    this.ledger.complete(item.seq, ok)
    if (ok) {
      this.failed = this.failed.filter((failed: T): boolean => failed.seq !== item.seq)
    } else if (!this.failed.some((failed: T): boolean => failed.seq === item.seq)) {
      // 保留原 item（含稳定 eventId），交由下一次 barrier/查询重试，不因页面销毁丢弃。
      this.failed.push(item)
    }
    if (retryJob !== undefined) {
      retryJob.finish()
    }
  }
}