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
// Issue #879 本轮复核评论修复（问题5）：
// - 新增多通道结构 InsertRun / DeletedRun / RetainedMove / GlyphRect。
// - EditorMotionPlan 不再用单一 motionKind 决定整个事务，改为同时容纳多通道。
// - 保留 MotionKind 枚举用于 EditorVisualFrame.motionKind（表示当前帧的主要运动类型，
//   供 needsMotionLayer 判断），但 plan 不再用单一 motionKind 决定整个事务。
//
// Issue #879 本轮复核评论修复（字形身份与连续编辑衔接）：
// - MotionGlyphWindow 新增 runId / sourceRevision / glyphIds / windowId / currentClipLeft /
//   currentClipRight / currentPositionX / currentPositionY 字段。
// - 连续快速编辑时，新 plan 的起始帧应以当前帧的实际屏幕状态（clip 起点位置）
//   而非旧版整字初态矩形作为下一动画起点，避免快速打字/连续 Backspace 从整字初态跳起。
//
// Issue #879 复核评论6078682695 修复：
// - 问题1：InsertRun/DeletedRun/RetainedMove 新增 pieces: RunStartPiece[] 字段——
//   多段可见片段不再合并成连续矩形，每个 piece 各有独立 startClipLeft/Right，
//   buildFrame 按 piece 分别采样，每个 piece 生成独立的 MotionGlyphWindow。
// - 问题2：EditorMotionPlan 新增 carryOverGhostRuns: CarryOverGhostRun[]——
//   上一帧仍有非零 clip、尚未归还静态层的旧窗口作为残影继承到候选 plan，
//   buildFrame 每帧同时采样本轮新 run 与 carryOver ghost。
//
// 所有坐标单位 vp，相对组件左上角。
// 所有文本 offset 是 UTF-16 code unit offset（ArkTS string.length 语义）。

import type { RunStartPiece } from './editor_commit_start'

/**
 * 运动类型枚举。
 *
 * - None：无运动（静态画面）。
 * - Reveal：吐字（插入新字符，front 从 x0 推进到 x1）。
 * - Conceal：吞字（Backspace 删除，front 从 x1 退到 x0）。
 * - ConcealForward：Forward Delete（逻辑光标不动，被删字贴向固定 caret 缩回）。
 * - Reflow：保留文字移动（因插入/删除导致后续文字位移）。
 * - CaretMove：纯光标移动（不涉及正文增删，只移动光标位置）。
 *
 * Issue #879 本轮复核评论修复（问题5）：MotionKind 仅用于 EditorVisualFrame.motionKind，
 * 表示当前帧的主要运动类型（供 needsMotionLayer 判断）。
 * EditorMotionPlan 不再用单一 motionKind 决定整个事务——
 * 一次事务可同时包含吐字/吞字/保留字移动/光标移动多个通道。
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
 * 字形矩形几何（vp，相对组件左上角）。
 */
export interface GlyphRect {
  /** 左边界（vp） */
  x: number
  /** 上边界（vp） */
  y: number
  /** 宽度（vp） */
  width: number
  /** 高度（vp） */
  height: number
}

/**
 * 吐字通道——新插入的字。
 *
 * 新字源完整排版，裁切窗口从起始状态扩展到目标状态 [x0, x1]。
 * 跨行时按视觉行分别计算裁切窗口——每个 InsertRun 对应一个视觉行内的连续字片段，
 * 不拿一个 x0→x1 横向跨行。
 *
 * Issue #879 最新复核评论修复（问题2）：
 * - 新增 startClipLeft/startClipRight 字段——起始裁切窗口状态。
 *   连续快速编辑时，起始状态来自上一帧的 MotionGlyphWindow.currentClipLeft/Right，
 *   而非固定的 [x0, x0]（零宽度）。
 * - Coordinator 的 sampleFrame 从 startClipLeft/Right 插值到 rect.x/rect.x+rect.width，
 *   而非从 x0→x1。
 */
