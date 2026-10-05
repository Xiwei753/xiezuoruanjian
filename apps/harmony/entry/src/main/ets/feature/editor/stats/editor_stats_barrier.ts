// editor_stats_barrier.ts — 写作统计上报队列的序号账本（Issue #829 评论 #5996577737 第 2 项）。
//
// 为什么单独抽出来：这一层是纯逻辑、不依赖 ArkUI / NAPI，但恰恰是最容易写错的地方 ——
// 上一版用一个「本次 flush 期间失败了几条」的全局计数器判成败，flush() 一进来就清零，
// 于是 flush 之前就已经失败的那次被抹掉、flush 在 drain 途中调用也会把已累积的失败清零；
// 另外 flush 等的是「队列全空」，保存期间用户继续输入就可能永远等不到。
//
// 现在改成按序号记账：
// - 入队时给每条事件一个单调递增的 seq（enqueue 返回这个 seq）；
// - 每条处理完（成功或失败）推进 completedSeq；
// - 失败（Core 返回 success=false、Promise reject、或被积压上限丢弃）记 lastFailedSeq；
// - flushThrough(barrier) 只等 barrier 及之前的事件处理完，并按 lastFailedSeq 判成败。
//   判成败只跟「barrier 以内有没有失败」有关，与谁在什么时候调用 flush 无关，
//   所以失败不会被任何一次后来的 flush 抹掉。
//
// 生产代码：feature/editor/stats/EditorStatsQueue.ets。
// 运行测试：node --experimental-strip-types editor_stats_barrier.test.mjs

/** 队列里一条待写事件（只带账本需要的 seq，业务字段由队列自己持有）。 */
export interface SequencedStatsEvent {
  seq: number
}

export class StatsBarrierLedger {
  // 入队过的最后一个序号。0 表示还没入过任何事件。
  private lastEnqueuedSeq: number = 0
  // 已处理完的最后一个序号（无论成功失败）。0 表示还没处理完过任何事件。
  private completedSeq: number = 0
  // 失败涉及的最大序号。0 表示至今没有失败。
  private lastFailedSeq: number = 0
  // 已被丢弃（未入队/被丢弃）的最大序号，和 lastFailedSeq 共用一个「失败」口径。
  // 丢弃也是一种「这条统计没进 Core」，调用方应当看到。
  private lastDroppedSeq: number = 0

  /** 给下一条事件分配序号并记录为「已入队」。返回该序号。 */
  nextSeq(): number {
    this.lastEnqueuedSeq++
    return this.lastEnqueuedSeq
  }

  /** 当前入队过的最后一个序号（保存流程拿它当 barrier）。 */
  currentSeq(): number {
    return this.lastEnqueuedSeq
  }

  /**
   * 一条事件处理完。成功推进 completedSeq；失败则同时记失败。
   * 无论成败都推进 completedSeq —— 否则 flushThrough 会一直等一条永远等不到的 barrier。
   */
  complete(seq: number, ok: boolean): void {
    this.completedSeq = Math.max(this.completedSeq, seq)
    if (!ok) {
      this.lastFailedSeq = Math.max(this.lastFailedSeq, seq)
    }
  }

  /**
   * 一条事件被丢弃（积压上限或不入队）而没写进 Core。
   * 和失败同口径：调用方 flushThrough 越过它时应当看到「不干净」。
   */
  dropped(seq: number): void {
    this.lastDroppedSeq = Math.max(this.lastDroppedSeq, seq)
  }

  /** barrier 及之前是否全部处理完。 */
  isCompleteThrough(barrier: number): boolean {
    return this.completedSeq >= barrier
  }

  /**
   * barrier 及之前是否全部成功写进 Core（丢弃也算不干净）。
   *
   * 判据：不存在「序号在 1..=barrier 之间」的失败或丢弃。
   * 「最脏序号 > barrier」才干净——最脏序号正好等于 barrier 时那条本身就是坏的。
   * 0 是「至今没有失败/丢弃」的哨兵，所以要单独放行。
   * 纯按序号比较，与谁在什么时候调用 flush 无关。
   */
  isCleanThrough(barrier: number): boolean {
    const worst: number = Math.max(this.lastFailedSeq, this.lastDroppedSeq)
    return worst === 0 || worst > barrier
  }
}
