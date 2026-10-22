// editor_stats_barrier.ts — 写作统计上报队列的序号账本（Issue #829 评论 #5996577737 第 2 项、
// 评论 #5997254152 第 1 项）。
//
// 为什么单独抽出来：这一层是纯逻辑、不依赖 ArkUI / NAPI，但恰恰是最容易写错的地方 ——
// 最早那版用一个「本次 flush 期间失败了几条」的全局计数器判成败，flush() 一进来就清零，
// 于是 flush 之前就已经失败的那次被抹掉、flush 在 drain 途中调用也会把已累积的失败清零；
// 另外 flush 等的是「队列全空」，保存期间用户继续输入就可能永远等不到。
//
// 现在改成按序号记账：
// - 入队时给每条事件一个单调递增的 seq（nextSeq 返回这个 seq）；
// - 每条处理完（成功或失败）推进 completedSeq；
// - 失败（Core 返回 success=false、Promise reject、或被积压上限丢弃）记下脏序号；
// - flushThrough(barrier) 只等 barrier 及之前的事件处理完，并按「1..=barrier 里有没有
//   脏事件」判成败。判成败只跟 barrier 有关，与谁在什么时候调用 flush 无关，
//   所以失败不会被任何一次后来的 flush 抹掉。
//
// **为什么记「最早的脏序号」而不是「最大的脏序号」**（Issue #829 评论 #5997254152 第 1 项）：
// 问的是「1..=barrier 区间内有没有任何一条缺失」，不是「最后一次缺失在哪」。用最大值判
// 会把更早的失败洗掉：seq=2 失败、后来 seq=5 又失败，此时 lastFailedSeq=5，
// isCleanThrough(4) 会因为 5 > 4 判成 true，而 seq=2 明明缺了一条，barrier=4 必须是 false。
// 失败事件会保留原命令和稳定 eventId，后续 barrier/查询会重试；重试成功后必须把
// 对应序号从失败集合里移除，否则一次短暂失败会让统计永远读不到。
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
  // 仍未成功写入的失败序号。重试成功后移除；只有全部清除才恢复「干净」。
  private failedSeqs: number[] = []
  // 被丢弃（没写进 Core）事件里**最早**的序号。0 表示至今没有丢弃。
  // 丢弃也是一种「这条统计没进 Core」，与失败同口径。
  private firstDroppedSeq: number = 0

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
    if (ok) {
      const index: number = this.failedSeqs.indexOf(seq)
      if (index >= 0) {
        this.failedSeqs.splice(index, 1)
      }
    } else if (this.failedSeqs.indexOf(seq) < 0) {
      this.failedSeqs.push(seq)
    }
  }

  /**
   * 一条事件被丢弃（积压上限等）而没写进 Core。
   * 调用方 flushThrough 越过它时应当看到「不干净」。
   */
  dropped(seq: number): void {
    this.firstDroppedSeq = keepEarliest(this.firstDroppedSeq, seq)
  }

  /** barrier 及之前是否全部处理完。 */
  isCompleteThrough(barrier: number): boolean {
    return this.completedSeq >= barrier
  }

  /**
   * barrier 及之前是否全部成功写进 Core（丢弃也算不干净）。
   *
   * 判据是「1..=barrier 区间内是否仍有失败/丢弃」：失败重试成功会从集合移除，
   * 因此一次短暂失败不会永久污染统计；丢弃仍按最早序号记脏。
   */
  isCleanThrough(barrier: number): boolean {
    const hasFailedInRange: boolean = this.failedSeqs.some((seq: number): boolean => seq <= barrier)
    const hasDroppedInRange: boolean = this.firstDroppedSeq !== 0 && this.firstDroppedSeq <= barrier
    return !hasFailedInRange && !hasDroppedInRange
  }
}

/**
 * 保留最早的脏序号：currentFirst 为 0（还没有）时记 seq，否则取更小的那个。
 * 单独抽成函数是为了让「第一次写入后不再被更晚的覆盖」这个语义只有一处实现。
 */
function keepEarliest(currentFirst: number, seq: number): number {
  if (currentFirst === 0) {
    return seq
  }
  return Math.min(currentFirst, seq)
}
