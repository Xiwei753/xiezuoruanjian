// ─── Editor DTOs ─── 与 Core api/types/editor.rs 对齐
// Issue #735：视觉类型（EditorVisualIntent/EditorVisualTransaction/AnimationMode 等）已从 Core 删除。
// EditorEditResult 直接暴露编辑事实（cause/operationKind/offsetMap），平台要做动画时从 cause 推导。
//
// Issue #753 线格式约定：
// - 所有 struct/enum 都是 `#[serde(rename_all = "camelCase")]`，除 `EditorTransactionCauseDto`
//   没有 rename_all（线格式 PascalCase 变体名）。
// - `offsetMap` / `compositionSession` / `composition` 带 `skip_serializing_if = "Option::is_none"`，
//   即 Core 在不适用时**整个键省略** → ArkTS 必须是可选属性，不能写成 `| null`。
// - `EditorContentDeltaDto` 是 `#[serde(default)]` 的非 Option 字段，一定出现。

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

// Issue #829 评论5：按编辑事务上报写作统计的入参（Core EditorChangeStatsInputDto）。
//
// 平台端只透传「编辑事实」——cause 和 contentDelta 的 inserted/deleted，
// **不要在端侧猜 source**（拼 "typing"/"pasted" 字符串）。cause→EventSource 的
// 映射和各计数字段归 Core 统一决定，端侧猜错就会污染纯输入字数和当前写作速度。
//
// 与 Core 的差异：Core 的 EditorContentDeltaDto 有四个计数
//（inserted/deleted/insertedNonWhitespace/deletedNonWhitespace），
// 统计只需要前两个，所以这里不带 nonWhitespace 那对字段。
export interface EditorChangeStatsInput {
  deviceId: string
  platform: string
  projectId: string
  volumeId: string
  chapterId: string
  cause: EditorTransactionCause
  insertedChars: number
  deletedChars: number
  // 距上一次统计事件的秒数，首次事件传 0。
  durationSeconds: number
  sessionId: string
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

// 操作类型 — 区分不同编辑操作的语义类别。与 Core EditorOperationKindDto 对齐。
// Core 侧 `#[serde(rename_all = "camelCase")]`，线格式是小写开头的 camelCase。
export enum EditorOperationKind {
  Insert = 'insert',
  Delete = 'delete',
  Replace = 'replace',
  CursorOnly = 'cursorOnly',
  CompositionUpdate = 'compositionUpdate',
  CompositionCommit = 'compositionCommit',
  CompositionCancel = 'compositionCancel',
  Load = 'load',
  Format = 'format'
}

// 偏移映射类型 — 与 Core OffsetMapKindDto 对齐（同样 camelCase）。
export enum OffsetMapKind {
  Identity = 'identity',
  Shifted = 'shifted'
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
//
// Issue #826 评论 9 阻塞 1：replaceByteStart/End 描述的是**旧正文**（base revision）
// 里被替换的区间；insertedByteStart/EndExclusive 才是本条 patch 插入的内容在
// **最终新正文**（new revision）中的准确区间，纯删除时为零长。
// 不得用 replaceByteStart + insertedText.length 推算后者：一笔 batch 里前一处
// 替换变长后，后一处在新正文里已经右移。
export interface DisplayPatch {
  baseRevision: number
  newRevision: number
  replaceByteStart: number
  replaceByteEndExclusive: number
  insertedByteStart: number
  insertedByteEndExclusive: number
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
  // Core 侧 skip_serializing_if：不适用时整个键省略。
  offsetMap?: OffsetMap
  compositionSession?: CompositionSession
  contentDelta: EditorContentDelta
  // #629 评论6 Part B：当前 composition 完整状态。composition 活跃时 Core 才发出该键，
  // 平台端据此构造临时显示文本和下划线范围。无 composition 时键省略。
  composition?: EditorCompositionState
}

// 编辑会话快照。Core snapshot() 返回。cursor 是 UTF-8 byte offset（光标位置）。
export interface EditorSessionSnapshot {
  text: string
  revision: number
  cursor: number
  selectionAnchor: number
  generation: number
  chapterId: string
  // #629 评论6 Part B：Core 只在 composition 活跃时发出该键（skip_serializing_if）。
  composition?: EditorCompositionState
}