export interface InsertRun {
  /** 字形在新正文中的 UTF-16 起始 offset */
  utf16Start: number
  /** 字形在新正文中的 UTF-16 结束 offset（exclusive） */
  utf16End: number
  /** 字形来源：固定 'new'（吐字总是用新正文源） */
  sourceKind: 'new'
  /** 字形在新排版中的位置（vp）——目标状态 */
  rect: GlyphRect
  /** 起始裁切窗口左边界（vp）——连续编辑时来自上一帧的 currentClipLeft，默认 rect.x */
  startClipLeft: number
  /** 起始裁切窗口右边界（vp）——连续编辑时来自上一帧的 currentClipRight，默认 rect.x（零宽度） */
  startClipRight: number
  /**
   * Issue #879 复核评论问题4：该 run 覆盖的所有字符簇的稳定身份列表（字形单元身份）。
   *
   * 由身份表（editor_glyph_identity.ts，随每笔编辑逐笔搬运字符簇身份）给出，
   * 位置平移、old/new 角色变化都不改身份。
   * 一个 run 可能覆盖多个字符簇（跨行片段、多字连续区间），
   * 与上一帧窗口接续时以本列表的幸存 glyphId 交集建立重分段映射，
   * 不再拿"窗口第一个字"代表整段窗口身份。
   */
  glyphIds: string[]
  /**
   * Issue #879 复核评论问题4：独立于字形单元身份的窗口身份。
   *
   * 由本 run 覆盖的完整字符簇身份序列派生（win-<len>-<fnv1a-hash>），
   * 因此位置平移、折行重分段只要覆盖的簇序列不变就保持稳定——它是"一整段文字裁切窗口"的身份，
   * 与单个字形单元的 glyphId 不是同一种对象。不同簇序列不会碰撞。
   */
  windowId: string
  /**
   * Issue #879 复核评论6078682695 问题1：按 piece 分开的起始可见区间。
   *
   * 多段可见片段不再合并成连续矩形——每个 piece 各有独立 startClipLeft/Right，
   * buildFrame 按 piece 分别采样，每个 piece 生成独立的 MotionGlyphWindow。
   * 区间之间的空洞绝不填补。
   */
  pieces: RunStartPiece[]
}

/**
 * 吞字通道——被删的旧字。
 *
 * 旧字源留在动画层，裁切窗口从起始状态缩到目标状态。
 * 跨行时按视觉行分别计算裁切窗口。
 *
 * Forward Delete 是 DeletedRun 的特例：逻辑光标不动，被删字贴向固定 caret 缩回。
 *
 * Issue #879 最新复核评论修复（问题2）：
 * - 新增 startClipLeft/startClipRight 字段——起始裁切窗口状态。
 *   连续快速编辑时，起始状态来自上一帧的 MotionGlyphWindow.currentClipLeft/Right，
 *   而非固定的 [x0, x1]（完整宽度）。
 * - Coordinator 的 sampleFrame 从 startClipLeft/Right 插值到目标 clip 状态，
 *   而非从 x0→x1。
 */
export interface DeletedRun {
  /** 字形在旧正文中的 UTF-16 起始 offset */
  utf16Start: number
  /** 字形在旧正文中的 UTF-16 结束 offset（exclusive） */
  utf16End: number
  /** 字形来源：固定 'old'（吞字总是用旧正文源） */
  sourceKind: 'old'
  /** 字形在旧排版中的位置（vp）——目标状态 */
  rect: GlyphRect
  /** 是否为 Forward Delete（逻辑光标不动，被删字贴向固定 caret 缩回） */
  isForward: boolean
  /** 起始裁切窗口左边界（vp）——连续编辑时来自上一帧的 currentClipLeft，默认 rect.x */
  startClipLeft: number
  /** 起始裁切窗口右边界（vp）——连续编辑时来自上一帧的 currentClipRight，默认 rect.x+rect.width */
  startClipRight: number
  /** Issue #879 复核评论问题4：该 run 覆盖的所有字符簇身份列表——见 InsertRun.glyphIds */
  glyphIds: string[]
  /** Issue #879 复核评论问题4：独立于字形单元身份的窗口身份——见 InsertRun.windowId */
  windowId: string
  /** Issue #879 复核评论6078682695 问题1：按 piece 分开的起始可见区间——见 InsertRun.pieces */
  pieces: RunStartPiece[]
}

