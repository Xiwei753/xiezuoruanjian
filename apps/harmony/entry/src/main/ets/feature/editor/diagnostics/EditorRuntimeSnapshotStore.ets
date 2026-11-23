// EditorRuntimeSnapshotStore — 原生 RichEditor 运行时事实快照存储。
//
// Issue #1053：自研排版的运动层/静态遮罩/首行缩进等字段已不存在，改为系统
// RichEditor 的真实事实：
// - 会话与版本：session/revision
// - 文本事实（不含正文）：native/core 的 UTF-16 长度、换行数、FNV-1a hash
// - 原生结构：Span 数、硬换行段落数、getLayoutManager().getLineCount()
// - 编辑器/内容区尺寸（px）、光标与选区（UTF-16）
// - 焦点、编辑态、IME 预编辑状态
// - 选用的 wordBreak/textAlign
// - 最近一次原生文本操作类型与范围
// - 保存/退出屏障处的原生↔Core 对齐事实
// - 当前行 LineMetrics（只抓光标所在行）
//
// 关键原则：
// - 快照不包含正文字符，只记录长度、换行数、hash、偏移与尺寸。
// - ArkTS 单线程模型，不需要锁；updateSnapshot 每次接收完整快照，避免新旧字段混合。
// - Store → HarmonyDiagnosticsRuntimeSnapshot → Checkpoint → Exporter 沿用现有诊断链，
//   落盘仍由归档 Worker 合并处理。

// 当前行度量快照。startIndex/endIndex 为 UTF-16 code unit offset。
export interface RichEditorLineMetricsSnapshot {
  lineIndex: number
  startIndex: number
  endIndex: number
  height: number
  width: number
  baseline: number
  left: number
  topHeight: number
}

// 最近一次原生文本操作（不含正文原文）。
export interface NativeTextOperationSnapshot {
  kind: string                  // 'typing' | 'ime-commit' | 'paste' | 'programmatic-sync' | 'initial-load'
  beforeUtf16Length: number     // 操作前本地镜像长度
  afterUtf16Length: number      // 操作后本地镜像长度
  rangeBeforeStartUtf16: number // 原生 onDidChange rangeBefore 起点（无法解析时 -1）
  rangeBeforeEndUtf16: number   // 原生 onDidChange rangeBefore 终点（无法解析时 -1）
  rangeAfterStartUtf16: number  // 原生 onDidChange rangeAfter 起点（无法解析时 -1）
  rangeAfterEndUtf16: number    // 原生 onDidChange rangeAfter 终点（无法解析时 -1）
  startUtf16: number            // 实际提交 Core 的编辑起点（UTF-16）
  endUtf16: number              // 实际提交 Core 的编辑终点（UTF-16，操作前坐标）
  replacedUtf16Length: number   // 被替换长度
  insertedUtf16Length: number   // 插入长度
  spanCountInRange: number      // 编辑范围内原生 Span 数
  nonTextSpanCount: number      // 编辑范围内的非文本 Span 数（正常应为 0）
  imeComposition: boolean       // 操作发生时是否处于 IME 预编辑
  programmatic: boolean         // 是否程序性样式/文本更新
  coreRevision: number          // 操作发生时的 Core revision
  factError: boolean            // 该次操作是否触发原生事实错误
  atMs: number
}

// 原生全文镜像与 Core 正文的对齐事实（保存/退出屏障记录，不含正文）。
export interface NativeCoreAlignmentSnapshot {
  stage: string                 // 'save-when-idle' | 'exit-finish-editing' | 'sync-from-core' | 'initial-load'
  matched: boolean              // 本地镜像与 Core 正文是否一致
  nativeUtf16Length: number
  coreUtf16Length: number
  nativeNewlineCount: number
  coreNewlineCount: number
  nativeHash: string
  coreHash: string
  nativeSpanCount: number       // 原生全文 Span 总数（含非文本 Span；仅在屏障完整读取时计算）
  nativeRevision: number        // 本次事实对应的已应用 Core revision（未知时 -1）
  coreRevision: number          // 本次事实读取时的 Core revision（未知时 -1）
  mirrorMatchesEditor: boolean  // 本地镜像是否等于从原生 Span 重建的全文
  firstMismatchUtf16: number    // 镜像与原生全文的首个差异 offset（-1 表示一致或无法重建）
  detail: string                // 事实描述，不含正文字符
  atMs: number
}

