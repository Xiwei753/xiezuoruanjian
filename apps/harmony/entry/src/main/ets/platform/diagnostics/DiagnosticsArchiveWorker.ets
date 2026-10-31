// DiagnosticsArchiveWorker — 诊断归档的唯一执行线程。
//
// 主线程只发送路径、白名单附件文本和稳定元数据。本 Worker 串行执行文件枚举、
// 日志读取与脱敏、故障附件复制、manifest 写入、ZIP 压缩、staging 清理和
// latest/previous 原子发布；任何阶段失败都返回稳定 stage，不把原始路径或内容
// 写进日志。
//
// 同一归档用途在排队期间合并；每次请求仍会收到自己的 requestId 终态。

import { worker, util, MessageEvents, ThreadWorkerGlobalScope } from '@kit.ArkTS'
import fs from '@ohos.file.fs'
import hilog from '@ohos.hilog'

import { HarmonyDiagnosticsArchive } from './HarmonyDiagnosticsArchive'
import {
  DiagnosticsArchiveAttachment,
  DiagnosticsArchiveManifestMeta,
  DiagnosticsArchiveRequest,
  DiagnosticsArchiveStage,
  DiagnosticsFaultCaptureRequest,
  DiagnosticsFaultKeysRequest,
  DiagnosticsFaultRecoveryRequest,
  DiagnosticsFileCopyRequest,
  DiagnosticsHeavyRequest,
  DiagnosticsSnapshotRequest,
  DiagnosticsWorkerRequest,
  DiagnosticsWorkerResult
} from './DiagnosticsArchiveProtocol'

const LOG_TAG = 'DiagnosticsArchiveWorker'
const LOG_FILENAME_PATTERN = /^sujian-current-[A-Za-z0-9._-]+\.log$/
const CRASH_FILE_PATTERN = /^fault-[0-9]+\.log$/
const EVENT_KEY_PATTERN = /^[A-Za-z0-9_-]+$/
const FAULT_COMPLETE_FILE = 'complete.json'
const FAULT_STATE_FILE = 'fault-state.json'
const FILE_COPY_CHUNK_BYTES = 256 * 1024

const workerPort: ThreadWorkerGlobalScope = worker.workerPort

interface LogSummary {
  status: string
  file_count: number
  files: string[]
}

interface ManifestCollection {
  logs: LogSummary
  attachments: string[]
  crash_events: string[]
}

interface DiagnosticsManifest {
  schema_version: number
  platform: string
  archive_kind: string
  data_version: string
  build_identity: string
  app_version: string
  git_sha: string
  exported_at: string
  event_key: string
  collection: ManifestCollection
  // 仅 fault 包写入；用于说明附件是否完整以及系统是否因 log_over_limit 截断。
  fault_status?: string
  expected_external_log_count?: number
  confirmed_external_log_count?: number
  log_over_limit?: boolean
  // full/checkpoint 包中未能读入 staging 的历史故障附件；如实反映缺口。
  missing_crash_attachments?: string[]
}

interface CrashCandidate {
  eventKey: string
  mtime: number
}

interface CrashAttachmentCopyResult {
  eventKeys: string[]
  missing: string[]
}

interface FaultPublishProgress {
  stage: DiagnosticsArchiveStage
}

interface FaultAttachmentEntry {
  fileName: string
  complete: boolean
  size: number
}

// 事件目录内的持久附件状态。它只记录数量/文件名/大小/完成位，不保存系统路径，
// 是 complete.json 是否可以写入的唯一依据。
interface FaultAttachmentState {
  schemaVersion: number
  eventKey: string
  expectedExternalLogCount: number
  confirmedExternalLogCount: number
  logOverLimit: boolean
  updatedAtMs: number
  attachments: FaultAttachmentEntry[]
}

interface FaultCompletion {
  schemaVersion: number
  eventKey: string
  archivePath: string
  archiveSize: number
  completedAtMs: number
  expectedExternalLogCount: number
  confirmedExternalLogCount: number
}

interface PendingOperation {
  request: DiagnosticsHeavyRequest
  requestIds: string[]
  mergeKey: string
}

let operationQueue: PendingOperation[] = []
let processingOperation: boolean = false
let pendingSnapshot: DiagnosticsSnapshotRequest | null = null
let pendingSnapshotIds: string[] = []
let processingSnapshot: boolean = false

workerPort.onmessage = (event: MessageEvents): void => {
  const request = event.data as DiagnosticsWorkerRequest
  if (request.requestKind === 'snapshot') {
    enqueueSnapshot(request)
  } else if (request.requestKind === 'archive') {
    enqueueOperation(request, `archive:${request.archiveKind}:${request.eventKey}`)
  } else if (request.requestKind === 'fault_capture') {
    enqueueOperation(request, `fault_capture:${request.eventKey}`)
  } else if (request.requestKind === 'fault_recovery') {
    enqueueOperation(request, 'fault_recovery')
  } else if (request.requestKind === 'fault_keys') {
    enqueueOperation(request, 'fault_keys')
  } else if (request.requestKind === 'file_copy') {
    enqueueOperation(request, `file_copy:${request.sourcePath}:${request.targetPath}`)
  }
}

// 故障落盘、归档压缩和导出包复制共用同一个串行队列：调用方看到的每个
// requestId 都会收到一次终态，等价的待处理请求只保留最新 payload。
function enqueueOperation(request: DiagnosticsHeavyRequest, mergeKey: string): void {
  for (const pending of operationQueue) {
    if (pending.mergeKey === mergeKey) {
      pending.request = request
      pending.requestIds.push(request.requestId)
      return
    }
  }
  const pending: PendingOperation = {
    request: request,
    requestIds: [request.requestId],
    mergeKey: mergeKey
  }
  operationQueue.push(pending)
  void drainOperationQueue()
}

async function drainOperationQueue(): Promise<void> {
  if (processingOperation) {
    return
  }
  processingOperation = true
  try {
    while (operationQueue.length > 0) {
      const pending = operationQueue.shift()
      if (pending === undefined) {
        continue
      }
      const result = await runHeavyOperation(pending.request)
      postOperationResult(result, pending.requestIds)
    }
  } finally {
    processingOperation = false
  }
}

async function runHeavyOperation(request: DiagnosticsHeavyRequest): Promise<DiagnosticsWorkerResult> {
  if (request.requestKind === 'archive') {
    return runArchive(request)
  }
  if (request.requestKind === 'fault_capture') {
    return runFaultCapture(request)
  }
  if (request.requestKind === 'fault_recovery') {
    return runFaultRecovery(request)
  }
  if (request.requestKind === 'fault_keys') {
    return runFaultKeys(request)
  }
  return runFileCopy(request)
}