/**
 * 保留字移动通道——offsetMap 中 Shifted 的保留字。
 *
 * 旧位置 → 新位置插值平移。动画期间用旧字源绘制并平移到新位置，
 * 动画结束后归还静态层（按新位置绘制）。
 *
 * Issue #879 本轮复核评论修复（问题5）：
 * 旧实现 Reflow 分支"直接在新位置画，不做平移动画"——
 * 输入导致后续字换行时，后续字不会从旧行移到新行。
 * 现在用 RetainedMove 记录 oldRect → newRect，做真正的平移动画。
 *
 * Issue #879 最新复核评论修复（问题2）：
 * - 新增 startPositionX/startPositionY 字段——起始位置状态。
 *   连续快速编辑时，起始状态来自上一帧的 MotionGlyphWindow.currentPositionX/Y，
 *   而非固定的 oldRect.x/y。
 * - Coordinator 的 sampleFrame 从 startPositionX/Y 插值到 newRect.x/y，
 *   而非从 oldRect.x/y。
 */
export interface RetainedMove {
  /** 字形在旧正文中的 UTF-16 起始 offset */
  oldUtf16Start: number
  /** 字形在旧正文中的 UTF-16 结束 offset（exclusive） */
  oldUtf16End: number
  /** 字形在新正文中的 UTF-16 起始 offset */
  newUtf16Start: number
  /** 字形在新正文中的 UTF-16 结束 offset（exclusive） */
  newUtf16End: number
  /** 字形来源：'old'（动画期间用旧字源绘制并平移到新位置） */
  sourceKind: 'old'
  /** 旧位置矩形（vp）——目标动画的起始参考 */
  oldRect: GlyphRect
  /** 新位置矩形（vp）——目标状态 */
  newRect: GlyphRect
  /** 起始位置 x（vp）——连续编辑时来自上一帧的 currentPositionX，默认 oldRect.x */
  startPositionX: number
  /** 起始位置 y（vp）——连续编辑时来自上一帧的 currentPositionY，默认 oldRect.y */
  startPositionY: number
  /** Issue #879 复核评论问题4：该 run 覆盖的所有字符簇身份列表——见 InsertRun.glyphIds */
  glyphIds: string[]
  /** Issue #879 复核评论问题4：独立于字形单元身份的窗口身份——见 InsertRun.windowId */
  windowId: string
  /** Issue #879 复核评论6078682695 问题1：按 piece 分开的起始可见区间——见 InsertRun.pieces */
  pieces: RunStartPiece[]
}

/**
 * Issue #879 复核评论6078682695 问题2：carryOver ghost 残影通道。
 *
 * 上一帧仍有非零 clip、且尚未归还静态层的旧窗口作为残影继承到候选 plan。
 * buildFrame 每帧同时采样本轮新 run 与 carryOver ghost，
 * 保持其旧源 TextController/StyledString 有效，直到残影真正隐藏才释放。
 *
 * 跨事务残影场景：rev10 `ab`→rev11 `a`（`b` 半吞）→rev12 `ac`，
 * 新候选第一帧仍含 old-10 的半个 `b`，并继续消退。
 */
export interface CarryOverGhostRun {
  /** 该残影覆盖的字符簇身份列表 */
  glyphIds: string[]
  /** 独立于字形单元身份的窗口身份 */
  windowId: string
  /** 该残影的源修订号——按此查不可变源排版记录 */
  sourceRevision: number
  /** 该残影在源正文中的 UTF-16 起始 offset */
  utf16Start: number
  /** 该残影在源正文中的 UTF-16 结束 offset（exclusive） */
  utf16End: number
  /** 当前裁切左边界（vp）——继承自上一帧的 currentClipLeft */
  currentClipLeft: number
  /** 当前裁切右边界（vp）——继承自上一帧的 currentClipRight */
  currentClipRight: number
  /** 目标裁切左边界（vp）——残影消退的终点（零宽度） */
  targetClipLeft: number
  /** 目标裁切右边界（vp）——残影消退的终点（零宽度） */
  targetClipRight: number
  /** 裁切窗口上边界（vp） */
  clipTop: number
  /** 裁切窗口下边界（vp） */
  clipBottom: number
  /** 字形排版位置 x（vp）——源 StyledString 排版出的原始位置 */
  glyphX: number
  /** 字形排版位置 y（vp） */
  glyphY: number
  /** 字形宽度（vp） */
  glyphWidth: number
  /** 字形高度（vp） */
  glyphHeight: number
  /** 绘制偏移 x（vp）——继承自上一帧的 offsetX */
  offsetX: number
  /** 绘制偏移 y（vp）——继承自上一帧的 offsetY */
  offsetY: number
}

