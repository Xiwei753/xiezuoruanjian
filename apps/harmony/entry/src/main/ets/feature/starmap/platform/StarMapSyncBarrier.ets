// StarMapSyncBarrier — 星图页面与附近设备同步提交之间的屏障（Issue #988）。
//
// 附近设备同步写入的是 live 星图文件；StarMapScreen / StarMapScene 仍可能持有旧图，
// 或正有一次星图 CRUD 在 bridge 里落盘。仅在写完后 emit 事件不足以保护活跃星图：
// Home 会刷新，但当前星图不会重新 getStarMapGraph，旧图随后还可能覆盖刚写入的对象。
//
// 边界：
// - 本类只负责“等待在途星图写入 + 提交后通知活跃星图重载”，不解析星图 JSON、不做同步合并。
// - 实际对象合并仍在 PeerObjectStore；本类不接触正文、不接触 GitHub 配置。
// - 没有活跃星图 / 本次没有星图路径时直接放行，不阻塞普通文件同步。

import { PeerObjectCommitResult } from '../../../platform/peer/PeerObjectStore'

const WRITE_IDLE_TIMEOUT_MS: number = 4000

// 活跃星图页面对一次附近设备提交暴露的能力：按受影响路径重载当前图。
export interface ActiveStarMapSession {
  readonly afterPeerCommit: (paths: string[], applied: number, conflicts: number) => Promise<void>
}

export function isPeerStarMapPath(path: string): boolean {
  return path.startsWith('app/starmaps/')
}

export class StarMapSyncBarrier {
  private static instance: StarMapSyncBarrier | null = null

  private sessions: ActiveStarMapSession[] = []
  private pendingWrites: number = 0
  private idleWaiters: Array<() => void> = []

  // 同步提交闸门：持有期间新的星图写入先等待，避免“检查完空闲又插进一笔写入”。
  private barrierHeld: boolean = false
  private barrierWaiters: Array<() => void> = []
  private writeWaiters: Array<() => void> = []

  static getInstance(): StarMapSyncBarrier {
    if (StarMapSyncBarrier.instance === null) {
      StarMapSyncBarrier.instance = new StarMapSyncBarrier()
    }
    return StarMapSyncBarrier.instance
  }

  register(session: ActiveStarMapSession): () => void {
    this.sessions.push(session)
    return (): void => {
      const index = this.sessions.indexOf(session)
      if (index >= 0) {
        this.sessions.splice(index, 1)
      }
    }
  }

  // 星图 CRUD 落盘前调用。若同步提交已持有闸门，先等它完成，不能把新写入插进合并中间。
  async beginWrite(): Promise<void> {
    while (this.barrierHeld) {
      await new Promise<void>((resolve: () => void): void => {
        this.writeWaiters.push(resolve)
      })
    }
    this.pendingWrites++
  }

  endWrite(): void {
    if (this.pendingWrites > 0) {
      this.pendingWrites--
    }
    if (this.pendingWrites === 0) {
      const waiters: Array<() => void> = this.idleWaiters
      this.idleWaiters = []
      waiters.forEach((resolve: () => void): void => resolve())
    }
  }

  /**
   * 一段附近设备对象提交：先封住新的星图写入，等在途写入结束，再执行 commit；
   * 只有 commit 真正应用了变更才通知活跃星图重载。失败 / 冲突不刷新，也不伪造成功。
   */
  async withSyncBarrier(paths: string[],
    action: () => Promise<PeerObjectCommitResult>): Promise<PeerObjectCommitResult> {
    if (!paths.some((path: string): boolean => isPeerStarMapPath(path))) {
      return action()
    }
    await this.acquireBarrier()
    try {
      const idle: boolean = await this.waitForIdle()
      if (!idle) {
        return {
          success: false,
          applied: 0,
          conflicts: 0,
          message: '星图仍有写入未完成，附近设备对象暂未提交'
        }
      }
      const result: PeerObjectCommitResult = await action()
      if (result.applied > 0 && this.sessions.length > 0) {
        const sessions: ActiveStarMapSession[] = this.sessions.slice()
        for (const session of sessions) {
          await session.afterPeerCommit(paths, result.applied, result.conflicts)
        }
      }
      return result
    } finally {
      this.releaseBarrier()
    }
  }

  private waitForIdle(): Promise<boolean> {
    if (this.pendingWrites === 0) {
      return Promise.resolve(true)
    }
    return new Promise<boolean>((resolve: (idle: boolean) => void): void => {
      let settled: boolean = false
      const finish = (idle: boolean): void => {
        if (settled) {
          return
        }
        settled = true
        resolve(idle)
      }
      this.idleWaiters.push((): void => finish(true))
      setTimeout((): void => finish(false), WRITE_IDLE_TIMEOUT_MS)
    })
  }

  private acquireBarrier(): Promise<void> {
    if (!this.barrierHeld) {
      this.barrierHeld = true
      return Promise.resolve()
    }
    return new Promise<void>((resolve: () => void): void => {
      this.barrierWaiters.push(resolve)
    })
  }

  private releaseBarrier(): void {
    const next: (() => void) | undefined = this.barrierWaiters.shift()
    if (next !== undefined) {
      // 所有权直接交给下一笔同步，保持 barrierHeld。
      next()
      return
    }
    this.barrierHeld = false
    const waitingWrites: Array<() => void> = this.writeWaiters
    this.writeWaiters = []
    waitingWrites.forEach((resolve: () => void): void => resolve())
  }
}
