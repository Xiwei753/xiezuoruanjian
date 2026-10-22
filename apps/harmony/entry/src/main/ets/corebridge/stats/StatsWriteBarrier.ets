// StatsWriteBarrier.ets — Issue #981 进程级统计写入屏障。
//
// 编辑字数事件和实测写作片段由 EditorStatsQueue 串行写入 Core；统计页可能由另一个
// 页面、另一个 NativeStatsBridge 实例发起查询。读取前必须等待查询时刻之前已入队的
// 全部写入完成，否则会读到半套字数/时长。
//
// 这里只保存“把队列刷到当前序号”的轻量回调，不持有页面或 UI 状态。

export type StatsWriteFlusher = () => Promise<boolean>

export class StatsWriteBarrierRegistry {
  private static instance: StatsWriteBarrierRegistry | null = null
  private readonly flushers: StatsWriteFlusher[] = []

  static getInstance(): StatsWriteBarrierRegistry {
    if (StatsWriteBarrierRegistry.instance === null) {
      StatsWriteBarrierRegistry.instance = new StatsWriteBarrierRegistry()
    }
    return StatsWriteBarrierRegistry.instance
  }

  register(flusher: StatsWriteFlusher): void {
    if (this.flushers.indexOf(flusher) < 0) {
      this.flushers.push(flusher)
    }
  }

  release(flusher: StatsWriteFlusher): void {
    const index: number = this.flushers.indexOf(flusher)
    if (index >= 0) {
      this.flushers.splice(index, 1)
    }
  }

  /**
   * 对每个已注册队列快照当前序号并等待写完。快照之后的新的写入不属于本次查询，
   * 不会让查询无限等待。
   */
  async awaitCurrentWrites(): Promise<boolean> {
    const snapshot: StatsWriteFlusher[] = this.flushers.slice()
    let clean: boolean = true
    for (const flusher of snapshot) {
      const result: boolean = await flusher()
      if (!result) {
        clean = false
      }
    }
    return clean
  }
}