/**
 * Issue #879 复核评论6081596024 问题1：跨事务延续的运动窗口。
 *
 * 上一帧的 insert（吐字）或 reflow（保留字移动）窗口如果尚未完成动画，
 * 且其字形不属于本次新 plan 的任何 run，需要继承到新 plan 继续运动。
 * 与 CarryOverGhostRun 不同——这些字形仍存在于新版正文中，
 * 需要从冻结视觉状态继续向目标运动，而不是消退到零宽度。
 *
 * - carryOverInsert：上一帧正在吐出的字，继续从当前 clip 扩展到完整可见
 * - carryOverReflow：上一帧正在移动的字，继续从当前位置移动到新版布局位置
 */
export interface CarryOverMotionRun {
  /** 运动所有权类型——'insert' 或 'reflow' */
  motionOwnerKind: 'insert' | 'reflow'
  /** 该窗口覆盖的字符簇身份列表 */
  glyphIds: string[]
  /** 独立于字形单元身份的窗口身份 */
  windowId: string
  /** 该窗口的源修订号——按此查不可变源排版记录 */
  sourceRevision: number
  /** 该窗口在源正文中的 UTF-16 起始 offset */
  utf16Start: number
  /** 该窗口在源正文中的 UTF-16 结束 offset（exclusive） */
  utf16End: number
  /** 当前裁切左边界（vp）——继承自上一帧的 currentClipLeft */
  currentClipLeft: number
  /** 当前裁切右边界（vp）——继承自上一帧的 currentClipRight */
  currentClipRight: number
  /** 目标裁切左边界（vp）——运动终点 */
  targetClipLeft: number
  /** 目标裁切右边界（vp）——运动终点 */
  targetClipRight: number
  /** 裁切窗口上边界（vp） */
  clipTop: number
  /** 裁切窗口下边界（vp） */
  clipBottom: number
  /** 字形排版位置 x（vp）——源 StyledString 排版出的原始位置 */
  glyphX: number
  /** 字形排版位置 y（vp） */
  glyphY: number
  /** 字形宽度（vp） */
  glyphWidth: number
  /** 字形高度（vp） */
  glyphHeight: number
  /** 绘制偏移 x（vp）——继承自上一帧的 offsetX */
  offsetX: number
  /** 绘制偏移 y（vp）——继承自上一帧的 offsetY */
  offsetY: number
  /** 当前位置 x（vp）——继承自上一帧的 currentPositionX */
  currentPositionX: number
  /** 当前位置 y（vp）——继承自上一帧的 currentPositionY */
  currentPositionY: number
}

