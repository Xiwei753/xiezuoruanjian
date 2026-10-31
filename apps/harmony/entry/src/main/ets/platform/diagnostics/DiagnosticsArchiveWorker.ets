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
  DiagnosticsArchiveRequest,
  DiagnosticsArchiveStage,
  DiagnosticsSnapshotRequest,
  DiagnosticsWorkerRequest,
  DiagnosticsWorkerResult
} from './DiagnosticsArchiveProtocol'

const LOG_TAG = 'DiagnosticsArchiveWorker'
const LOG_FILENAME_PATTERN = /^sujian-current-[A-Za-z0-9._-]+\.log$/
const CRASH_FILE_PATTERN = /^fault-[0-9]+\.log$/
const EVENT_KEY_PATTERN = /^[A-Za-z0-9_-]+$/

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

interface PendingArchive {
  request: DiagnosticsArchiveRequest
  requestIds: string[]
}

let archiveQueue: PendingArchive[] = []
let processingArchive: boolean = false
let pendingSnapshot: DiagnosticsSnapshotRequest | null = null
let pendingSnapshotIds: string[] = []
let processingSnapshot: boolean = false

workerPort.onmessage = (event: MessageEvents): void => {
  const request = event.data as DiagnosticsWorkerRequest
  if (request.requestKind === 'snapshot') {
    enqueueSnapshot(request)
  } else if (request.requestKind === 'archive') {
    enqueueArchive(request)
  }
}

function enqueueArchive(request: DiagnosticsArchiveRequest): void {
  for (const pending of archiveQueue) {
    if (pending.request.archiveKind === request.archiveKind &&
      (request.archiveKind !== 'fault' || pending.request.eventKey === request.eventKey)) {
      // 等价归档只保留最新内容，但每个调用方的 requestId 都必须收到终态。
      pending.request = request
      pending.requestIds.push(request.requestId)
      return
    }
  }
  const pending: PendingArchive = { request: request, requestIds: [request.requestId] }
  archiveQueue.push(pending)
  void drainArchiveQueue()
}

async function drainArchiveQueue(): Promise<void> {
  if (processingArchive) {
    return
  }
  processingArchive = true
  try {
    while (archiveQueue.length > 0) {
      const pending = archiveQueue.shift()
      if (pending === undefined) {
        continue
      }
      const result = await runArchive(pending.request)
      postArchiveResult(result, pending.requestIds)
    }
  } finally {
    processingArchive = false
  }
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

function postArchiveResult(result: DiagnosticsWorkerResult, requestIds: string[]): void {
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
  const collection: ManifestCollection = {
    logs: logs,
    attachments: attachments,
    crash_events: crashEvents
  }
  return {
    schema_version: 1,
    platform: 'harmony',
    archive_kind: request.archiveKind,
    data_version: request.manifest.dataVersion,
    build_identity: request.manifest.buildKey,
    app_version: request.manifest.appVersion,
    git_sha: request.manifest.gitSha,
    exported_at: request.manifest.exportedAt,
    event_key: request.eventKey,
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
