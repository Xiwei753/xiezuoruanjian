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
}

interface CrashCandidate {
  eventKey: string
  mtime: number
}

interface FaultCompletion {
  schemaVersion: number
  eventKey: string
  archivePath: string
  completedAtMs: number
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

    stage = 'crash_copy'
    const crashEvents = copyCrashAttachments(
      request.crashesDir, stagingDir, request.archiveKind, request.eventKey, request.recentCrashLimit
    )

    stage = 'manifest_write'
    const manifest = buildManifest(request, logSummary, attachmentNames, crashEvents)
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
// complete.json 是唯一的持久完成标记；params.json 只是原始事件快照。
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
    // 有 complete.json 但 ZIP 已被清理时，删除旧标记并重新生成。
    if (fileExists(completePath)) {
      safeDeleteFile(completePath)
    }

    if (!fileExists(paramsPath)) {
      if (request.paramsJson.length === 0) {
        throw new Error('missing fault params')
      }
      atomicWrite(eventDir, 'params.json', request.paramsJson)
    }

    stage = 'fault_log_copy'
    copyExternalFaultLogs(request.externalLogPaths, eventDir)

    stage = 'fault_compress'
    const archivePath = await publishFaultArchive(
      request.outputDir, request.logDir, request.crashesDir, request.manifest, eventKey
    )

    stage = 'fault_complete'
    writeFaultCompletion(eventDir, eventKey, archivePath)
    return successResult(request.requestId, 'fault_complete', archivePath, startedAt)
  } catch (e) {
    return failureResult(request.requestId, stage, errorName(e), '', startedAt)
  }
}

// 启动补全：上次进程如果在复制/压缩/写完成标记之间被杀，这里从已经落盘的
// params.json + fault-N.log 继续生成 ZIP，而不是因为 params.json 存在就跳过。
async function runFaultRecovery(request: DiagnosticsFaultRecoveryRequest): Promise<DiagnosticsWorkerResult> {
  const startedAt = Date.now()
  let stage: DiagnosticsArchiveStage = 'fault_recovery'
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
      const completePath = `${eventDir}/${FAULT_COMPLETE_FILE}`
      if (fileExists(completePath)) {
        safeDeleteFile(completePath)
      }
      stage = 'fault_compress'
      const archivePath = await publishFaultArchive(
        request.outputDir, request.logDir, request.crashesDir, request.manifest, entry
      )
      stage = 'fault_complete'
      writeFaultCompletion(eventDir, entry, archivePath)
    }
    return successResult(request.requestId, 'fault_complete', '', startedAt)
  } catch (e) {
    return failureResult(request.requestId, stage, errorName(e), '', startedAt)
  }
}

async function publishFaultArchive(outputDir: string, logDir: string, crashesDir: string,
  meta: DiagnosticsArchiveManifestMeta, eventKey: string): Promise<string> {
  const stagingDir = `${outputDir}/.staging-fault-${eventKey}-${Date.now()}-${Math.floor(Math.random() * 1000000)}`
  try {
    cleanStaleStaging(outputDir)
    HarmonyDiagnosticsArchive.removeTree(stagingDir)
    ensureDirectory(stagingDir)
    const logSummary = copyPersistedLogs(logDir, stagingDir)
    const crashEvents = copyCrashAttachments(crashesDir, stagingDir, 'fault', eventKey, 0)
    const manifest = buildManifestFrom(meta, 'fault', eventKey, logSummary, [], crashEvents)
    HarmonyDiagnosticsArchive.writeTextFile(
      stagingDir, 'diagnostics_manifest.json', JSON.stringify(manifest, null, 2)
    )
    return await HarmonyDiagnosticsArchive.compressDirectory(
      stagingDir, outputDir, `sujian-diagnostics-fault-${eventKey}`, true
    )
  } finally {
    HarmonyDiagnosticsArchive.removeTree(stagingDir)
  }
}

// external_log 可能是大文件：按固定块复制，成功后删除系统源文件。源文件已不存在
// 时不阻塞补全（上次可能已经复制过，fault-N.log 仍在事件目录）。
function copyExternalFaultLogs(paths: string[], eventDir: string): void {
  for (let index = 0; index < paths.length; index++) {
    const sourcePath = paths[index]
    if (sourcePath.length === 0 || !fileExists(sourcePath)) {
      continue
    }
    copyFileChunked(sourcePath, `${eventDir}/fault-${index + 1}.log`)
    try {
      fs.unlinkSync(sourcePath)
    } catch (_) {
      hilog.warn(0x0000, LOG_TAG, 'unable to remove copied external fault log source')
    }
  }
}

function isFaultComplete(eventDir: string): boolean {
  const completePath = `${eventDir}/${FAULT_COMPLETE_FILE}`
  const completion = readFaultCompletion(completePath)
  return completion !== null && completion.archivePath.length > 0 && fileExists(completion.archivePath)
}

function readFaultCompletion(path: string): FaultCompletion | null {
  if (!fileExists(path)) {
    return null
  }
  try {
    const parsed = JSON.parse(readTextFile(path)) as FaultCompletion
    if (parsed !== null && typeof parsed.archivePath === 'string' &&
      typeof parsed.completedAtMs === 'number') {
      return parsed
    }
  } catch (_) {
  }
  return null
}

function writeFaultCompletion(eventDir: string, eventKey: string, archivePath: string): void {
  const completion: FaultCompletion = {
    schemaVersion: 1,
    eventKey: eventKey,
    archivePath: archivePath,
    completedAtMs: Date.now()
  }
  atomicWrite(eventDir, FAULT_COMPLETE_FILE, JSON.stringify(completion, null, 2))
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
  eventKey: string, recentCrashLimit: number): string[] {
  const copied: string[] = []
  if (!fileExists(crashesDir)) {
    return copied
  }
  const candidates: CrashCandidate[] = []
  if (archiveKind === 'fault') {
    if (EVENT_KEY_PATTERN.test(eventKey)) {
      const candidate: CrashCandidate = { eventKey: eventKey, mtime: 0 }
      candidates.push(candidate)
    }
  } else if (recentCrashLimit > 0) {
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
        }
      }
    } catch (_) {
      return copied
    }
    candidates.sort((left: CrashCandidate, right: CrashCandidate): number => right.mtime - left.mtime)
  }

  const limit = archiveKind === 'fault' ? candidates.length : recentCrashLimit
  for (let index = 0; index < candidates.length && index < limit; index++) {
    const eventDir = `${crashesDir}/${candidates[index].eventKey}`
    if (!fileExists(eventDir)) {
      continue
    }
    try {
      for (const fileName of fs.listFileSync(eventDir)) {
        if (fileName !== 'params.json' && !CRASH_FILE_PATTERN.test(fileName)) {
          continue
        }
        try {
          const content = readTextFile(`${eventDir}/${fileName}`)
          if (content.length > 0) {
            HarmonyDiagnosticsArchive.writeTextFile(
              stagingDir, `crashes/${candidates[index].eventKey}/${fileName}`, content
            )
            copied.push(candidates[index].eventKey)
          }
        } catch (_) {
        }
      }
    } catch (_) {
    }
  }
  const unique: string[] = []
  for (const event of copied) {
    if (unique.indexOf(event) < 0) {
      unique.push(event)
    }
  }
  return unique
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