function enqueueSnapshot(request: DiagnosticsSnapshotRequest): void {
  // 高频编辑器/窗口变化只保留最后一次完整 payload，旧 requestId 一起合并。
  if (pendingSnapshot === null) {
    pendingSnapshotIds = []
  }
  pendingSnapshot = request
  pendingSnapshotIds.push(request.requestId)
  if (!processingSnapshot) {
    void drainSnapshots()
  }
}

async function drainSnapshots(): Promise<void> {
  processingSnapshot = true
  try {
    while (pendingSnapshot !== null) {
      const request = pendingSnapshot
      const requestIds = pendingSnapshotIds
      pendingSnapshot = null
      pendingSnapshotIds = []
      const result = runSnapshot(request)
      for (const requestId of requestIds) {
        postSnapshotResult(result, requestId)
      }
    }
  } finally {
    processingSnapshot = false
    // 在 finally 与最后一次 enqueue 之间存在极短窗口；再检查一次避免丢请求。
    if (pendingSnapshot !== null) {
      void drainSnapshots()
    }
  }
}

function postOperationResult(result: DiagnosticsWorkerResult, requestIds: string[]): void {
  for (const requestId of requestIds) {
    const response: DiagnosticsWorkerResult = {
      requestId: requestId,
      success: result.success,
      stage: result.stage,
      message: result.message,
      archivePath: result.archivePath,
      durationMs: result.durationMs,
      mergedRequestIds: requestIds
    }
    if (result.eventKeys !== undefined) {
      response.eventKeys = result.eventKeys
    }
    workerPort.postMessage(response)
  }
}

function postSnapshotResult(result: DiagnosticsWorkerResult, requestId: string): void {
  const response: DiagnosticsWorkerResult = {
    requestId: requestId,
    success: result.success,
    stage: result.stage,
    message: result.message,
    archivePath: result.archivePath,
    durationMs: result.durationMs,
    mergedRequestIds: [requestId]
  }
  workerPort.postMessage(response)
}

async function runArchive(request: DiagnosticsArchiveRequest): Promise<DiagnosticsWorkerResult> {
  const startedAt = Date.now()
  let stage: DiagnosticsArchiveStage = 'staging_cleanup'
  let archivePath = ''
  try {
    cleanStaleStaging(request.outputDir)
    const stagingDir = `${request.outputDir}/.staging-${request.archiveKind}-${Date.now()}-${Math.floor(Math.random() * 1000000)}`
    HarmonyDiagnosticsArchive.removeTree(stagingDir)
    ensureDirectory(stagingDir)

    stage = 'log_copy'
    const logSummary = copyPersistedLogs(request.logDir, stagingDir)

    stage = 'attachment_write'
    const attachmentNames: string[] = []
    for (const attachment of request.attachments) {
      writeAttachment(stagingDir, attachment)
      attachmentNames.push(attachment.relativePath)
    }

    stage = request.archiveKind === 'fault' ? 'fault_archive_attachment_copy' : 'crash_copy'
    const crashCopy = copyCrashAttachments(
      request.crashesDir, stagingDir, request.archiveKind, request.eventKey, request.recentCrashLimit
    )

    stage = 'manifest_write'
    const manifest = buildManifest(request, logSummary, attachmentNames, crashCopy.eventKeys)
    if (crashCopy.missing.length > 0) {
      manifest.missing_crash_attachments = crashCopy.missing
    }
    HarmonyDiagnosticsArchive.writeTextFile(
      stagingDir, 'diagnostics_manifest.json', JSON.stringify(manifest, null, 2)
    )

    stage = 'compress'
    if (request.archiveKind === 'checkpoint') {
      archivePath = await publishCheckpoint(stagingDir, request.outputDir)
    } else if (request.archiveKind === 'fault') {
      archivePath = await HarmonyDiagnosticsArchive.compressDirectory(
        stagingDir, request.outputDir, request.fileStem, true
      )
    } else {
      archivePath = await HarmonyDiagnosticsArchive.compressDirectory(
        stagingDir, request.outputDir, request.fileStem
      )
    }

    HarmonyDiagnosticsArchive.removeTree(stagingDir)
    return {
      requestId: request.requestId,
      success: true,
      stage: 'publish',
      message: '',
      archivePath: archivePath,
      durationMs: Date.now() - startedAt,
      mergedRequestIds: []
    }
  } catch (e) {
    return {
      requestId: request.requestId,
      success: false,
      stage: stage,
      message: e instanceof Error ? e.name : 'archive_failed',
      archivePath: archivePath,
      durationMs: Date.now() - startedAt,
      mergedRequestIds: []
    }
  }
}

// 启动去重只做目录级扫描，不读取 params/external_log，也不触发压缩。
function runFaultKeys(request: DiagnosticsFaultKeysRequest): DiagnosticsWorkerResult {
  const startedAt = Date.now()
  const keys: string[] = []
  try {
    if (fileExists(request.crashesDir)) {
      for (const entry of fs.listFileSync(request.crashesDir)) {
        if (!EVENT_KEY_PATTERN.test(entry)) {
          continue
        }
        const eventDir = `${request.crashesDir}/${entry}`
        // 只有真正发布完成的故障才参与持久去重；params.json 只表示"事件已收到"，
        // 未完成事件必须保留给本次回放/恢复继续复制 external_log 和压缩。
        if (isFaultComplete(eventDir)) {
          keys.push(entry)
        }
      }
    }
    return {
      requestId: request.requestId,
      success: true,
      stage: 'fault_keys',
      message: '',
      archivePath: '',
      durationMs: Date.now() - startedAt,
      mergedRequestIds: [],
      eventKeys: keys
    }
  } catch (e) {
    return failureResult(request.requestId, 'fault_keys', errorName(e), '', startedAt)
  }
}

