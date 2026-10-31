// DiagnosticsArchiveProtocol — ArkUI 线程与诊断归档 Worker 之间唯一的消息契约。
//
// 归档 Worker 只接触沙箱路径、白名单附件文本和稳定的元数据；不接触 Core bridge、
// UI 状态或任何原始崩溃日志路径。这样主线程只负责收集轻量附件并投递请求，文件枚举、
// 读取、脱敏、压缩和发布全部在 Worker 线程执行。

// 归档用途。full 是用户手动导出的独立时间戳包；checkpoint 是后台收尾时维护的
// latest/previous 双槽包；fault 是单个系统故障事件的补全包，不能覆盖双槽包。
export type DiagnosticsArchiveKind = 'full' | 'checkpoint' | 'fault'

// Worker 请求类型。snapshot 只做小型运行态 JSON 的原子写入，不做 ZIP。
export type DiagnosticsWorkerRequestKind = 'archive' | 'snapshot'

// 失败阶段用于定位是文件枚举、附件写入、压缩还是发布失败；只上报稳定枚举。
export type DiagnosticsArchiveStage =
  'staging_cleanup' |
  'log_copy' |
  'attachment_write' |
  'crash_copy' |
  'manifest_write' |
  'compress' |
  'publish' |
  'snapshot_write' |
  'worker'

// 已经由 Exporter 整理并脱敏的附件。relativePath 必须命中白名单。
export interface DiagnosticsArchiveAttachment {
  relativePath: string
  content: string
}

// 归档包 manifest 中由主线程提供的稳定元数据；不包含 URL、账号或正文。
export interface DiagnosticsArchiveManifestMeta {
  dataVersion: string
  appVersion: string
  buildKey: string
  gitSha: string
  exportedAt: string
}

// 归档请求。requestId 由客户端生成；Worker 对等价请求合并后仍逐条回复。
export interface DiagnosticsArchiveRequest {
  requestKind: 'archive'
  requestId: string
  archiveKind: DiagnosticsArchiveKind
  filesDir: string
  outputDir: string
  logDir: string
  crashesDir: string
  fileStem: string
  eventKey: string
  attachments: DiagnosticsArchiveAttachment[]
  // full 包最多附带多少个最近故障目录；checkpoint 为 0，fault 由 eventKey 指定。
  recentCrashLimit: number
  // checkpoint 为 true 时按 latest/previous 双槽原子发布；其余用途写独立文件。
  publishLatest: boolean
  manifest: DiagnosticsArchiveManifestMeta
}

// 运行态快照请求。高频编辑器变化由 Worker 侧合并为最后一次 payload。
export interface DiagnosticsSnapshotRequest {
  requestKind: 'snapshot'
  requestId: string
  runtimeDir: string
  archiveStatePath: string
  archiveStateJson: string
  themeJson: string
  uiJson: string
  editorJson: string
}

export type DiagnosticsWorkerRequest = DiagnosticsArchiveRequest | DiagnosticsSnapshotRequest

// Worker 返回结果。mergedRequestIds 让被合并的每个请求都能收到同一个终态。
export interface DiagnosticsWorkerResult {
  requestId: string
  success: boolean
  stage: DiagnosticsArchiveStage
  message: string
  archivePath: string
  durationMs: number
  mergedRequestIds: string[]
}

export const DIAGNOSTICS_ARCHIVE_DATA_VERSION: string = '1'
