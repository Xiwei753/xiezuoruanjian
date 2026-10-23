// ContinuationSnapshotStore — 未保存正文的进程内快照（Issue #988）。
//
// 接续 payload 只携带 snapshotId/snapshotHash，不把整章正文塞进 Want。源设备在编辑时
// 把同一 revision 的正文快照放在这里，并通过附近设备会话传给对端；目标设备只有在校验
// snapshotHash 后才把快照写回编辑器。这里不保存 GitHub token/账号信息，也不改变正文
// 持久化格式——快照只是编辑会话的临时交付物，不会直接写 Core 数据根。

const MAX_SNAPSHOTS: number = 24

export class ContinuationSnapshotStore {
  private static instance: ContinuationSnapshotStore
  private snapshots: Map<string, string> = new Map<string, string>()

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

  remove(snapshotId: string): void {
    this.snapshots.delete(snapshotId)
  }
}