// 故障事件：Worker 内完成 crashes/<eventKey> 建目录、params 原子写入、
// external_log 分块复制/清理、fault ZIP 发布和 complete.json 完成标记。
// params.json 只是原始事件快照；fault-state.json 记录每个附件的预期大小和确认
// 状态；只有 confirmed >= expected 时才允许发布 fault ZIP 并写 complete.json。
async function runFaultCapture(request: DiagnosticsFaultCaptureRequest): Promise<DiagnosticsWorkerResult> {
  const startedAt = Date.now()
  let stage: DiagnosticsArchiveStage = 'fault_prepare'
  const eventKey = sanitizeEventKey(request.eventKey)
  if (!EVENT_KEY_PATTERN.test(eventKey)) {
    return failureResult(request.requestId, stage, 'invalid_event_key', '', startedAt)
  }
  const eventDir = `${request.crashesDir}/${eventKey}`
  const paramsPath = `${eventDir}/params.json`
  const completePath = `${eventDir}/${FAULT_COMPLETE_FILE}`
  try {
    ensureDirectory(request.crashesDir)
    ensureDirectory(eventDir)
    if (isFaultComplete(eventDir)) {
      const completion = readFaultCompletion(completePath)
      return successResult(request.requestId, 'fault_complete',
        completion === null ? '' : completion.archivePath, startedAt)
    }
    // 有 complete.json 但 ZIP 已被清理或附件不完整时，删除旧标记并重新收尾。
    if (fileExists(completePath)) {
      safeDeleteFile(completePath)
    }

    if (!fileExists(paramsPath)) {
      if (request.paramsJson.length === 0) {
        throw new Error('missing fault params')
      }
      atomicWrite(eventDir, 'params.json', request.paramsJson)
    }

    const requestedExpected = Math.max(
      normalizeCount(request.expectedExternalLogCount),
      normalizeCount(request.externalLogPaths.length)
    )
    let state = readFaultAttachmentState(eventDir, eventKey)
    if (state === null) {
      state = createFaultAttachmentState(eventKey, requestedExpected, request.logOverLimit)
    } else {
      const expected = Math.max(state.expectedExternalLogCount, requestedExpected)
      state.expectedExternalLogCount = expected
      state.logOverLimit = state.logOverLimit || request.logOverLimit
      ensureFaultAttachmentEntries(state)
    }
    writeFaultAttachmentState(eventDir, state)

    stage = 'fault_log_copy'
    for (let index = 0; index < request.externalLogPaths.length; index++) {
      copyExternalFaultLog(request.externalLogPaths[index], eventDir, index, state)
    }
    refreshFaultAttachmentState(eventDir, state)
    if (state.confirmedExternalLogCount < state.expectedExternalLogCount) {
      // 不生成普通 fault ZIP，也不写 complete.json；保留 params + fault-state
      // 给系统后续回放或下一次启动恢复。missing source / 复制中断都落在这里。
      return failureResult(request.requestId, 'fault_log_copy',
        `fault_attachments_incomplete:${state.confirmedExternalLogCount}/${state.expectedExternalLogCount}`,
        '', startedAt)
    }

    stage = 'fault_compress'
    const progress: FaultPublishProgress = { stage: 'fault_compress' }
    let archivePath = ''
    try {
      archivePath = await publishFaultArchive(
        request.outputDir, request.logDir, request.crashesDir, request.manifest, eventKey, state, progress
      )
    } catch (e) {
      return failureResult(request.requestId, progress.stage, errorName(e), '', startedAt)
    }

    stage = 'fault_complete'
    writeFaultCompletion(eventDir, eventKey, archivePath, state)
    return successResult(request.requestId, 'fault_complete', archivePath, startedAt)
  } catch (e) {
    return failureResult(request.requestId, stage, errorName(e), '', startedAt)
  }
}

// 启动补全：上次进程如果在复制/压缩/写 complete.json 之间被杀，这里从已经
// 落盘的 params.json + fault-state.json + fault-N.log 继续。只有 fault-state
// 中已确认的附件数量达到预期才发布 ZIP；fault-state 丢失/损坏时，即使目录里
// 已有非空 fault-N.log，也不能凭 stat.size 推断完整，必须保持 incomplete 并
// 等待同一事件的系统回放重新提供 external_log。
async function runFaultRecovery(request: DiagnosticsFaultRecoveryRequest): Promise<DiagnosticsWorkerResult> {
  const startedAt = Date.now()
  let stage: DiagnosticsArchiveStage = 'fault_recovery'
  let failureStage: DiagnosticsArchiveStage = 'fault_recovery'
  let incomplete = 0
  let failed = 0
  try {
    if (!fileExists(request.crashesDir)) {
      return successResult(request.requestId, 'fault_complete', '', startedAt)
    }
    for (const entry of fs.listFileSync(request.crashesDir)) {
      if (!EVENT_KEY_PATTERN.test(entry)) {
        continue
      }
      const eventDir = `${request.crashesDir}/${entry}`
      try {
        if (!fs.statSync(eventDir).isDirectory()) {
          continue
        }
      } catch (_) {
        continue
      }
      if (!fileExists(`${eventDir}/params.json`) || isFaultComplete(eventDir)) {
        continue
      }
      let eventFailureStage: DiagnosticsArchiveStage = 'fault_recovery'
      stage = 'fault_recovery'
      try {
        const completePath = `${eventDir}/${FAULT_COMPLETE_FILE}`
        if (fileExists(completePath)) {
          safeDeleteFile(completePath)
        }
        let state = readFaultAttachmentState(eventDir, entry)
        if (state === null) {
          // 旧版本没有 fault-state.json：只能从 params.params.external_log_count
          // 恢复预期数量；没有该字段就保持 incomplete，绝不把 partial 包标记完成。
          const expected = readExternalLogCountFromParams(eventDir)
          if (expected < 0) {
            incomplete += 1
            continue
          }
          // 旧版本曾直接 TRUNC 写正式 fault-N.log，半截文件同样非空。没有可信的
          // 复制完成记录时，不能仅凭当前 stat.size 把附件升级为 complete；保留
          // 预期数量并等待同一事件回放重新复制，或由人工诊断包报告缺失。
          state = createFaultAttachmentState(entry, expected, false)
          writeFaultAttachmentState(eventDir, state)
        } else {
          const expected = readExternalLogCountFromParams(eventDir)
          if (expected > state.expectedExternalLogCount) {
            state.expectedExternalLogCount = expected
            ensureFaultAttachmentEntries(state)
          }
        }
        refreshFaultAttachmentState(eventDir, state)
        if (state.confirmedExternalLogCount < state.expectedExternalLogCount) {
          incomplete += 1
          continue
        }
        stage = 'fault_compress'
        const progress: FaultPublishProgress = { stage: 'fault_compress' }
        let archivePath = ''
        try {
          archivePath = await publishFaultArchive(
            request.outputDir, request.logDir, request.crashesDir, request.manifest, entry, state, progress
          )
        } catch (e) {
          eventFailureStage = progress.stage
          throw new Error('fault publish failed')
        }
        stage = 'fault_complete'
        failureStage = 'fault_complete'
        writeFaultCompletion(eventDir, entry, archivePath, state)
      } catch (e) {
        failed += 1
        failureStage = eventFailureStage
        hilog.warn(0x0000, LOG_TAG, 'fault recovery event failed: stage=%{public}s reason=%{public}s',
          eventFailureStage, errorName(e))
      }
    }
    if (failed > 0) {
      return failureResult(request.requestId, failureStage,
        `fault_recovery_failed:${failed}`, '', startedAt)
    }
    if (incomplete > 0) {
      return failureResult(request.requestId, 'fault_log_copy',
        `fault_recovery_incomplete:${incomplete}`, '', startedAt)
    }
    return successResult(request.requestId, 'fault_complete', '', startedAt)
  } catch (e) {
    return failureResult(request.requestId, stage, errorName(e), '', startedAt)
  }
}

