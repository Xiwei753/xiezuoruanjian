// HarmonyDiagnosticsArchiveWorkerClient — ArkUI 线程侧的唯一 Worker 客户端。
//
// 负责 Worker 懒启动、requestId 关联、结果分发和 Ability 销毁时终止。所有
// 文件枚举/读取/压缩/发布都在 DiagnosticsArchiveWorker 内完成，本类只发送
// 小对象并等待终态。

import { worker, ErrorEvent, MessageEvents } from '@kit.ArkTS'
import hilog from '@ohos.hilog'

import {
  DiagnosticsArchiveRequest,
  DiagnosticsSnapshotRequest,
  DiagnosticsWorkerRequest,
  DiagnosticsWorkerResult
} from './DiagnosticsArchiveProtocol'

const LOG_TAG = 'DiagnosticsArchiveClient'
const WORKER_SCRIPT = 'entry/ets/platform/diagnostics/DiagnosticsArchiveWorker.ets'

type ResultResolver = (result: DiagnosticsWorkerResult) => void

interface PendingRequest {
  requestId: string
  resolve: ResultResolver
}

export class HarmonyDiagnosticsArchiveWorkerClient {
  private static instance: HarmonyDiagnosticsArchiveWorkerClient
  private threadWorker: worker.ThreadWorker | null = null
  private pending: Map<string, PendingRequest> = new Map()
  private sequence: number = 0

  private constructor() {}

  static getInstance(): HarmonyDiagnosticsArchiveWorkerClient {
    if (!HarmonyDiagnosticsArchiveWorkerClient.instance) {
      HarmonyDiagnosticsArchiveWorkerClient.instance = new HarmonyDiagnosticsArchiveWorkerClient()
    }
    return HarmonyDiagnosticsArchiveWorkerClient.instance
  }

  requestArchive(request: DiagnosticsArchiveRequest): Promise<DiagnosticsWorkerResult> {
    const requestId = this.nextRequestId()
    request.requestId = requestId
    return this.send(request, requestId)
  }

  requestSnapshot(runtimeDir: string, archiveStatePath: string, archiveStateJson: string,
    themeJson: string, uiJson: string, editorJson: string): Promise<DiagnosticsWorkerResult> {
    const requestId = this.nextRequestId()
    const request: DiagnosticsSnapshotRequest = {
      requestKind: 'snapshot',
      requestId: requestId,
      runtimeDir: runtimeDir,
      archiveStatePath: archiveStatePath,
      archiveStateJson: archiveStateJson,
      themeJson: themeJson,
      uiJson: uiJson,
      editorJson: editorJson
    }
    return this.send(request, requestId)
  }

  terminate(): void {
    const threadWorker = this.threadWorker
    this.threadWorker = null
    if (threadWorker !== null) {
      try {
        threadWorker.terminate()
      } catch (_) {
      }
    }
    this.resolveAllAsFailure('worker_terminated')
  }

  private send(request: DiagnosticsWorkerRequest, requestId: string): Promise<DiagnosticsWorkerResult> {
    const threadWorker = this.ensureWorker()
    if (threadWorker === null) {
      return Promise.resolve(this.failureResult(requestId, 'worker_unavailable'))
    }
    return new Promise<DiagnosticsWorkerResult>((resolve: ResultResolver): void => {
      const entry: PendingRequest = { requestId: requestId, resolve: resolve }
      this.pending.set(requestId, entry)
      try {
        threadWorker.postMessage(request)
      } catch (e) {
        this.pending.delete(requestId)
        resolve(this.failureResult(requestId, e instanceof Error ? e.name : 'worker_post_failed'))
      }
    })
  }

  private ensureWorker(): worker.ThreadWorker | null {
    if (this.threadWorker !== null) {
      return this.threadWorker
    }
    try {
      const threadWorker = new worker.ThreadWorker(WORKER_SCRIPT)
      threadWorker.onmessage = (event: MessageEvents): void => {
        const result = event.data as DiagnosticsWorkerResult
        if (result === null || result.requestId.length === 0) {
          return
        }
        const entry = this.pending.get(result.requestId)
        if (entry !== undefined) {
          this.pending.delete(result.requestId)
          entry.resolve(result)
        }
      }
      threadWorker.onerror = (event: ErrorEvent): void => {
        hilog.error(0x0000, LOG_TAG, 'archive worker error: %{public}s', event.message)
        this.handleWorkerExit('worker_error')
      }
      threadWorker.onexit = (code: number): void => {
        if (code !== 0) {
          hilog.warn(0x0000, LOG_TAG, 'archive worker exited with code %{public}d', code)
        }
        this.handleWorkerExit('worker_exited')
      }
      this.threadWorker = threadWorker
      return threadWorker
    } catch (e) {
      hilog.error(0x0000, LOG_TAG, 'cannot start archive worker: %{public}s',
        e instanceof Error ? e.name : 'worker_start_failed')
      this.threadWorker = null
      return null
    }
  }

  private handleWorkerExit(reason: string): void {
    const exitedWorker = this.threadWorker
    this.threadWorker = null
    if (exitedWorker !== null) {
      try {
        exitedWorker.terminate()
      } catch (_) {
      }
    }
    this.resolveAllAsFailure(reason)
  }

  private resolveAllAsFailure(reason: string): void {
    const entries: PendingRequest[] = []
    this.pending.forEach((entry: PendingRequest): void => {
      entries.push(entry)
    })
    this.pending.clear()
    for (const entry of entries) {
      entry.resolve(this.failureResult(entry.requestId, reason))
    }
  }

  private failureResult(requestId: string, reason: string): DiagnosticsWorkerResult {
    return {
      requestId: requestId,
      success: false,
      stage: 'worker',
      message: reason,
      archivePath: '',
      durationMs: 0,
      mergedRequestIds: []
    }
  }

  private nextRequestId(): string {
    this.sequence += 1
    return `diag-${Date.now()}-${this.sequence}`
  }
}
