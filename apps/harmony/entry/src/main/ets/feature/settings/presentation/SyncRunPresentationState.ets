// SyncRunPresentationState — 进程内同步运行展示快照，不替代 Core 持久状态。

import { SyncOutcome } from './SyncRunTypes'

export type SyncEntrySource = 'manual' | 'auto'
export type SyncDisplayPhase = 'idle' | 'in_progress' | 'success' | 'failed'

export interface SyncRunSnapshot {
  phase: SyncDisplayPhase
  source: SyncEntrySource
  startedAtMs: number | null
  finishedAtMs: number | null
  outcome: SyncOutcome | null
}

export type SyncRunListener = (snapshot: SyncRunSnapshot) => void

export class SyncRunPresentationState {
  private static instance: SyncRunPresentationState | null = null
  private current: SyncRunSnapshot = {
    phase: 'idle',
    source: 'manual',
    startedAtMs: null,
    finishedAtMs: null,
    outcome: null
  }
  private listeners: SyncRunListener[] = []

  private constructor() {
  }

  static getInstance(): SyncRunPresentationState {
    if (SyncRunPresentationState.instance === null) {
      SyncRunPresentationState.instance = new SyncRunPresentationState()
    }
    return SyncRunPresentationState.instance
  }

  snapshot(): SyncRunSnapshot {
    return this.copySnapshot(this.current)
  }

  subscribe(listener: SyncRunListener): () => void {
    this.listeners.push(listener)
    return (): void => {
      this.listeners = this.listeners.filter((current: SyncRunListener) => current !== listener)
    }
  }

  begin(source: SyncEntrySource): void {
    this.current = {
      phase: 'in_progress',
      source: source,
      startedAtMs: Date.now(),
      finishedAtMs: null,
      outcome: null
    }
    this.notifyListeners()
  }

  finish(outcome: SyncOutcome): void {
    const previous: SyncRunSnapshot = this.current
    this.current = {
      phase: outcome.phase === 'success' ? 'success' : 'failed',
      source: previous.source,
      startedAtMs: previous.startedAtMs,
      finishedAtMs: Date.now(),
      outcome: this.copyOutcome(outcome)
    }
    this.notifyListeners()
  }

  private notifyListeners(): void {
    const listeners: SyncRunListener[] = this.listeners.slice()
    for (const listener of listeners) {
      try {
        listener(this.snapshot())
      } catch (_) {
        // 一个销毁中的页面监听器不能阻断其他同步状态订阅者。
      }
    }
  }

  private copySnapshot(snapshot: SyncRunSnapshot): SyncRunSnapshot {
    return {
      phase: snapshot.phase,
      source: snapshot.source,
      startedAtMs: snapshot.startedAtMs,
      finishedAtMs: snapshot.finishedAtMs,
      outcome: snapshot.outcome === null ? null : this.copyOutcome(snapshot.outcome)
    }
  }

  private copyOutcome(outcome: SyncOutcome): SyncOutcome {
    const copy: SyncOutcome = { ...outcome }
    if (outcome.messageArgs !== undefined) {
      copy.messageArgs = { ...outcome.messageArgs }
    }
    return copy
  }
}