async function publishFaultArchive(outputDir: string, logDir: string, crashesDir: string,
  meta: DiagnosticsArchiveManifestMeta, eventKey: string,
  state: FaultAttachmentState, progress: FaultPublishProgress): Promise<string> {
  const stagingDir = `${outputDir}/.staging-fault-${eventKey}-${Date.now()}-${Math.floor(Math.random() * 1000000)}`
  try {
    progress.stage = 'staging_cleanup'
    cleanStaleStaging(outputDir)
    HarmonyDiagnosticsArchive.removeTree(stagingDir)
    ensureDirectory(stagingDir)
    progress.stage = 'log_copy'
    const logSummary = copyPersistedLogs(logDir, stagingDir)
    // fault 包不能沿用非关键的 crash_copy：params、state 和每份已确认日志
    // 必须真实进入 staging，否则 complete.json 的含义就不成立。
    progress.stage = 'fault_archive_attachment_copy'
    const crashEvents = copyFaultAttachmentsStrict(crashesDir, stagingDir, eventKey, state)
    progress.stage = 'manifest_write'
    const manifest = buildManifestFrom(meta, 'fault', eventKey, logSummary, [], crashEvents)
    manifest.fault_status = 'complete'
    manifest.expected_external_log_count = state.expectedExternalLogCount
    manifest.confirmed_external_log_count = state.confirmedExternalLogCount
    manifest.log_over_limit = state.logOverLimit
    HarmonyDiagnosticsArchive.writeTextFile(
      stagingDir, 'diagnostics_manifest.json', JSON.stringify(manifest, null, 2)
    )
    progress.stage = 'fault_compress'
    return await HarmonyDiagnosticsArchive.compressDirectory(
      stagingDir, outputDir, `sujian-diagnostics-fault-${eventKey}`, true
    )
  } finally {
    HarmonyDiagnosticsArchive.removeTree(stagingDir)
  }
}

// fault 包的严格附件投影：params.json、fault-state.json 和 state.attachments 中
// 每个已确认的 fault-N.log 都必须成功写入 staging；任何缺失/读失败/空文件都
// 抛出，阻止 manifest、压缩和 complete.json。
function copyFaultAttachmentsStrict(crashesDir: string, stagingDir: string,
  eventKey: string, state: FaultAttachmentState): string[] {
  if (!EVENT_KEY_PATTERN.test(eventKey)) {
    throw new Error('fault event key invalid')
  }
  const eventDir = `${crashesDir}/${eventKey}`
  if (!fileExists(eventDir)) {
    throw new Error('fault event directory missing')
  }
  try {
    if (!fs.statSync(eventDir).isDirectory()) {
      throw new Error('fault event path is not a directory')
    }
  } catch (e) {
    throw new Error('fault event directory unreadable')
  }
  if (state.attachments.length !== state.expectedExternalLogCount) {
    throw new Error('fault attachment state count mismatch')
  }
  if (state.confirmedExternalLogCount < state.expectedExternalLogCount) {
    throw new Error('fault attachments incomplete')
  }
  copyFaultFileStrict(eventDir, stagingDir, eventKey, 'params.json', -1)
  copyFaultFileStrict(eventDir, stagingDir, eventKey, FAULT_STATE_FILE, -1)
  for (const entry of state.attachments) {
    if (!entry.complete) {
      throw new Error('fault attachment not confirmed')
    }
    copyFaultFileStrict(eventDir, stagingDir, eventKey, entry.fileName, entry.size)
  }
  return [eventKey]
}

function copyFaultFileStrict(eventDir: string, stagingDir: string, eventKey: string,
  fileName: string, expectedSize: number): void {
  const sourcePath = `${eventDir}/${fileName}`
  if (!fileExists(sourcePath)) {
    throw new Error('fault attachment missing')
  }
  let sourceSize = 0
  try {
    const sourceStat = fs.statSync(sourcePath)
    if (!sourceStat.isFile()) {
      throw new Error('fault attachment is not a file')
    }
    sourceSize = sourceStat.size
  } catch (e) {
    throw new Error('fault attachment unreadable')
  }
  if (expectedSize >= 0 && sourceSize !== expectedSize) {
    throw new Error('fault attachment size mismatch')
  }
  if (sourceSize === 0 && expectedSize !== 0) {
    throw new Error('fault attachment empty')
  }
  let content = ''
  try {
    content = readTextFile(sourcePath)
  } catch (e) {
    throw new Error('fault attachment read failed')
  }
  try {
    HarmonyDiagnosticsArchive.writeTextFile(
      stagingDir, `crashes/${eventKey}/${fileName}`, content
    )
  } catch (e) {
    throw new Error('fault attachment staging write failed')
  }
  const stagedPath = `${stagingDir}/crashes/${eventKey}/${fileName}`
  try {
    const stagedStat = fs.statSync(stagedPath)
    if (!stagedStat.isFile()) {
      throw new Error('fault attachment staging missing')
    }
    if (expectedSize === 0 && stagedStat.size !== 0) {
      throw new Error('fault attachment staging size mismatch')
    }
    if (expectedSize !== 0 && stagedStat.size === 0) {
      throw new Error('fault attachment staging empty')
    }
  } catch (e) {
    throw new Error('fault attachment staging verification failed')
  }
}

// external_log 可能是大文件：先写 fault-N.log.tmp，校验源/目标字节数并关闭句柄，
// 原子 rename 为正式文件后才删除系统源日志。中断只留下 .tmp，不会污染 final。
function copyExternalFaultLog(sourcePath: string, eventDir: string, index: number,
  state: FaultAttachmentState): void {
  const fileName = `fault-${index + 1}.log`
  const targetPath = `${eventDir}/${fileName}`
  const tempPath = `${targetPath}.tmp`
  const entry = findOrCreateAttachmentEntry(state, fileName)
  if (entry.complete && isConfirmedAttachment(targetPath, entry.size)) {
    try {
      if (fileExists(sourcePath)) {
        fs.unlinkSync(sourcePath)
      }
    } catch (_) {
    }
    return
  }
  if (sourcePath.length === 0 || !fileExists(sourcePath)) {
    return
  }
  let sourceSize = 0
  try {
    const sourceStat = fs.statSync(sourcePath)
    if (!sourceStat.isFile()) {
      return
    }
    sourceSize = sourceStat.size
  } catch (_) {
    return
  }

  entry.complete = false
  entry.size = sourceSize
  writeFaultAttachmentState(eventDir, state)
  safeDeleteFile(tempPath)
  try {
    copyFileChunked(sourcePath, tempPath)
    if (fs.statSync(tempPath).size !== sourceSize) {
      throw new Error('fault log copy size mismatch')
    }
    // 先写完成状态再 rename：如果进程在两者之间被杀，refresh 会用同尺寸 .tmp
    // 补做 rename；如果 rename 失败则回退 complete=false。
    entry.complete = true
    entry.size = sourceSize
    writeFaultAttachmentState(eventDir, state)
    try {
      replaceFile(tempPath, targetPath)
    } catch (e) {
      entry.complete = false
      writeFaultAttachmentState(eventDir, state)
      safeDeleteFile(tempPath)
      throw new Error('fault log rename failed')
    }
    writeFaultAttachmentState(eventDir, state)
    try {
      fs.unlinkSync(sourcePath)
    } catch (_) {
      hilog.warn(0x0000, LOG_TAG, 'unable to remove copied external fault log source')
    }
  } catch (_) {
    entry.complete = false
    writeFaultAttachmentState(eventDir, state)
    safeDeleteFile(tempPath)
  }
}