/**
 * 运动层字形窗口。
 *
 * 描述一个正在动画中的字形（或字形组）的裁切窗口和排版位置。
 * sourceKind 标识字形来自旧正文（'old'）还是新正文（'new'）：
 * - 吐字（Reveal）：sourceKind='new'，新字完整排版，裁切窗口从 [x0, x0] 扩展到 [x0, x1]。
 * - 吞字（Conceal）：sourceKind='old'，旧字源留在动画层，裁切窗口从 [x0, x1] 缩到 [x0, x0]。
 * - Forward Delete（ConcealForward）：sourceKind='old'，被删字贴向固定 caret 缩回。
 * - Reflow：sourceKind='old'，保留文字从旧位置平移到新位置（用旧字源绘制 + offset 平移）。
 *
 * Issue #879 本轮复核评论修复（问题5）：
 * MotionGlyphWindow 仍保留——由 Coordinator 从多通道 plan（InsertRun/DeletedRun/RetainedMove）
 * 采样生成，描述某一帧运动层要画的字形窗口。EditorMotionLayer 消费本结构做 clipShape 裁切。
 * 新增 offsetX/offsetY 字段支持 RetainedMove 的平移动画（旧字源绘制后整体偏移到当前插值位置）。
 *
 * Issue #879 本轮复核评论修复（字形身份与连续编辑衔接）：
 * 新增 runId / sourceRevision / glyphIds / windowId / currentClipLeft / currentClipRight /
 * currentPositionX / currentPositionY 字段。连续快速编辑时，新 plan 的起始帧应使用
 * 当前帧的实际屏幕状态（clip 起点位置）而非旧版整字初态矩形作为下一动画起点。
 * - runId：与 EditorMotionSourcePool 的 runId 对应，ForEach 中保持稳定身份。
 * - sourceRevision：识别字形来自哪个版本，快速打字时新 plan 起始帧用当前帧的 revision。
 * - glyphIds：本窗口覆盖的字符簇身份列表，跨版本稳定——
 *   同一字符在上一笔是"new"下一笔是"old"时身份不变，位置平移也不改身份。
 * - currentClipLeft/Right：当前帧实际裁切窗口边界，作为下一动画起点而非整字初态 x0/x1。
 * - currentPositionX/Y：当前帧实际字形位置（含 offset），作为下一动画起点而非旧版整字初态位置。
 *
 * Issue #879 本轮复核评论修复（问题2）：
 * - 新增 renderNodeKey 字段——分离 ForEach/TextController 管理和跨版本匹配。
 *   renderNodeKey = sourceKind + '-' + sourceRevision + '-' + windowId，
 *   当字形源从 new-rN 变为 old-rN 时 renderNodeKey 改变，ForEach 创建新 Text 节点而非复用旧节点。
 *   glyphIds 只用于寻找上一帧相同字形的 clip/位置——跨版本匹配。
 *   更换内容源时对应布局 ready 失效（新 renderNodeKey → 新窗口状态 → layoutReady=false）。
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
  /** 字形排版位置 x（vp）——源 StyledString 排版出的原始位置 */
  glyphX: number
  /** 字形排版位置 y（vp）——源 StyledString 排版出的原始位置 */
  glyphY: number
  /** 字形宽度（vp） */
  glyphWidth: number
  /** 字形高度（vp） */
  glyphHeight: number
  /** 绘制偏移 x（vp）——RetainedMove 平移动画时把旧字源整体偏移到当前插值位置 */
  offsetX: number
  /** 绘制偏移 y（vp）——RetainedMove 平移动画时把旧字源整体偏移到当前插值位置 */
  offsetY: number
  /** 运动源 run 身份标识——与 EditorMotionSourcePool 的 runId 对应，用于在 ForEach 中保持稳定身份 */
  runId: string
  /** 字形源对应的正文 revision——连续编辑时识别字形来自哪个版本 */
  sourceRevision: number
  /**
   * Issue #879 复核评论问题4：该窗口覆盖的所有字符簇的稳定身份列表（字形单元身份）。
   *
   * 用于与上一帧窗口按 glyphId 交集匹配——查找时用 glyphIds 数组逐个查，
   * 取交集最大的窗口作为匹配，不再仅靠起始簇身份匹配。
   */
  glyphIds: string[]
  /**
   * Issue #879 复核评论6077187962 问题4：独立于字形单元身份的窗口身份（见 InsertRun.windowId）。
   *
   * 字形单元 id 是单个字符簇的身份；windowId 是"一整段文字裁切窗口"的身份，
   * 由覆盖的簇身份序列派生。窗口与上帧接续一律走 glyphIds 交集映射，
   * 本字段只描述窗口自身。
   *
   * windowId 的长度随 glyphIds 数量线性增长，不适合直接用作 ArkUI 节点 ID。
   * 它保留在数据结构中作为算法内部的精确身份（用于匹配/查找）。
   */
  windowId: string
  /**
   * Issue #879 复核评论6077187962 问题4：有限长度的窗口实例 ID。
   *
   * 格式为 `win-<number>`（如 `win-0`, `win-1`），由 WindowInstanceIdAllocator 分配。
   * 长度固定，不随 glyphIds 数量增长，专门用于 ArkUI 节点 ID / ForEach key /
   * ComponentObserver 索引等渲染层场景。
   *
   * 同一 glyphIds 序列在活跃期间始终映射到同一个 windowInstanceId；
   * 窗口释放后该 ID 可被重新分配给不同的 glyphIds。
   */
  windowInstanceId: string
  /**
   * 渲染节点身份标识——用于 ForEach key 和 TextController/窗口状态管理。
   *
   * Issue #879 本轮复核评论修复（问题2）：
   * renderNodeKey = sourceKind + '-' + sourceRevision + '-' + windowInstanceId。
   * 当字形源从 new-rN 变为 old-rN 时 renderNodeKey 改变——ForEach 创建新 Text 节点而非复用旧节点。
   * 这解决了"上一帧 new 字形变成下一笔 old 字形时 ForEach 复用同一 Text 节点，
   * 但源已从 new-rN 切换为 old-rN，onAppear 不再触发，节点仍画原先的 StyledString"的问题。
   *
   * Issue #879 复核评论6077187962 问题4：renderNodeKey 现在使用 windowInstanceId（win-<number>）
   * 而非 windowId（长度随 glyphIds 线性增长）。windowInstanceId 是有限长度的，
   * 保证 renderNodeKey 不随窗口覆盖的字符簇数量增长。
   * windowId 仍保留在数据结构中作为算法内部的精确身份（用于匹配/查找）。
   */
  renderNodeKey: string
  /** 当前帧的裁切窗口左边界（vp）——连续编辑时作为下一动画起点而非整字初态的 x0 */
  currentClipLeft: number
  /** 当前帧的裁切窗口右边界（vp）——连续编辑时作为下一动画起点而非整字初态的 x1 */
  currentClipRight: number
  /** 当前帧的字形位置 x（含 offset，vp）——连续编辑时作为下一动画起点而非旧版整字初态位置 */
  currentPositionX: number
  /** 当前帧的字形位置 y（含 offset，vp）——连续编辑时作为下一动画起点而非旧版整字初态位置 */
  currentPositionY: number
  /**
   * Issue #879 复核评论6080604353 问题1：该窗口的运动所有权类型。
   *
   * 用于 identifyCarryOverGhosts 区分删除残影和正常保留字移动：
   * - 'deleted'：吞字通道（旧删除 ghost），应被继承为 carryOver ghost
   * - 'reflow'：保留字移动通道，**不应**被继承为 carryOver ghost
   * - 'insert'：吐字通道（新插入字），**不应**被继承为 carryOver ghost
   * - 'carryGhost'：上一帧继承的 carryOver ghost，应继续被继承直到完全消退
   */
  motionOwnerKind: 'deleted' | 'reflow' | 'insert' | 'carryGhost'
}

/**
 * 静态层可见字形范围。
 *
 * 标记静态正文层在这一帧应该绘制的字形范围。
 * 运动层负责的字形范围不包含在内——同一字形每帧只画一次。
 *
 * Issue #879 本轮复核评论修复（问题1）：
 * 静态可见区完整扣除所有被 motion 接管的目标 glyph 身份
 * （insertRuns + retainedMoves 的新位置 + deletedRuns 的旧位置），
 * 而不是按当前 MotionKind 临时扣一个空间矩形。
 * 主 Text 仍完整排版，但其绘制输出按 staticVisibleRanges 用 clipShape/maskShape 裁切——
 * 只显示未被运动接管的范围。
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
 *
 * Issue #879 本轮复核评论修复（问题1）：
 * staticVisibleRanges 用于主 Text 的 clipShape/maskShape 裁切——
 * 主 Text 仍完整排版，但只绘制 staticVisibleRanges 范围内的字形，
 * 被运动接管的字形从静态绘制输出中排除（不再用背景色 Rect 遮罩）。
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
  /** 运动类型——当前帧的主要运动类型，供 needsMotionLayer 判断 */
  motionKind: MotionKind
}
