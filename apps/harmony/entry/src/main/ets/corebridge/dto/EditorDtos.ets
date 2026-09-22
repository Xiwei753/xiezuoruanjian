// ─── Editor DTOs ─── 与 Core api/types/editor.rs 对齐
// Issue #735：视觉类型（EditorVisualIntent/EditorVisualTransaction/AnimationMode 等）已从 Core 删除。
// EditorEditResult 直接暴露编辑事实（cause/operationKind/offsetMap），平台要做动画时从 cause 推导。

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

// 操作类型 — 区分不同编辑操作的语义类别。与 Core EditorOperationKind 对齐。
export enum EditorOperationKind {
  Insert = 'Insert',
  Delete = 'Delete',
  Replace = 'Replace',
  CursorOnly = 'CursorOnly',
  CompositionUpdate = 'CompositionUpdate',
  CompositionCommit = 'CompositionCommit',
  CompositionCancel = 'CompositionCancel',
  Load = 'Load',
  Format = 'Format'
}

// 偏移映射类型 — 与 Core OffsetMapKind 对齐。
export enum OffsetMapKind {
  Identity = 'Identity',
  Shifted = 'Shifted'
}

// 单个偏移映射条目 — 与 Core OffsetMapEntry 对齐。
// oldByteOffset / newByteOffset 均为 UTF-8 byte offset。
export interface OffsetMapEntry {
  oldByteOffset: number
  newByteOffset: number
  length: number
  kind: OffsetMapKind
}

// 偏移映射 — 与 Core OffsetMap 对齐。
// 记录 old 正文 → new 正文的字符身份映射。
export interface OffsetMap {
  entries: OffsetMapEntry[]
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

// #629 评论6 Part B：Composition 完整状态。Core 暴露 preedit 文本和 replace range，
// ArkTS 端据此构造临时显示文本和下划线，不复制 Core 的 composition 状态机。
// replaceByteStart/replaceByteEndExclusive 是 UTF-8 byte offset 半开区间，
// 指明 committed text 中将被 preeditText 替换的范围。
// preeditCursorUtf16 是 preedit 内部光标的 UTF-16 code unit offset（IME 协议要求）。
export interface EditorCompositionState {
  sessionId: number
  baseRevision: number
  generation: number
  replaceByteStart: number
  replaceByteEndExclusive: number
  preeditText: string
  preeditCursorUtf16: number
}

// 编辑内容增量统计。
export interface EditorContentDelta {
  insertedChars: number
  deletedChars: number
  insertedNonWhitespaceChars: number
  deletedNonWhitespaceChars: number
}

// 编辑命令结果。Core 编辑命令统一返回本结构。
// Issue #735：直接暴露编辑事实（cause/operationKind/offsetMap），不再带 visualIntent。
export interface EditorEditResult {
  outcome: EditorEditOutcome
  transactionId: number
  baseRevision: number
  newRevision: number
  displayPatches: DisplayPatch[]
  oldSelectionAnchor: number
  oldSelectionHead: number
  newSelectionAnchor: number
  newSelectionHead: number
  cause: EditorTransactionCause
  operationKind: EditorOperationKind
  offsetMap: OffsetMap | null
  compositionSession: CompositionSession | null
  contentDelta: EditorContentDelta
  // #629 评论6 Part B：当前 composition 完整状态。composition 活跃时非 null，
  // 平台端据此构造临时显示文本和下划线范围。无 composition 时为 null。
  composition: EditorCompositionState | null
}

// 编辑会话快照。Core snapshot() 返回。cursor 是 UTF-8 byte offset（光标位置）。
export interface EditorSessionSnapshot {
  text: string
  revision: number
  cursor: number
  selectionAnchor: number
  generation: number
  chapterId: string
  // #629 评论6 Part B：composition 活跃时非 null，平台端据此构造临时显示文本和下划线。
  composition: EditorCompositionState | null
}