function createFaultAttachmentState(eventKey: string, expected: number,
  logOverLimit: boolean): FaultAttachmentState {
  const state: FaultAttachmentState = {
    schemaVersion: 2,
    eventKey: eventKey,
    expectedExternalLogCount: normalizeCount(expected),
    confirmedExternalLogCount: 0,
    logOverLimit: logOverLimit,
    updatedAtMs: Date.now(),
    attachments: []
  }
  ensureFaultAttachmentEntries(state)
  return state
}

function ensureFaultAttachmentEntries(state: FaultAttachmentState): void {
  while (state.attachments.length < state.expectedExternalLogCount) {
    const entry: FaultAttachmentEntry = {
      fileName: `fault-${state.attachments.length + 1}.log`,
      complete: false,
      size: 0
    }
    state.attachments.push(entry)
  }
}

function findOrCreateAttachmentEntry(state: FaultAttachmentState,
  fileName: string): FaultAttachmentEntry {
  for (const entry of state.attachments) {
    if (entry.fileName === fileName) {
      return entry
    }
  }
  const created: FaultAttachmentEntry = { fileName: fileName, complete: false, size: 0 }
  state.attachments.push(created)
  return created
}

function readFaultAttachmentState(eventDir: string,
  eventKey: string): FaultAttachmentState | null {
  const path = `${eventDir}/${FAULT_STATE_FILE}`
  if (!fileExists(path)) {
    return null
  }
  try {
    const parsed = JSON.parse(readTextFile(path)) as Record<string, Object>
    const schemaVersion = readIntegerField(parsed, 'schemaVersion', -1)
    const expected = readIntegerField(parsed, 'expectedExternalLogCount', -1)
    if (schemaVersion < 2 || expected < 0 || readStringField(parsed, 'eventKey') !== eventKey) {
      return null
    }
    const attachments: FaultAttachmentEntry[] = []
    const rawAttachments = parsed['attachments']
    if (Array.isArray(rawAttachments)) {
      const entries = rawAttachments as Object[]
      for (const item of entries) {
        if (item === null || typeof item !== 'object') {
          continue
        }
        const record = item as Record<string, Object>
        const fileName = readStringField(record, 'fileName')
        const size = readIntegerField(record, 'size', -1)
        if (!CRASH_FILE_PATTERN.test(fileName) || size < 0) {
          continue
        }
        const entry: FaultAttachmentEntry = {
          fileName: fileName,
          complete: record['complete'] === true,
          size: size
        }
        attachments.push(entry)
      }
    }
    const state: FaultAttachmentState = {
      schemaVersion: 2,
      eventKey: eventKey,
      expectedExternalLogCount: expected,
      confirmedExternalLogCount: 0,
      logOverLimit: parsed['logOverLimit'] === true,
      updatedAtMs: readIntegerField(parsed, 'updatedAtMs', 0),
      attachments: attachments
    }
    ensureFaultAttachmentEntries(state)
    return state
  } catch (_) {
    return null
  }
}

function writeFaultAttachmentState(eventDir: string, state: FaultAttachmentState): void {
  state.confirmedExternalLogCount = countConfirmedAttachments(eventDir, state)
  state.updatedAtMs = Date.now()
  atomicWrite(eventDir, FAULT_STATE_FILE, JSON.stringify(state, null, 2))
}

// 恢复时修复"已写 complete=true、rename 前被杀"的窗口：同尺寸 .tmp 可以补做
// rename。没有 .tmp 且 final 缺失/尺寸不符的条目会回退为未完成。
function refreshFaultAttachmentState(eventDir: string, state: FaultAttachmentState): void {
  for (const entry of state.attachments) {
    if (!entry.complete) {
      continue
    }
    const targetPath = `${eventDir}/${entry.fileName}`
    if (isConfirmedAttachment(targetPath, entry.size)) {
      continue
    }
    const tempPath = `${targetPath}.tmp`
    if (isConfirmedAttachment(tempPath, entry.size)) {
      try {
        replaceFile(tempPath, targetPath)
        continue
      } catch (_) {
      }
    }
    entry.complete = false
  }
  writeFaultAttachmentState(eventDir, state)
}

function countConfirmedAttachments(eventDir: string, state: FaultAttachmentState): number {
  let confirmed = 0
  for (const entry of state.attachments) {
    if (entry.complete && isConfirmedAttachment(`${eventDir}/${entry.fileName}`, entry.size)) {
      confirmed += 1
    }
  }
  return confirmed
}

function isConfirmedAttachment(path: string, expectedSize: number): boolean {
  try {
    if (!fileExists(path)) {
      return false
    }
    const stat = fs.statSync(path)
    return stat.isFile() && stat.size === expectedSize
  } catch (_) {
    return false
  }
}

function replaceFile(sourcePath: string, targetPath: string): void {
  try {
    fs.renameSync(sourcePath, targetPath)
  } catch (_) {
    safeDeleteFile(targetPath)
    fs.renameSync(sourcePath, targetPath)
  }
}

function isFaultComplete(eventDir: string): boolean {
  const eventKey = fileName(eventDir)
  const completion = readFaultCompletion(`${eventDir}/${FAULT_COMPLETE_FILE}`)
  if (completion === null || completion.schemaVersion < 2 || completion.eventKey !== eventKey) {
    return false
  }
  const state = readFaultAttachmentState(eventDir, eventKey)
  if (state === null) {
    return false
  }
  refreshFaultAttachmentState(eventDir, state)
  if (state.expectedExternalLogCount !== completion.expectedExternalLogCount ||
    state.attachments.length !== state.expectedExternalLogCount ||
    state.confirmedExternalLogCount < state.expectedExternalLogCount ||
    completion.confirmedExternalLogCount < completion.expectedExternalLogCount) {
    return false
  }
  if (fileName(completion.archivePath) !== `sujian-diagnostics-fault-${eventKey}.zip`) {
    return false
  }
  try {
    if (!fileExists(completion.archivePath)) {
      return false
    }
    const stat = fs.statSync(completion.archivePath)
    return stat.isFile() && stat.size > 0 && stat.size === completion.archiveSize
  } catch (_) {
    return false
  }
}

