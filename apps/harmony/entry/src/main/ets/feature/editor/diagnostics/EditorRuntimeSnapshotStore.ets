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
// Issue #768 评论5837824666：从真实 LayoutManager.LineMetrics 记录完整字段，
// 不再从 renderResult.lines 拼残缺的 currentLineMetrics。
export interface CurrentLineMetricsSnapshot {
  startIndex: number
  endIndex: number
  height: number
  baseline: number
  left: number
  topHeight: number
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

  // Issue #768 评论5837824666：布局身份字段。
  // 记录当前已发布的布局对应的 snapshot 身份，供诊断方判断是否出现"新 revision + 旧 layout"。
  // 若 revision !== layoutRevision，说明布局还没跟上 snapshot 变化。
  layoutRevision: number                // 布局对应的 snapshot revision
  layoutGeneration: number              // 布局对应的 snapshot generation
  layoutCompositionGeneration: number   // 布局对应的 snapshot compositionGeneration
  visualCaretAffinity: string           // 布局对应的 visual caret affinity ('upstream' | 'downstream')

  // Issue #768 评论5835171266 第5项：编辑器挂载状态和快照时间戳。
  isEditorMounted: boolean              // 编辑器是否仍挂载（aboutToDisappear 时设 false）
  capturedAtMs: number                  // 快照捕获时间戳（Date.now()）
}

// Issue #773 评论 5845033983：observer 类型，让 Checkpoint 能订阅 editor 状态变化。
// 不反向 import Checkpoint（避免循环依赖）。
export type EditorSnapshotObserver = (snapshot: EditorRuntimeSnapshot | null) => void

export class EditorRuntimeSnapshotStore {
  private static instance: EditorRuntimeSnapshotStore
  private snapshot: EditorRuntimeSnapshot | null = null
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

  // SujianEditor 调用，更新完整快照。
  // 接收完整 EditorRuntimeSnapshot（所有字段必填），直接替换当前快照。
  // SujianEditor 持有全部状态，每次从自身字段构造完整快照传入，
  // 保证诊断数据一致，不会出现新旧字段混合。
  updateSnapshot(snapshot: EditorRuntimeSnapshot): void {
    this.snapshot = snapshot
    this.notifyObservers()
  }

  // 诊断导出器调用，读取当前快照。
  getSnapshot(): EditorRuntimeSnapshot | null {
    return this.snapshot
  }

  // Issue #768 评论5835171266 第5项：编辑器卸载时标记 isEditorMounted=false。
  // 不清空快照，保留最后状态供离页后导出诊断。下次打开编辑器时 updateSnapshot 会覆盖。
  markEditorUnmounted(): void {
    if (this.snapshot !== null) {
      // 构造新快照，把 isEditorMounted 设为 false，其余字段保留。
      // ArkTS 不支持异构 spread，必须逐字段构造。
      const old = this.snapshot
      this.snapshot = {
        hasActiveSession: old.hasActiveSession,
        revision: old.revision,
        generation: old.generation,
        textUtf8Bytes: old.textUtf8Bytes,
        textUtf16Length: old.textUtf16Length,
        cursorByteOffset: old.cursorByteOffset,
        cursorUtf16Offset: old.cursorUtf16Offset,
        selectionAnchorByte: old.selectionAnchorByte,
        selectionHeadByte: old.selectionHeadByte,
        hasComposition: old.hasComposition,
        compositionStartByte: old.compositionStartByte,
        compositionEndByte: old.compositionEndByte,
        fontSizeVp: old.fontSizeVp,
        lineSpacingMultiplier: old.lineSpacingMultiplier,
        effectiveLineHeightVp: old.effectiveLineHeightVp,
        firstLineIndentEnabled: old.firstLineIndentEnabled,
        firstLineIndentChars: old.firstLineIndentChars,
        effectiveFirstLineIndentVp: old.effectiveFirstLineIndentVp,
        viewportWidthPx: old.viewportWidthPx,
        viewportHeightPx: old.viewportHeightPx,
        contentWidthPx: old.contentWidthPx,
        contentHeightPx: old.contentHeightPx,
        systemLayoutLineCount: old.systemLayoutLineCount,
        caretRect: old.caretRect,
        selectionRectCount: old.selectionRectCount,
        currentLineMetrics: old.currentLineMetrics,
        imeAttached: old.imeAttached,
        editorFocused: old.editorFocused,
        currentScrollX: old.currentScrollX,
        currentScrollY: old.currentScrollY,
        leadingWhitespaceCountOfCurrentParagraph: old.leadingWhitespaceCountOfCurrentParagraph,
        layoutSource: old.layoutSource,
        usingLayoutManager: old.usingLayoutManager,
        apiLevel: old.apiLevel,
        lastLayoutError: old.lastLayoutError,
        fallbackCount: old.fallbackCount,
        layoutRevision: old.layoutRevision,
        layoutGeneration: old.layoutGeneration,
        layoutCompositionGeneration: old.layoutCompositionGeneration,
        visualCaretAffinity: old.visualCaretAffinity,
        isEditorMounted: false,
        capturedAtMs: old.capturedAtMs,
      }
      this.notifyObservers()
    }
  }

  // 清空快照（仅在完全重置时使用，aboutToDisappear 不再调用）。
  clear(): void {
    this.snapshot = null
    this.notifyObservers()
  }
}
