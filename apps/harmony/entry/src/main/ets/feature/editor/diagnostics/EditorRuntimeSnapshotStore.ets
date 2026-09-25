// EditorRuntimeSnapshotStore — 编辑器运行时状态快照存储。
//
// 负责：
// 1. 收集编辑器运行时状态（脱敏，不含正文内容）
// 2. SujianEditor 在关键生命周期点调用 updateSnapshot 更新状态
// 3. 诊断导出器从 getSnapshot 读取快照，导出为 editor_runtime_sanitized.json
//
// 关键原则：
// - 不存储正文内容（text 字段），只存长度和偏移
// - 不存储用户可识别的信息
// - 线程安全不需要考虑（ArkUI 单线程模型）

// 编辑器运行时快照 — 全部脱敏，不含正文内容。
interface EditorRuntimeSnapshot {
  // 布局信息
  lineCount: number                    // 总行数
  contentWidthPx: number               // 内容区域宽度（px）
  contentHeightPx: number              // 内容区域高度（px）
  viewportWidthPx: number              // 视口宽度（px）
  viewportHeightPx: number             // 视口高度（px）

  // 光标/选区信息
  cursorUtf16Offset: number            // 光标 UTF-16 偏移
  selectionStartUtf16: number          // 选区起点
  selectionEndUtf16: number            // 选区终点
  hasComposition: boolean              // 是否有输入法 composition
  compositionLength: number            // composition 长度（不记内容）

  // 布局来源
  layoutSource: string                 // 'system' | 'fallback' | 'none'
  usingLayoutManager: boolean          // 是否使用系统 LayoutManager
  apiLevel: number                     // 当前 API Level

  // 编辑器状态
  isImeAttached: boolean               // IME 是否连接
  revision: number                     // 当前编辑器 revision
  textLength: number                   // 正文总长度（字符数，不含内容）

  // 错误/降级信息
  lastLayoutError: string | null       // 最后一次布局错误信息
  fallbackCount: number                // fallback 次数累计
}

export class EditorRuntimeSnapshotStore {
  private static instance: EditorRuntimeSnapshotStore
  private snapshot: EditorRuntimeSnapshot | null = null

  static getInstance(): EditorRuntimeSnapshotStore {
    if (EditorRuntimeSnapshotStore.instance === undefined || EditorRuntimeSnapshotStore.instance === null) {
      EditorRuntimeSnapshotStore.instance = new EditorRuntimeSnapshotStore()
    }
    return EditorRuntimeSnapshotStore.instance
  }

  // SujianEditor 调用，更新快照（部分更新，合并到现有快照）。
  updateSnapshot(snapshot: Partial<EditorRuntimeSnapshot>): void {
    if (this.snapshot === null) {
      // 首次更新，需要提供完整快照
      this.snapshot = snapshot as EditorRuntimeSnapshot
    } else {
      // 部分更新，合并字段
      this.snapshot = { ...this.snapshot, ...snapshot }
    }
  }

  // 诊断导出器调用，读取当前快照。
  getSnapshot(): EditorRuntimeSnapshot | null {
    return this.snapshot
  }

  // 清空快照（组件销毁时）。
  clear(): void {
    this.snapshot = null
  }
}