function readFaultCompletion(path: string): FaultCompletion | null {
  if (!fileExists(path)) {
    return null
  }
  try {
    const parsed = JSON.parse(readTextFile(path)) as Record<string, Object>
    const completion: FaultCompletion = {
      schemaVersion: readIntegerField(parsed, 'schemaVersion', -1),
      eventKey: readStringField(parsed, 'eventKey'),
      archivePath: readStringField(parsed, 'archivePath'),
      archiveSize: readIntegerField(parsed, 'archiveSize', -1),
      completedAtMs: readIntegerField(parsed, 'completedAtMs', -1),
      expectedExternalLogCount: readIntegerField(parsed, 'expectedExternalLogCount', -1),
      confirmedExternalLogCount: readIntegerField(parsed, 'confirmedExternalLogCount', -1)
    }
    if (completion.archivePath.length === 0 || completion.archiveSize <= 0 ||
      completion.expectedExternalLogCount < 0 || completion.confirmedExternalLogCount < 0) {
      return null
    }
    return completion
  } catch (_) {
    return null
  }
}

function writeFaultCompletion(eventDir: string, eventKey: string, archivePath: string,
  state: FaultAttachmentState): void {
  if (state.confirmedExternalLogCount < state.expectedExternalLogCount) {
    throw new Error('fault attachments incomplete')
  }
  const stat = fs.statSync(archivePath)
  if (!stat.isFile() || stat.size <= 0) {
    throw new Error('fault archive invalid')
  }
  const completion: FaultCompletion = {
    schemaVersion: 2,
    eventKey: eventKey,
    archivePath: archivePath,
    archiveSize: stat.size,
    completedAtMs: Date.now(),
    expectedExternalLogCount: state.expectedExternalLogCount,
    confirmedExternalLogCount: state.confirmedExternalLogCount
  }
  atomicWrite(eventDir, FAULT_COMPLETE_FILE, JSON.stringify(completion, null, 2))
}

function readExternalLogCountFromParams(eventDir: string): number {
  try {
    const parsed = JSON.parse(readTextFile(`${eventDir}/params.json`)) as Record<string, Object>
    const params = parsed['params']
    if (params !== null && typeof params === 'object' && !Array.isArray(params)) {
      const nested = (params as Record<string, Object>)['external_log_count']
      if (typeof nested === 'number' && nested >= 0) {
        return normalizeCount(nested)
      }
    }
    // 兼容历史顶层字段。
    const value = parsed['external_log_count']
    if (typeof value === 'number' && value >= 0) {
      return normalizeCount(value)
    }
  } catch (_) {
  }
  return -1
}

function normalizeCount(value: number): number {
  if (!(value >= 0)) {
    return 0
  }
  return Math.floor(value)
}

function readIntegerField(source: Record<string, Object>, key: string, fallback: number): number {
  const value = source[key]
  if (typeof value === 'number' && value >= 0) {
    return Math.floor(value)
  }
  return fallback
}

function readStringField(source: Record<string, Object>, key: string): string {
  const value = source[key]
  return typeof value === 'string' ? value : ''
}

// 导出包搬运：源路径在沙箱，目标可以是沙箱路径或 picker URI。固定 256 KiB
// 缓冲区分块读写，不按 ZIP 大小一次性申请 ArrayBuffer。
async function runFileCopy(request: DiagnosticsFileCopyRequest): Promise<DiagnosticsWorkerResult> {
  const startedAt = Date.now()
  try {
    if (!fileExists(request.sourcePath)) {
      return failureResult(request.requestId, 'file_copy', 'source_missing', '', startedAt)
    }
    const sourceSize = fs.statSync(request.sourcePath).size
    if (sourceSize <= 0) {
      return failureResult(request.requestId, 'file_copy', 'source_empty', '', startedAt)
    }
    if (!request.targetPath.includes('://')) {
      ensureDirectory(parentDir(request.targetPath))
    }

    const sourceFd = fs.openSync(request.sourcePath, fs.OpenMode.READ_ONLY).fd
    let targetFd = -1
    try {
      // picker 返回的是已创建的文档 URI，只申请读写；沙箱分享路径需要创建/截断。
      const targetMode = request.targetPath.includes('://')
        ? fs.OpenMode.READ_WRITE
        : (fs.OpenMode.READ_WRITE | fs.OpenMode.CREATE | fs.OpenMode.TRUNC)
      targetFd = fs.openSync(request.targetPath, targetMode).fd
      const buffer = new ArrayBuffer(FILE_COPY_CHUNK_BYTES)
      let copied = 0
      while (copied < sourceSize) {
        const remaining = sourceSize - copied
        const chunk = remaining < FILE_COPY_CHUNK_BYTES ? remaining : FILE_COPY_CHUNK_BYTES
        const readSize = fs.readSync(sourceFd, buffer, { length: chunk })
        if (readSize <= 0) {
          throw new Error('file copy read incomplete')
        }
        let written = 0
        while (written < readSize) {
          const part: ArrayBuffer = written === 0 && readSize === chunk
            ? buffer : buffer.slice(written, readSize)
          const writeSize = fs.writeSync(targetFd, part, { length: readSize - written })
          if (writeSize <= 0) {
            throw new Error('file copy write incomplete')
          }
          written += writeSize
        }
        copied += readSize
      }
    } finally {
      fs.closeSync(sourceFd)
      if (targetFd >= 0) {
        fs.closeSync(targetFd)
      }
    }
    return successResult(request.requestId, 'file_copy', request.targetPath, startedAt)
  } catch (e) {
    return failureResult(request.requestId, 'file_copy', errorName(e), '', startedAt)
  }
}

