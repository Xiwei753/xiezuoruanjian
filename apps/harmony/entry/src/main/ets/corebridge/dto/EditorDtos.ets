// ─── Editor Animation DTOs ─── 与 Core api/types/editor.rs 对齐
// EditorTransactionCause 已被 NativeEditorSessionBridge 使用；动画 DTO 供 motion 层消费，
// 符合 Issue #629 评论第5节「以后做动画时新增 motion 层」规划。

export enum EditorAnimationKind {
  Insert = 'Insert',
  Delete = 'Delete',
  Cursor = 'Cursor'
}

export enum AnimationMode {
  GlyphAnimation = 'GlyphAnimation',
  ClusterAnimation = 'ClusterAnimation',
  RunAnimation = 'RunAnimation',
  LineReflowAnimation = 'LineReflowAnimation',
  SnapshotAnimation = 'SnapshotAnimation',
  SystemSuppressed = 'SystemSuppressed'
}

export enum EditorTransactionCause {
  Typing = 'Typing',
  Delete = 'Delete',
  ImeComposition = 'ImeComposition',
  TypingCommit = 'TypingCommit',
  Paste = 'Paste',
  Undo = 'Undo',
  Redo = 'Redo',
  Load = 'Load',
  Format = 'Format',
  Programmatic = 'Programmatic'
}

export enum VisualCoordinateMode {
  Baseline = 'Baseline'
}

export interface CursorRect {
  x: number
  top: number
  bottom: number
  baselineY: number
}

export interface GlyphRect {
  x: number
  y: number
  w: number
  h: number
  char: string
  baselineY: number
  byteStart: number
  byteEnd: number
}

export interface EditorVisualTransaction {
  id: number
  kind: EditorAnimationKind
  cause: EditorTransactionCause
  oldText: string
  newText: string
  oldSelectionAnchor: number
  oldSelectionHead: number
  newSelectionAnchor: number
  newSelectionHead: number
  insertedRangeStart: number
  insertedRangeEnd: number
  animationMode: AnimationMode
  durationMs: number
  coordinateMode: VisualCoordinateMode
}

// ─── Editor Core DTOs ─── 与 Core api/types/editor.rs 真实契约对齐
// 字段名严格 camelCase（Core serde rename_all = "camelCase"）。
// Issue #629 评论 3：create→u64 sessionId, snapshot→EditorSessionSnapshotDto,
// 编辑命令→EditorEditResultDto。ArkTS 端按此契约定义，不得发明字段。

// 编辑命令结果枚举。字符串值与 Rust EditorEditOutcome 变体对齐。
export enum EditorEditOutcome {
  Applied = 'applied',
  AppliedWithAdjustedSelection = 'appliedWithAdjustedSelection',
  NoChange = 'noChange',
  StaleRevision = 'staleRevision',
  InvalidOffset = 'invalidOffset',
  InvalidRange = 'invalidRange'
}

// 显示增量补丁。replaceByteStart/replaceByteEndExclusive 是 UTF-8 byte offset。
export interface DisplayPatch {
  baseRevision: number
  newRevision: number
  replaceByteStart: number
  replaceByteEndExclusive: number
  insertedText: string
  resultingSelectionStart: number
  resultingSelectionEnd: number
}

// IME composition 会话标识。由 Core 创建并返回，ArkTS 端只持有不伪造。
export interface CompositionSession {
  sessionId: number
  baseRevision: number
  generation: number
}

// 编辑内容增量统计。
export interface EditorContentDelta {
  insertedChars: number
  deletedChars: number
  insertedNonWhitespaceChars: number
  deletedNonWhitespaceChars: number
}

// 视觉意图（动画时间线）。Issue #629 评论 3：只静态编辑器，visualIntent 浅处理，
// 定义接口但不消费动画时间线。后续 motion 层新增时再结构化。
export interface EditorVisualIntent {
  [key: string]: Object | undefined
}

// 编辑命令结果。Core 编辑命令统一返回本结构。
export interface EditorEditResult {
  outcome: EditorEditOutcome
  transactionId: number
  baseRevision: number
  newRevision: number
  displayPatches: DisplayPatch[]
  oldSelectionStart: number
  oldSelectionEnd: number
  newSelectionStart: number
  newSelectionEnd: number
  visualIntent: EditorVisualIntent
  compositionSession: CompositionSession | null
  contentDelta: EditorContentDelta
}

// 编辑会话快照。Core snapshot() 返回。cursor 是 UTF-8 byte offset（光标位置）。
export interface EditorSessionSnapshot {
  text: string
  revision: number
  cursor: number
  selectionAnchor: number
  generation: number
  chapterId: string
}
