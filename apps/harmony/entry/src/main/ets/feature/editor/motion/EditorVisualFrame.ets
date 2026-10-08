// EditorVisualFrame — 每帧视觉状态定义。
//
// Issue #879：写作区文字运动层的核心数据结构。
// 定义"裁切前沿"（front）概念：吐字时 front 从 x0 推进到 x1，
// 吞字时 front 从 x1 退到 x0。光标直接画在 front 上。
//
// 同一字形在每一帧只能由静态正文或运动层绘制一次——
// staticVisibleRanges 标记静态层应画的字形范围，
// motionGlyphWindows 标记运动层应画的字形窗口，
// 两者互不重叠。
//
// 所有坐标单位 vp，相对组件左上角。
// 所有文本 offset 是 UTF-16 code unit offset（ArkTS string.length 语义）。

/**
 * 运动类型枚举。
 *
 * - None：无运动（静态画面）。
 * - Reveal：吐字（插入新字符，front 从 x0 推进到 x1）。
 * - Conceal：吞字（Backspace 删除，front 从 x1 退到 x0）。
 * - ConcealForward：Forward Delete（逻辑光标不动，被删字贴向固定 caret 缩回）。
 * - Reflow：保留文字移动（因插入/删除导致后续文字位移）。
 * - CaretMove：纯光标移动（不涉及正文增删，只移动光标位置）。
 */
export enum MotionKind {
  None = 'none',
  Reveal = 'reveal',
  Conceal = 'conceal',
  ConcealForward = 'concealForward',
  Reflow = 'reflow',
  CaretMove = 'caretMove'
}

/**
 * 运动层字形窗口。
 *
 * 描述一个正在动画中的字形（或字形组）的裁切窗口和排版位置。
 * sourceKind 标识字形来自旧正文（'old'）还是新正文（'new'）：
 * - 吐字（Reveal）：sourceKind='new'，新字完整排版，裁切窗口从 [x0, x0] 扩展到 [x0, x1]。
 * - 吞字（Conceal）：sourceKind='old'，旧字源留在动画层，裁切窗口从 [x0, x1] 缩到 [x0, x0]。
 * - Forward Delete（ConcealForward）：sourceKind='old'，被删字贴向固定 caret 缩回。
 * - Reflow：sourceKind='new'，保留文字平移到新位置。
 */
export interface MotionGlyphWindow {
  /** 字形来源：'old'（旧正文）或 'new'（新正文） */
  sourceKind: 'old' | 'new'
  /** 字形在对应正文中的 UTF-16 起始 offset */
  utf16Start: number
  /** 字形在对应正文中的 UTF-16 结束 offset（exclusive） */
  utf16End: number
  /** 裁切窗口左边界（vp） */
  clipLeft: number
  /** 裁切窗口右边界（vp） */
  clipRight: number
  /** 裁切窗口上边界（vp） */
  clipTop: number
  /** 裁切窗口下边界（vp） */
  clipBottom: number
  /** 字形排版位置 x（vp） */
  glyphX: number
  /** 字形排版位置 y（vp） */
  glyphY: number
  /** 字形宽度（vp） */
  glyphWidth: number
  /** 字形高度（vp） */
  glyphHeight: number
}

/**
 * 静态层可见字形范围。
 *
 * 标记静态正文层在这一帧应该绘制的字形范围。
 * 运动层负责的字形范围不包含在内——同一字形每帧只画一次。
 */
export interface StaticVisibleRange {
  /** UTF-16 起始 offset */
  utf16Start: number
  /** UTF-16 结束 offset（exclusive） */
  utf16End: number
}

/**
 * 跨行光标移动的分段轨迹。
 *
 * 光标跨行移动时（如 Up/Down 键），按真实视觉行拆段，
 * 每段是一条直线轨迹（同行内水平移动 + 换行垂直跳转）。
 */
export interface MotionSegment {
  /** 段起始光标 x（vp） */
  startCaretX: number
  /** 段起始光标 y（vp） */
  startCaretY: number
  /** 段结束光标 x（vp） */
  endCaretX: number
  /** 段结束光标 y（vp） */
  endCaretY: number
}

/**
 * 编辑器视觉帧——某一时刻的完整视觉状态。
 *
 * 这是运动协调器（EditorMotionCoordinator）输出的数据结构，
 * 描述某一帧（可能是动画中间帧或最终静态帧）的完整视觉布局：
 * - 光标位置和高度。
 * - 静态层应画的字形范围（staticVisibleRanges）。
 * - 运动层应画的字形窗口（motionGlyphWindows）。
 * - 是否正在动画中。
 * - 运动类型。
 *
 * 渲染层（EditorMotionLayer）消费本结构，用 clipShape/maskShape
 * 对真实 ArkUI Text 绘制结果做遮罩，实现吐字吞字效果。
 */
export interface EditorVisualFrame {
  /** 光标 x 位置（vp）——动画中等于裁切前沿 front */
  caretX: number
  /** 光标 y 位置（vp） */
  caretY: number
  /** 光标高度（vp） */
  caretHeight: number
  /** 静态层可见字形范围列表 */
  staticVisibleRanges: StaticVisibleRange[]
  /** 运动层字形窗口列表 */
  motionGlyphWindows: MotionGlyphWindow[]
  /** 是否正在动画中 */
  isAnimating: boolean
  /** 运动类型 */
  motionKind: MotionKind
}