function copyFileChunked(sourcePath: string, targetPath: string): void {
  const sourceSize = fs.statSync(sourcePath).size
  const sourceFd = fs.openSync(sourcePath, fs.OpenMode.READ_ONLY).fd
  let targetFd = -1
  try {
    targetFd = fs.openSync(targetPath, fs.OpenMode.READ_WRITE | fs.OpenMode.CREATE | fs.OpenMode.TRUNC).fd
    const buffer = new ArrayBuffer(FILE_COPY_CHUNK_BYTES)
    let copied = 0
    while (copied < sourceSize) {
      const remaining = sourceSize - copied
      const chunk = remaining < FILE_COPY_CHUNK_BYTES ? remaining : FILE_COPY_CHUNK_BYTES
      const readSize = fs.readSync(sourceFd, buffer, { length: chunk })
      if (readSize <= 0) {
        throw new Error('fault log copy read incomplete')
      }
      let written = 0
      while (written < readSize) {
        const part: ArrayBuffer = written === 0 && readSize === chunk
          ? buffer : buffer.slice(written, readSize)
        const writeSize = fs.writeSync(targetFd, part, { length: readSize - written })
        if (writeSize <= 0) {
          throw new Error('fault log copy write incomplete')
        }
        written += writeSize
      }
      copied += readSize
    }
  } finally {
    fs.closeSync(sourceFd)
    if (targetFd >= 0) {
      fs.closeSync(targetFd)
    }
  }
}

function successResult(requestId: string, stage: DiagnosticsArchiveStage, archivePath: string,
  startedAt: number): DiagnosticsWorkerResult {
  return {
    requestId: requestId,
    success: true,
    stage: stage,
    message: '',
    archivePath: archivePath,
    durationMs: Date.now() - startedAt,
    mergedRequestIds: []
  }
}

function failureResult(requestId: string, stage: DiagnosticsArchiveStage, message: string,
  archivePath: string, startedAt: number): DiagnosticsWorkerResult {
  return {
    requestId: requestId,
    success: false,
    stage: stage,
    message: message,
    archivePath: archivePath,
    durationMs: Date.now() - startedAt,
    mergedRequestIds: []
  }
}

function errorName(e: Object): string {
  return e instanceof Error ? e.name : 'operation_failed'
}

function sanitizeEventKey(key: string): string {
  return key.replace(/[^A-Za-z0-9_-]/g, '_')
}

function uniqueStrings(values: string[]): string[] {
  const unique: string[] = []
  for (const value of values) {
    if (unique.indexOf(value) < 0) {
      unique.push(value)
    }
  }
  return unique
}

function writeAttachment(stagingDir: string, attachment: DiagnosticsArchiveAttachment): void {
  if (attachment.relativePath.length === 0 || attachment.relativePath.startsWith('/') ||
    attachment.relativePath.includes('..') || attachment.relativePath.includes('\\')) {
    throw new Error('invalid attachment path')
  }
  HarmonyDiagnosticsArchive.writeTextFile(stagingDir, attachment.relativePath, attachment.content)
}

function copyPersistedLogs(logDir: string, stagingDir: string): LogSummary {
  const files: string[] = []
  if (!fileExists(logDir)) {
    return { status: 'missing', file_count: 0, files: files }
  }
  try {
    for (const fileName of fs.listFileSync(logDir)) {
      if (!LOG_FILENAME_PATTERN.test(fileName)) {
        continue
      }
      const filePath = `${logDir}/${fileName}`
      try {
        if (!fs.statSync(filePath).isFile()) {
          continue
        }
        const content = readTextFile(filePath)
        if (content.length > 0) {
          HarmonyDiagnosticsArchive.writeTextFile(stagingDir, `logs/${fileName}`, content)
          files.push(fileName)
        }
      } catch (_) {
        // 单个日志不可读不影响其余日志和附件；失败统计由 manifest 文件数体现。
      }
    }
  } catch (_) {
    return { status: 'error', file_count: 0, files: files }
  }
  return {
    status: files.length > 0 ? 'ok' : 'missing',
    file_count: files.length,
    files: files
  }
}

function copyCrashAttachments(crashesDir: string, stagingDir: string, archiveKind: string,
  eventKey: string, recentCrashLimit: number): CrashAttachmentCopyResult {
  const eventKeys: string[] = []
  const missing: string[] = []
  if (!fileExists(crashesDir)) {
    return { eventKeys: eventKeys, missing: missing }
  }
  if (archiveKind === 'fault') {
    // fault 路径不接受“跳过并继续”：严格函数会把任何未进入 staging 的必需
    // 附件抛给 publishFaultArchive 的 fault_archive_attachment_copy 阶段。
    if (!EVENT_KEY_PATTERN.test(eventKey)) {
      throw new Error('fault event key invalid')
    }
    const state = readFaultAttachmentState(`${crashesDir}/${eventKey}`, eventKey)
    if (state === null) {
      throw new Error('fault attachment state missing')
    }
    refreshFaultAttachmentState(`${crashesDir}/${eventKey}`, state)
    const copied = copyFaultAttachmentsStrict(crashesDir, stagingDir, eventKey, state)
    return { eventKeys: copied, missing: missing }
  }
  const candidates: CrashCandidate[] = []
  if (recentCrashLimit > 0) {
    try {
      for (const entry of fs.listFileSync(crashesDir)) {
        if (!EVENT_KEY_PATTERN.test(entry)) {
          continue
        }
        try {
          const stat = fs.statSync(`${crashesDir}/${entry}`)
          if (stat.isDirectory()) {
            const candidate: CrashCandidate = { eventKey: entry, mtime: stat.mtime }
            candidates.push(candidate)
          }
        } catch (_) {
          missing.push(`crashes/${entry}`)
        }
      }
    } catch (_) {
      missing.push('crashes')
      return { eventKeys: eventKeys, missing: missing }
    }
    candidates.sort((left: CrashCandidate, right: CrashCandidate): number => right.mtime - left.mtime)
  }

  for (let index = 0; index < candidates.length && index < recentCrashLimit; index++) {
    const key = candidates[index].eventKey
    const eventDir = `${crashesDir}/${key}`
    if (!fileExists(eventDir)) {
      missing.push(`crashes/${key}`)
      continue
    }
    let names: string[] = []
    try {
      names = fs.listFileSync(eventDir)
    } catch (_) {
      missing.push(`crashes/${key}`)
      continue
    }
    const required: string[] = ['params.json', FAULT_STATE_FILE]
    for (const requiredName of required) {
      if (names.indexOf(requiredName) < 0) {
        missing.push(`crashes/${key}/${requiredName}`)
      }
    }
    for (const fileName of names) {
      if (fileName !== 'params.json' && fileName !== FAULT_STATE_FILE &&
        !CRASH_FILE_PATTERN.test(fileName)) {
        continue
      }
      const relativePath = `crashes/${key}/${fileName}`
      const sourcePath = `${eventDir}/${fileName}`
      try {
        const stat = fs.statSync(sourcePath)
        if (!stat.isFile() || stat.size === 0) {
          missing.push(relativePath)
          continue
        }
        const content = readTextFile(sourcePath)
        if (content.length === 0) {
          missing.push(relativePath)
          continue
        }
        HarmonyDiagnosticsArchive.writeTextFile(stagingDir, relativePath, content)
        if (eventKeys.indexOf(key) < 0) {
          eventKeys.push(key)
        }
      } catch (_) {
        missing.push(relativePath)
      }
    }
  }
  return { eventKeys: eventKeys, missing: uniqueStrings(missing) }
}

