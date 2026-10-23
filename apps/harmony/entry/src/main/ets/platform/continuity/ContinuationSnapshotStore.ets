// ContinuationSnapshotStore — 未保存正文的进程内快照（Issue #988）。
//
// 接续 payload 只携带 snapshotId/snapshotHash，不把整章正文塞进 Want。源设备在编辑时
// 把同一 revision 的正文快照放在这里，并通过附近设备会话传给对端；目标设备只有在校验
// snapshotHash 后才把快照写回编辑器。这里不保存 GitHub token/账号信息，也不改变正文
// 持久化格式——快照只是编辑会话的临时交付物，不会直接写 Core 数据根。

const MAX_SNAPSHOTS: number = 24

interface SnapshotWaiter {
  accept: (content: string) => boolean
  resolve: (content: string | null) => void
  timer: number
}

export class ContinuationSnapshotStore {
  private static instance: ContinuationSnapshotStore
  private snapshots: Map<string, string> = new Map<string, string>()
  private waiters: Map<string, SnapshotWaiter[]> = new Map<string, SnapshotWaiter[]>()

  static getInstance(): ContinuationSnapshotStore {
    if (!ContinuationSnapshotStore.instance) {
      ContinuationSnapshotStore.instance = new ContinuationSnapshotStore()
    }
    return ContinuationSnapshotStore.instance
  }

  save(snapshotId: string, content: string): void {
    if (snapshotId.length === 0) {
      return
    }
    this.snapshots.delete(snapshotId)
    this.snapshots.set(snapshotId, content)
    this.resolveWaiters(snapshotId, content)
    while (this.snapshots.size > MAX_SNAPSHOTS) {
      const oldest: string | undefined = this.snapshots.keys().next().value
      if (oldest === undefined) {
        break
      }
      this.snapshots.delete(oldest)
    }
  }

  get(snapshotId: string): string | null {
    const content: string | undefined = this.snapshots.get(snapshotId)
    return content === undefined ? null : content
  }

  /**
   * 等待同一个 snapshotId 的正文真正到达，而不是固定轮询。
   * accept 用于只接受哈希匹配的 revision；超时返回 null，并保留调用方的接续意图。
   */
  waitFor(snapshotId: string, accept: (content: string) => boolean,
    timeoutMs: number = 5000): Promise<string | null> {
    const existing: string | null = this.get(snapshotId)
    if (existing !== null && accept(existing)) {
      return Promise.resolve(existing)
    }
    return new Promise<string | null>((resolve: (content: string | null) => void): void => {
      const waiter: SnapshotWaiter = {
        accept: accept,
        resolve: resolve,
        timer: -1
      }
      waiter.timer = setTimeout((): void => {
        this.removeWaiter(snapshotId, waiter)
        resolve(null)
      }, timeoutMs)
      const list: SnapshotWaiter[] | undefined = this.waiters.get(snapshotId)
      if (list === undefined) {
        this.waiters.set(snapshotId, [waiter])
      } else {
        list.push(waiter)
      }
    })
  }

  remove(snapshotId: string): void {
    this.snapshots.delete(snapshotId)
    this.failWaiters(snapshotId)
  }

  private resolveWaiters(snapshotId: string, content: string): void {
    const list: SnapshotWaiter[] | undefined = this.waiters.get(snapshotId)
    if (list === undefined) {
      return
    }
    const remaining: SnapshotWaiter[] = []
    list.forEach((waiter: SnapshotWaiter): void => {
      if (waiter.accept(content)) {
        clearTimeout(waiter.timer)
        waiter.resolve(content)
      } else {
        remaining.push(waiter)
      }
    })
    if (remaining.length === 0) {
      this.waiters.delete(snapshotId)
    } else {
      this.waiters.set(snapshotId, remaining)
    }
  }

  private failWaiters(snapshotId: string): void {
    const list: SnapshotWaiter[] | undefined = this.waiters.get(snapshotId)
    if (list === undefined) {
      return
    }
    list.forEach((waiter: SnapshotWaiter): void => {
      clearTimeout(waiter.timer)
      waiter.resolve(null)
    })
    this.waiters.delete(snapshotId)
  }

  private removeWaiter(snapshotId: string, waiter: SnapshotWaiter): void {
    const list: SnapshotWaiter[] | undefined = this.waiters.get(snapshotId)
    if (list === undefined) {
      return
    }
    const index: number = list.indexOf(waiter)
    if (index >= 0) {
      list.splice(index, 1)
    }
    if (list.length === 0) {
      this.waiters.delete(snapshotId)
    }
  }
}