// 原生 RichEditor 运行时快照 — 全部脱敏，不含正文内容。
export interface RichEditorRuntimeSnapshot {
  schemaVersion: number

  // 会话/版本
  hasActiveSession: boolean
  sessionId: number                     // Core 会话 ID（无会话时 -1）
  nativeRevision: number                // 已应用到原生编辑器的 Core revision（未知时 -1）
  coreRevision: number                  // Core snapshot revision（无快照时 -1）

  // 文本事实（不含正文字符）。
  // Issue #1053 评论 7099290808：长度/换行数按编辑增量维护；hash 与全量 Span 数
  // 只在保存/退出屏障或失焦/导出前完整计算一次，并与 revision 绑定（见 lastNativeCoreAlignment）。
  nativeTextUtf16Length: number
  nativeNewlineCount: number
  nativeTextHash: string                // 上次屏障的稳定态 hash；未知时为空串
  coreTextUtf16Length: number
  coreNewlineCount: number
  coreTextHash: string                  // 上次屏障的稳定态 hash；未知时为空串
  nativeCoreTextMatched: boolean        // 上次屏障三方校验结果（非实时逐字符比较）
  nativeCoreDiverged: boolean           // 屏障发现原生与 Core 相异且原生正文有效；阻止异步回写覆盖原生内容

  // 原生结构
  nativeSpanCount: number               // 上次屏障完整读取的 Span 总数；未知时 -1
  paragraphCount: number                // 硬换行段落数 = 增量维护的换行数 + 1
  layoutLineCount: number               // 系统布局行数（LayoutManager.getLineCount）

  // 光标/选区（UTF-16）
  caretUtf16: number
  selectionStartUtf16: number
  selectionEndUtf16: number

  // 编辑器/内容尺寸（px，来自 onAreaChange 实测）
  editorWidthPx: number
  editorHeightPx: number
  contentWidthPx: number                // 正文排版内容区宽度
  contentHeightPx: number
  horizontalPaddingVp: number

  // 排版（Issue #1053 评论 7100110974：读取 getParagraphs 的实际生效值，而不是请求常量）
  wordBreak: string                     // 实际段落断行策略，如 'BREAK_WORD'；未知为 'unknown'
  lineBreakStrategy: string             // 实际换行策略，如 'HIGH_QUALITY'；未知为 'unknown'
  textAlign: string                     // 实际段落对齐，如 'Start'；未知为 'unknown'
  punctuationOverflow: boolean          // 是否按 API 能力启用行尾标点溢出
  compressLeadingPunctuation: boolean   // 是否按 API 能力启用行首标点压缩
  paragraphStyleApplied: boolean        // getParagraphs 读到的段落样式是否与预期一致
  paragraphStyleError: string | null    // 应用/读取段落样式失败的真实原因
  deviceApiLevel: number                // 设备 SDK API Level（0 表示未知）
  fontSizeVp: number
  lineSpacingMultiplier: number
  effectiveLineHeightVp: number

  // 焦点/IME
  editorFocused: boolean
  editing: boolean                      // RichEditor onEditingChange 状态
  imeComposing: boolean                 // 是否处于 IME 预编辑
  previewTextUtf16Length: number        // 预上屏文本长度（无预上屏为 0）

  // 最近原生文本操作与对齐事实
  lastNativeOperation: NativeTextOperationSnapshot | null
  lastNativeCoreAlignment: NativeCoreAlignmentSnapshot | null
  nativeFactError: string | null