function buildManifest(request: DiagnosticsArchiveRequest, logs: LogSummary,
  attachments: string[], crashEvents: string[]): DiagnosticsManifest {
  return buildManifestFrom(request.manifest, request.archiveKind, request.eventKey,
    logs, attachments, crashEvents)
}

function buildManifestFrom(meta: DiagnosticsArchiveManifestMeta, archiveKind: string, eventKey: string,
  logs: LogSummary, attachments: string[], crashEvents: string[]): DiagnosticsManifest {
  const collection: ManifestCollection = {
    logs: logs,
    attachments: attachments,
    crash_events: crashEvents
  }
  return {
    schema_version: 1,
    platform: 'harmony',
    archive_kind: archiveKind,
    data_version: meta.dataVersion,
    build_identity: meta.buildKey,
    app_version: meta.appVersion,
    git_sha: meta.gitSha,
    exported_at: meta.exportedAt,
    event_key: eventKey,
    collection: collection
  }
}

async function publishCheckpoint(stagingDir: string, outputDir: string): Promise<string> {
  const pendingZip = await HarmonyDiagnosticsArchive.compressDirectory(
    stagingDir, outputDir, 'sujian-diagnostics-pending', true
  )
  const latestTmp = `${outputDir}/sujian-diagnostics-latest.tmp`
  const latestPath = `${outputDir}/sujian-diagnostics-latest.zip`
  const previousPath = `${outputDir}/sujian-diagnostics-previous.zip`
  safeDeleteFile(latestTmp)
  fs.renameSync(pendingZip, latestTmp)

  const hadLatest = fileExists(latestPath)
  if (hadLatest) {
    safeDeleteFile(previousPath)
    fs.renameSync(latestPath, previousPath)
  }
  try {
    fs.renameSync(latestTmp, latestPath)
    return latestPath
  } catch (e) {
    if (hadLatest && !fileExists(latestPath) && fileExists(previousPath)) {
      try {
        fs.renameSync(previousPath, latestPath)
      } catch (_) {
      }
    }
    safeDeleteFile(latestTmp)
    if (e instanceof Error) {
      throw e
    }
    throw new Error('checkpoint publish failed')
  }
}

function runSnapshot(request: DiagnosticsSnapshotRequest): DiagnosticsWorkerResult {
  const startedAt = Date.now()
  try {
    ensureDirectory(request.runtimeDir)
    atomicWrite(request.runtimeDir, 'theme_runtime_sanitized.json', request.themeJson)
    atomicWrite(request.runtimeDir, 'ui_runtime_sanitized.json', request.uiJson)
    atomicWrite(request.runtimeDir, 'editor_runtime_sanitized.json', request.editorJson)
    if (request.archiveStatePath.length > 0 && request.archiveStateJson.length > 0) {
      const stateDir = parentDir(request.archiveStatePath)
      ensureDirectory(stateDir)
      atomicWrite(stateDir, fileName(request.archiveStatePath), request.archiveStateJson)
    }
    return {
      requestId: request.requestId,
      success: true,
      stage: 'snapshot_write',
      message: '',
      archivePath: '',
      durationMs: Date.now() - startedAt,
      mergedRequestIds: []
    }
  } catch (e) {
    return {
      requestId: request.requestId,
      success: false,
      stage: 'snapshot_write',
      message: e instanceof Error ? e.name : 'snapshot_failed',
      archivePath: '',
      durationMs: Date.now() - startedAt,
      mergedRequestIds: []
    }
  }
}

function atomicWrite(dir: string, filename: string, content: string): void {
  const basePath = `${dir}/${filename}`
  const newPath = `${dir}/${filename}.new`
  const bakPath = `${dir}/${filename}.bak`
  ensureDirectory(dir)
  // 上次进程若在 base→bak 之后、new→base 之前退出，先恢复唯一完整副本。
  if (!fileExists(basePath) && fileExists(bakPath)) {
    try {
      fs.renameSync(bakPath, basePath)
    } catch (_) {
    }
  }
  const file = fs.openSync(newPath, fs.OpenMode.CREATE | fs.OpenMode.WRITE_ONLY | fs.OpenMode.TRUNC)
  try {
    fs.writeSync(file.fd, content)
  } finally {
    fs.closeSync(file)
  }
  safeDeleteFile(bakPath)
  if (fileExists(basePath)) {
    fs.renameSync(basePath, bakPath)
  }
  try {
    fs.renameSync(newPath, basePath)
    safeDeleteFile(bakPath)
  } catch (e) {
    if (!fileExists(basePath) && fileExists(bakPath)) {
      try {
        fs.renameSync(bakPath, basePath)
      } catch (_) {
      }
    }
    safeDeleteFile(newPath)
    if (e instanceof Error) {
      throw e
    }
    throw new Error('runtime snapshot write failed')
  }
}

function cleanStaleStaging(outputDir: string): void {
  if (!fileExists(outputDir)) {
    return
  }
  try {
    for (const entry of fs.listFileSync(outputDir)) {
      if (entry.startsWith('.staging-') || entry.startsWith('.archive-')) {
        HarmonyDiagnosticsArchive.removeTree(`${outputDir}/${entry}`)
      }
    }
  } catch (_) {
  }
}

function readTextFile(filePath: string): string {
  const fd = fs.openSync(filePath, fs.OpenMode.READ_ONLY)
  try {
    const stat = fs.statSync(filePath)
    const buf = new ArrayBuffer(stat.size)
    fs.readSync(fd.fd, buf)
    const decoder = new util.TextDecoder('utf-8')
    return decoder.decodeWithStream(new Uint8Array(buf))
  } finally {
    fs.closeSync(fd)
  }
}

function ensureDirectory(path: string): void {
  if (!fileExists(path)) {
    fs.mkdirSync(path, true)
  }
}

function safeDeleteFile(path: string): void {
  try {
    if (fileExists(path)) {
      fs.unlinkSync(path)
    }
  } catch (_) {
  }
}

function fileExists(path: string): boolean {
  try {
    return fs.accessSync(path)
  } catch (_) {
    return false
  }
}

function parentDir(path: string): string {
  const index = path.lastIndexOf('/')
  return index > 0 ? path.substring(0, index) : path
}

function fileName(path: string): string {
  const index = path.lastIndexOf('/')
  return index >= 0 ? path.substring(index + 1) : path
}

hilog.info(0x0000, LOG_TAG, 'diagnostics archive worker started')
