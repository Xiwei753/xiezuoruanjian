// DiagnosticsArchiveProtocol — ArkUI 线程与诊断归档 Worker 之间唯一的消息契约。
//
// 归档 Worker 只接触沙箱路径、白名单附件文本和稳定的元数据；不接触 Core bridge、
// UI 状态或任何原始崩溃日志路径。这样主线程只负责收集轻量附件并投递请求，文件枚举、
// 读取、脱敏、压缩和发布全部在 Worker 线程执行。

// 归档用途。full 是用户手动导出的独立时间戳包；checkpoint 是后台收尾时维护的
// latest/previous 双槽包；fault 是单个系统故障事件的补全包，不能覆盖双槽包。
export type DiagnosticsArchiveKind = 'full' | 'checkpoint' | 'fault'

// Worker 请求类型。snapshot 只做小型运行态 JSON 的原子写入，不做 ZIP；
// fault_capture 在 Worker 内完成故障目录落盘、外部日志复制和 fault ZIP；
// fault_recovery 在启动时补全上次中断的故障事件；file_copy 用分块读写搬运
// 已经生成的诊断包，避免 ArkUI 线程按 ZIP 大小申请内存。
export type DiagnosticsWorkerRequestKind =
  'archive' | 'snapshot' | 'fault_capture' | 'fault_recovery' | 'fault_keys' | 'file_copy'

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
  'fault_prepare' |
  'fault_log_copy' |
  'fault_compress' |
  'fault_complete' |
  'fault_recovery' |
  'fault_keys' |
  'file_copy' |
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

// 故障事件落盘请求。paramsJson 已在 ArkUI 线程完成白名单投影；externalLogPaths
// 只传系统外部日志路径，复制、删除和 ZIP 都由 Worker 串行执行。
// expectedExternalLogCount 是系统事件给出的 external_log 数量；Worker 用它和
// 持久化附件完成状态判定是否允许写 complete.json。logOverLimit 只作为缺失原因
// 写入事件元数据，不参与完成判定。
export interface DiagnosticsFaultCaptureRequest {
  requestKind: 'fault_capture'
  requestId: string
  outputDir: string
  logDir: string
  crashesDir: string
  eventKey: string
  paramsJson: string
  externalLogPaths: string[]
  expectedExternalLogCount: number
  logOverLimit: boolean
  manifest: DiagnosticsArchiveManifestMeta
}

// 启动补全请求。Worker 扫描 crashes/ 下没有有效 complete.json 的事件目录；
// 只有当 fault-state.json 记录的已确认附件数达到预期数时才重新发布 fault ZIP，
// 否则保持 incomplete，等待系统后续回放提供外部日志。
export interface DiagnosticsFaultRecoveryRequest {
  requestKind: 'fault_recovery'
  requestId: string
  outputDir: string
  logDir: string
  crashesDir: string
  manifest: DiagnosticsArchiveManifestMeta
}

// 启动去重查询：只返回已经发布完成（complete.json + ZIP 均有效）的 eventKey，
// 不读附件、不压缩。未完成事件留给系统回放或 fault_recovery 继续补全。
export interface DiagnosticsFaultKeysRequest {
  requestKind: 'fault_keys'
  requestId: string
  crashesDir: string
}

// 诊断包分块复制请求。sourcePath 位于沙箱，targetPath 可以是沙箱路径或
// DocumentViewPicker 返回的 URI；Worker 固定大小分块读写，不一次性分配整包内存。
export interface DiagnosticsFileCopyRequest {
  requestKind: 'file_copy'
  requestId: string
  sourcePath: string
  targetPath: string
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

export type DiagnosticsWorkerRequest =
  DiagnosticsArchiveRequest |
  DiagnosticsSnapshotRequest |
  DiagnosticsFaultCaptureRequest |
  DiagnosticsFaultRecoveryRequest |
  DiagnosticsFaultKeysRequest |
  DiagnosticsFileCopyRequest

// 需要与归档共用单一串行队列的请求。snapshot 体积小且独立，不进入该队列。
export type DiagnosticsHeavyRequest =
  DiagnosticsArchiveRequest |
  DiagnosticsFaultCaptureRequest |
  DiagnosticsFaultRecoveryRequest |
  DiagnosticsFaultKeysRequest |
  DiagnosticsFileCopyRequest

// Worker 返回结果。mergedRequestIds 让被合并的每个请求都能收到同一个终态。
export interface DiagnosticsWorkerResult {
  requestId: string
  success: boolean
  stage: DiagnosticsArchiveStage
  message: string
  archivePath: string
  durationMs: number
  mergedRequestIds: string[]
  // 仅 fault_keys 查询返回；其他请求不设置。
  eventKeys?: string[]
}

export const DIAGNOSTICS_ARCHIVE_DATA_VERSION: string = '1'