  // 当前行度量（只抓光标所在行）
  currentLineMetrics: RichEditorLineMetricsSnapshot | null

  // 挂载状态与快照时间戳
  isEditorMounted: boolean
  capturedAtMs: number
}

// Issue #773 评论 5845033983：observer 类型，让 Checkpoint 能订阅 editor 状态变化。
// 不反向 import Checkpoint（避免循环依赖）。
export type EditorSnapshotObserver = (snapshot: RichEditorRuntimeSnapshot | null) => void

// ── 诊断用纯函数（只输出事实，不保存正文） ─────────────────────

// 统计硬换行数量。
export function countNewlines(text: string): number {
  let count = 0
  for (let i = 0; i < text.length; i++) {
    if (text.charCodeAt(i) === 10) {
      count++
    }
  }
  return count
}

// FNV-1a 32 位 hash，输出 8 位十六进制。只用于判断两份正文是否同一内容。
export function hashTextUtf16(text: string): string {
  let hash = 0x811c9dc5
  for (let i = 0; i < text.length; i++) {
    hash = hash ^ text.charCodeAt(i)
    hash = Math.imul(hash, 0x01000193) >>> 0
  }
  const hex = (hash >>> 0).toString(16)
  let padded = ''
  for (let i = hex.length; i < 8; i++) {
    padded += '0'
  }
  return padded + hex
}

// 返回两段文本首个不同的 UTF-16 offset；完全一致返回 -1。
export function firstMismatchUtf16(a: string, b: string): number {
  const shared = Math.min(a.length, b.length)
  for (let i = 0; i < shared; i++) {
    if (a.charCodeAt(i) !== b.charCodeAt(i)) {
      return i
    }
  }
  if (a.length !== b.length) {
    return shared
  }
  return -1
}

export class EditorRuntimeSnapshotStore {
  private static instance: EditorRuntimeSnapshotStore
  private snapshot: RichEditorRuntimeSnapshot | null = null
  // Issue #773 评论 5845033983：observer 列表，Checkpoint 订阅 editor 状态变化。
  private observers: EditorSnapshotObserver[] = []

  static getInstance(): EditorRuntimeSnapshotStore {
    if (EditorRuntimeSnapshotStore.instance === undefined || EditorRuntimeSnapshotStore.instance === null) {
      EditorRuntimeSnapshotStore.instance = new EditorRuntimeSnapshotStore()
    }
    return EditorRuntimeSnapshotStore.instance
  }

  // Issue #773 评论 5845033983：添加 observer，editor 状态变化时通知。
  addObserver(observer: EditorSnapshotObserver): void {
    this.observers.push(observer)
  }

  // Issue #773 评论 5845033983：移除 observer。
  removeObserver(observer: EditorSnapshotObserver): void {
    const index = this.observers.indexOf(observer)
    if (index >= 0) {
      this.observers.splice(index, 1)
    }
  }

  // Issue #773 评论 5845033983：通知所有 observer。
  private notifyObservers(): void {
    for (const observer of this.observers) {
      observer(this.snapshot)
    }
  }

  // SujianEditor 调用，更新完整快照（所有字段必填），保证诊断数据一致。
  updateSnapshot(snapshot: RichEditorRuntimeSnapshot): void {
    this.snapshot = snapshot
    this.notifyObservers()
  }

  // 诊断导出器调用，读取当前快照。
  getSnapshot(): RichEditorRuntimeSnapshot | null {
    return this.snapshot
  }

  // 编辑器卸载时标记 isEditorMounted=false。
  // 不清空快照，保留最后状态供离页后导出诊断；下次打开编辑器时 updateSnapshot 覆盖。
  markEditorUnmounted(): void {
    if (this.snapshot !== null) {
      this.snapshot.isEditorMounted = false
      this.notifyObservers()
    }
  }

  // 清空快照（仅在完全重置时使用，aboutToDisappear 不调用）。
  clear(): void {
    this.snapshot = null
    this.notifyObservers()
  }
}
