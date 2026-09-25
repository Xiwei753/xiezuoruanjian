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
//
// Issue #768：补齐 Issue 要求的全部诊断字段。updateSnapshot 接收完整快照，
// 不用 Partial<T>（ArkTS 严格模式不支持 mapped type）也不用对象 spread 合并
// （ArkTS 不支持异构类型 spread）。SujianEditor 持有全部状态，每次构造完整快照传入，
// 保证诊断数据一致（不会出现部分字段是旧值、部分是新值的混合状态）。

// 光标矩形快照（px，相对组件左上）。
export interface CaretRectSnapshot {
  x: number
  y: number
  width: number
  height: number
}

// 当前行度量快照。startIndex/endIndex 为 UTF-16 code unit offset。
// baseline 为 0 时表示不可用（LineLayout 不含 baseline，仅 LayoutManager.LineMetrics 有）。
export interface CurrentLineMetricsSnapshot {
  startIndex: number
  endIndex: number
  height: number
  baseline: number
}

// 编辑器运行时快照 — 全部脱敏，不含正文内容。
// Issue #768 要求的字段全部包含。
export interface EditorRuntimeSnapshot {
  // 会话/版本
  hasActiveSession: boolean             // 是否有活跃编辑会话
  revision: number                      // 当前编辑器 revision
  generation: number                    // Core 编辑状态 generation

  // 文本长度（不含内容）
  textUtf8Bytes: number                 // 正文 UTF-8 字节数
  textUtf16Length: number               // 正文 UTF-16 code unit 数

  // 光标/选区（byte offset）
  cursorByteOffset: number              // 光标 UTF-8 byte 偏移
  cursorUtf16Offset: number             // 光标 UTF-16 code unit 偏移
  selectionAnchorByte: number           // 选区锚点 UTF-8 byte 偏移
  selectionHeadByte: number             // 选区终点 UTF-8 byte 偏移

  // composition
  hasComposition: boolean               // 是否有输入法 composition
  compositionStartByte: number          // composition 起点 UTF-8 byte 偏移（无 composition 时 -1）
  compositionEndByte: number            // composition 终点 UTF-8 byte 偏移（无 composition 时 -1）

  // 排版设置
  fontSizeVp: number                    // 字号 vp
  lineSpacingMultiplier: number         // 行高倍数
  effectiveLineHeightVp: number         // 实际行高 = fontSize * lineSpacingMultiplier
  firstLineIndentEnabled: boolean       // 首行缩进开关
  firstLineIndentChars: number          // 首行缩进字符数（autoIndentWidth）
  effectiveFirstLineIndentVp: number    // 实际首行缩进 = enabled ? indentChars * fontSize : 0

  // 视口/内容尺寸（px）
  viewportWidthPx: number               // 视口宽度
  viewportHeightPx: number              // 视口高度
  contentWidthPx: number                // 内容区域宽度
  contentHeightPx: number               // 内容区域高度

  // 布局几何
  systemLayoutLineCount: number         // 系统布局行数
  caretRect: CaretRectSnapshot | null   // 光标矩形
  selectionRectCount: number            // 选区矩形数
  currentLineMetrics: CurrentLineMetricsSnapshot | null  // 当前行度量

  // IME/焦点/滚动
  imeAttached: boolean                  // IME 是否连接
  editorFocused: boolean                // 编辑器是否获焦
  currentScrollX: number                // 当前水平滚动偏移 px
  currentScrollY: number                // 当前垂直滚动偏移 px

  // 段落前导空白
  leadingWhitespaceCountOfCurrentParagraph: number  // 当前段落前导空白字符数

  // 布局来源/降级（保留现有有用字段）
  layoutSource: string                  // 'system' | 'fallback' | 'none'
  usingLayoutManager: boolean           // 是否使用系统 LayoutManager
  apiLevel: number                      // 当前 API Level
  lastLayoutError: string | null        // 最后一次布局错误信息
  fallbackCount: number                 // fallback 次数累计
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

  // SujianEditor 调用，更新完整快照。
  // 接收完整 EditorRuntimeSnapshot（所有字段必填），直接替换当前快照。
  // SujianEditor 持有全部状态，每次从自身字段构造完整快照传入，
  // 保证诊断数据一致，不会出现新旧字段混合。
  updateSnapshot(snapshot: EditorRuntimeSnapshot): void {
    this.snapshot = snapshot
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
