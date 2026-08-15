// ─── Editor Animation DTOs ─── 与 Core api/types/editor.rs 对齐
// 鸿蒙端动画暂未实现，DTO 定义先行对齐，为后续接入做准备。

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
