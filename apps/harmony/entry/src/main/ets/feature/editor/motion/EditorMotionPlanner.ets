// EditorMotionPlanner — 运动规划器。
//
// Issue #879：从 editResult.displayPatches[] 读区间，规划文字运动。
//
// 核心逻辑：
// 1. 从 displayPatches 读 replaceByteStart/End（旧正文范围）和 insertedByteStart/End（新正文范围）。
// 2. 用 offsetMap 识别保留文字的 reflow（旧正文中的字符在新正文中的对应位置）。
// 3. NoChange、纯选区（CursorOnly）、ImeComposition 预编辑变动不生成正文增删 motion。
// 4. IME commit 后的真实 committed patch 才参与正文运动。
// 5. Forward Delete 保持逻辑光标静止——被删字贴向固定 caret 缩回。
//
// Issue #879 复核评论修复：
// - 入参改为 oldSnapshot + oldLayout + newSnapshot + newLayout + editResult + lastVisibleFrame
// - 旧正文范围用 oldSnapshot.text 解码（patch.replaceByteStart/End 是旧正文 UTF-8 区间）
// - 新正文范围用 newSnapshot.text 解码（patch.insertedByteStart/End 是新正文 UTF-8 区间）
// - 旧字形几何用 oldLayout 计算，新字形几何用 newLayout 计算
// - 跨行 caret 拆段：按旧行末与新行首等实际停靠点分段，不做穿字斜线
//
// Issue #879 本轮复核评论修复（问题5）：
// - EditorMotionPlan 改为多通道结构：insertRuns / deletedRuns / retainedMoves / caretPath。
// - planFromPatches 同时收集 insertRuns（新插入字）、deletedRuns（被删旧字）、
//   retainedMoves（offsetMap 中 Shifted 的保留字，用 oldLayout 算 oldRect、newLayout 算 newRect）。
// - 保留 motionKind 字段表示当前帧的主要运动类型（供 needsMotionLayer 判断），
//   但 plan 不再用单一 motionKind 决定整个事务。
// - 跨行按视觉行分别计算裁切窗口（splitRectByLines），不拿一个 x0→x1 横向跨行。
//
// 不修改 Core 的编辑事实模型，只读 editResult 做规划。
// 所有 byte offset 通过 TextOffsetMapper 转 UTF-16 offset 供渲染层使用。

import { EditorEditResult, DisplayPatch, EditorOperationKind, OffsetMap, OffsetMapEntry, OffsetMapKind } from '../../../corebridge/dto/EditorDtos'
import { EditorVisualFrame, MotionKind, MotionSegment, InsertRun, DeletedRun, RetainedMove, GlyphRect, CarryOverGhostRun, CarryOverMotionRun } from './EditorVisualFrame'
import { EditorLayoutSnapshot } from '../render/EditorLayoutSnapshot'
import { TextOffsetMapper } from '../input/TextOffsetMapper'
import { LineLayout } from '../render/editor_render_geometry'
import { GlyphIdentityTable, GlyphIdentityEntry, windowIdForGlyphIds, clusterBoundaries } from './editor_glyph_identity'
import {
  DisplayedContext, FrozenWindowSpan, RunGeometry, RunStartPiece,
  deletedRunStartState, insertRunStartState, retainedMoveStartState
} from './editor_commit_start'

/**
 * Issue #879 最新复核评论问题3：保留字移动的中间结构。
 *
 * identifyRetainedMoves 只负责几何配对，不决定字形身份——
 * 字形身份（glyphIds）由调用方用身份表按旧正文位置解析，避免中间层凭空造身份。
 */
interface RetainedMoveSpan {
  oldUtf16Start: number
  oldUtf16End: number
  newUtf16Start: number
  newUtf16End: number
  sourceKind: 'old' | 'new'
  oldRect: GlyphRect
  newRect: GlyphRect
  startPositionX: number
  startPositionY: number
}

/**
 * 受影响的正文范围（UTF-16 offset 对）。
 */
export interface AffectedRange {
  utf16Start: number
  utf16End: number
}

/**
 * 光标位置信息。
 */
export interface CaretPosition {
  x: number
  y: number
  height: number
}

/**
 * 运动规划结果——多通道结构。
 *
 * Issue #879 本轮复核评论修复（问题5）：
 * 一次编辑事务可同时包含多个通道：
 * - insertRuns：吐字（新插入字，裁切窗口从 [x0,x0] 扩到 [x0,x1]）。
 * - deletedRuns：吞字（被删旧字，裁切窗口从 [x0,x1] 缩到 [x0,x0]）。
 * - retainedMoves：保留字移动（offsetMap 中 Shifted 的保留字，oldRect → newRect 插值）。
 * - caretPath：光标轨迹（跨行时按视觉行拆段）。
 *
 * Coordinator 的 sampleFrame/advance 同一 progress 同时采样所有通道，
 * 不再用互斥的 if (motionKind === Reveal) 分支。
 *
 * motionKind 保留，表示当前帧的主要运动类型（供 needsMotionLayer 判断），
 * 但不再用单一 motionKind 决定整个事务。
 */
export interface EditorMotionPlan {
  /** 吐字通道：新插入字 */
  insertRuns: InsertRun[]
  /** 吞字通道：被删旧字 */
  deletedRuns: DeletedRun[]
  /** 保留字移动通道：offsetMap 中 Shifted 的保留字 */
  retainedMoves: RetainedMove[]
  /** 光标轨迹：跨行时按视觉行拆段 */
  caretPath: MotionSegment[]
  /** 动画时长（ms） */
  durationMs: number
  /** 主要运动类型——供 needsMotionLayer 判断，不决定整个事务 */
  motionKind: MotionKind
  /**
   * Issue #879 复核评论6078682695 问题2：carryOver ghost 残影通道。
   *
   * 上一帧仍有非零 clip、且尚未归还静态层的旧窗口作为残影继承到候选 plan。
   * buildFrame 每帧同时采样本轮新 run 与 carryOver ghost。
   * Planner 自身不产生 carryOverGhostRuns（由 Coordinator.retargetCandidate 识别），
   * 但 plan 接口需要容纳此字段。
   */
  carryOverGhostRuns: CarryOverGhostRun[]
  /**
   * Issue #879 复核评论6081596024 问题1：跨事务延续的运动窗口（insert/reflow）。
   *
   * 上一帧的 insert（吐字）或 reflow（保留字移动）窗口如果尚未完成动画，
   * 且其字形不属于本次新 plan 的任何 run，需要继承到新 plan 继续运动。
   * Planner 自身不产生 carryOverMotionRuns（由 Coordinator.retargetCandidate 识别），
   * 但 plan 接口需要容纳此字段。
   */
  carryOverMotionRuns: CarryOverMotionRun[]
}

/**
 * 字形矩形片段——一个视觉行内的连续字片段及其几何。
 * 用于跨行时按视觉行分别计算裁切窗口。
 */
export interface GlyphRectSegment {
  /** 该视觉行内的 UTF-16 起始 offset */
  utf16Start: number
  /** 该视觉行内的 UTF-16 结束 offset（exclusive） */
  utf16End: number
  /** 该视觉行内的几何矩形（vp） */
  rect: GlyphRect
}

/**
 * 运动规划器。
 *
 * 从 oldSnapshot + oldLayout + newSnapshot + newLayout + editResult 出发，
 * 规划文字运动方案。核心是解读 displayPatches 和 offsetMap，
 * 判断哪些文字需要吐字/吞字动画，哪些文字需要 reflow 平移。
 *
 * Issue #879 本轮复核评论修复（问题5）：
 * planFromPatches 同时收集 insertRuns / deletedRuns / retainedMoves，
 * 不再用单一 motionKind 决定整个事务。
 */
export class EditorMotionPlanner {
  /**
   * 规划运动方案。
   *
   * @param oldSnapshot 旧布局快照（含旧正文 text）
   * @param oldLayout 旧行布局（旧字形的排版几何）
   * @param newSnapshot 新布局快照（含新正文 text）
   * @param newLayout 新行布局（新字形的排版几何）
   * @param editResult 本次编辑事务结果
   * @param lastVisibleFrame 上一帧的视觉状态
   * @returns 运动规划结果（多通道结构）
   */
  static plan(
    oldSnapshot: EditorLayoutSnapshot,
    oldLayout: LineLayout[],
    newSnapshot: EditorLayoutSnapshot,
    newLayout: LineLayout[],
    editResult: EditorEditResult | null,
    lastVisibleFrame: EditorVisualFrame,
    oldIdentities: GlyphIdentityTable | null,
    newIdentities: GlyphIdentityTable | null
  ): EditorMotionPlan {
    // 无 editResult（初始加载/切换章节/字号变更）→ 无运动
    if (editResult === null) {
      return EditorMotionPlanner.emptyPlan()
    }

    // NoChange → 无运动
    if (editResult.outcome !== 'applied' && editResult.outcome !== 'appliedWithAdjustedSelection') {
      return EditorMotionPlanner.emptyPlan()
    }

    // 纯选区移动（CursorOnly）→ 只规划光标移动
    if (editResult.operationKind === EditorOperationKind.CursorOnly) {
      return EditorMotionPlanner.caretMovePlan(oldSnapshot, oldLayout, newSnapshot, newLayout, editResult, lastVisibleFrame)
    }

    // IME 预编辑变动（CompositionUpdate）→ 不生成正文增删 motion
    // 预编辑文字的显示由静态层处理（composition 下划线等），不参与吐字吞字动画
    if (editResult.operationKind === EditorOperationKind.CompositionUpdate ||
        editResult.operationKind === EditorOperationKind.CompositionCancel) {
      return EditorMotionPlanner.emptyPlan()
    }

    // IME commit 后的真实 committed patch → 参与正文运动
    // CompositionCommit 走正常的 displayPatches 分析流程

    // 从 displayPatches 分析正文增删
    return EditorMotionPlanner.planFromPatches(
      oldSnapshot, oldLayout, newSnapshot, newLayout, editResult, lastVisibleFrame,
      oldIdentities, newIdentities
    )
  }

  /**
   * 空规划——无运动。
   */
  private static emptyPlan(): EditorMotionPlan {
    return {
      insertRuns: [],
      deletedRuns: [],
      retainedMoves: [],
      caretPath: [],
      durationMs: 0,
      motionKind: MotionKind.None,
      carryOverGhostRuns: [],
      carryOverMotionRuns: []
    }
  }

  /**
   * 纯光标移动规划。
   *
   * Issue #879 复核评论修复：跨行 caret 拆段——按旧行末与新行首等实际停靠点分段。
   * 同行内水平移动 + 换行垂直跳转，不做穿字斜线。远距离点击直接跳转（不动画）。
   */
  private static caretMovePlan(
    oldSnapshot: EditorLayoutSnapshot,
    oldLayout: LineLayout[],
    newSnapshot: EditorLayoutSnapshot,
    newLayout: LineLayout[],
    editResult: EditorEditResult,
    lastVisibleFrame: EditorVisualFrame
  ): EditorMotionPlan {
    // 从新行布局计算新光标位置
    const cursorUtf16 = TextOffsetMapper.utf8ToUtf16(newSnapshot.text, newSnapshot.cursorByteOffset)
    const newCaret = EditorMotionPlanner.findCaretPosition(cursorUtf16, newLayout, newSnapshot.text)

    // 旧光标位置来自 lastVisibleFrame
    const oldCaretX = lastVisibleFrame.caretX
    const oldCaretY = lastVisibleFrame.caretY

    // 判断是否跨行
    const oldLineIndex = EditorMotionPlanner.findLineIndexByY(oldCaretY, oldLayout)
    const newLineIndex = EditorMotionPlanner.findLineIndexByY(newCaret.y, newLayout)
    const isCrossLine = oldLineIndex !== newLineIndex && oldLineIndex >= 0 && newLineIndex >= 0

    const caretPath: MotionSegment[] = []

    if (!isCrossLine) {
      // 同行移动：单段直线
      caretPath.push({
        startCaretX: oldCaretX,
        startCaretY: oldCaretY,
        endCaretX: newCaret.x,
        endCaretY: newCaret.y
      })
    } else {
      // 跨行移动：按实际停靠点分段
      const oldLine = oldLayout[oldLineIndex]
      const newLine = newLayout[newLineIndex]

      // 判断移动方向：向下移动还是向上移动
      const isMoveDown = newLineIndex > oldLineIndex

      if (isMoveDown) {
        // 向下移动：先在旧行水平移到行末，再垂直跳到新行，再水平移到目标
        caretPath.push({
          startCaretX: oldCaretX,
          startCaretY: oldCaretY,
          endCaretX: oldLine.left + EditorMotionPlanner.getLineEndX(oldLine, oldSnapshot.text),
          endCaretY: oldCaretY
        })
        caretPath.push({
          startCaretX: oldLine.left + EditorMotionPlanner.getLineEndX(oldLine, oldSnapshot.text),
          startCaretY: oldCaretY,
          endCaretX: newLine.left,
          endCaretY: newCaret.y
        })
        caretPath.push({
          startCaretX: newLine.left,
          startCaretY: newCaret.y,
          endCaretX: newCaret.x,
          endCaretY: newCaret.y
        })
      } else {
        // 向上移动：先在旧行水平移到行首，再垂直跳到新行末，再水平移到目标
        caretPath.push({
          startCaretX: oldCaretX,
          startCaretY: oldCaretY,
          endCaretX: oldLine.left,
          endCaretY: oldCaretY
        })
        caretPath.push({
          startCaretX: oldLine.left,
          startCaretY: oldCaretY,
          endCaretX: newLine.left + EditorMotionPlanner.getLineEndX(newLine, newSnapshot.text),
          endCaretY: newCaret.y
        })
        caretPath.push({
          startCaretX: newLine.left + EditorMotionPlanner.getLineEndX(newLine, newSnapshot.text),
          startCaretY: newCaret.y,
          endCaretX: newCaret.x,
          endCaretY: newCaret.y
        })
      }
    }

    return {
      insertRuns: [],
      deletedRuns: [],
      retainedMoves: [],
      caretPath,
      durationMs: 0, // 由 policy 决定实际时长
      motionKind: MotionKind.CaretMove,
      carryOverGhostRuns: [],
      carryOverMotionRuns: []
    }
  }

  /**
   * 从 displayPatches 分析正文增删，规划多通道运动方案。
   *
   * Issue #879 本轮复核评论修复（问题5）：
   * - 同时收集 insertRuns（新插入字）、deletedRuns（被删旧字）、retainedMoves（Shifted 保留字）。
   * - 跨行按视觉行分别计算裁切窗口（splitRectByLines），不拿一个 x0→x1 横向跨行。
   * - 保留 motionKind 表示主要运动类型，但不再用单一 motionKind 决定整个事务。
   *
   * Issue #879 复核评论修复：
   * - patch.replaceByteStart/End 是旧正文 UTF-8 区间，用 oldSnapshot.text 解码
   * - patch.insertedByteStart/End 是新正文 UTF-8 区间，用 newSnapshot.text 解码
   * - 旧字形几何用 oldLayout 计算，新字形几何用 newLayout 计算
   */
  private static planFromPatches(
    oldSnapshot: EditorLayoutSnapshot,
    oldLayout: LineLayout[],
    newSnapshot: EditorLayoutSnapshot,
    newLayout: LineLayout[],
    editResult: EditorEditResult,
    lastVisibleFrame: EditorVisualFrame,
    oldIdentities: GlyphIdentityTable | null,
    newIdentities: GlyphIdentityTable | null
  ): EditorMotionPlan {
    const patches: DisplayPatch[] = editResult.displayPatches
    const oldText = oldSnapshot.text
    const newText = newSnapshot.text

    // 无 patches → 无运动
    if (patches.length === 0) {
      return EditorMotionPlanner.emptyPlan()
    }

    // 判断 Forward Delete（用于 DeletedRun.isForward 标记）
    const isForwardDelete: boolean = EditorMotionPlanner.isForwardDelete(editResult)

    // Issue #879 复核评论6075662695问题3：起始状态按字形算——
    // 准备阶段用「当时的在屏状态」（lastVisibleFrame + 旧布局/旧身份表）算一次，
    // 提交瞬间 Coordinator 会用真实的 displayed 布局 + 冻结帧重算（retargetCandidate），
    // 因此这里不再抄旧窗口的绝对 clip，也不用「最大交集窗口的整体裁切范围」。
    const displayedContext: DisplayedContext = {
      text: oldText,
      layout: oldLayout,
      identities: oldIdentities,
      frozenWindows: EditorMotionPlanner.frozenWindowSpans(
        lastVisibleFrame, oldText, oldLayout, newText, newLayout,
        oldSnapshot.revision, newSnapshot.revision
      )
    }

    // 收集多通道
    const insertRuns: InsertRun[] = []
    const deletedRuns: DeletedRun[] = []
    const retainedMoves: RetainedMove[] = []

    for (const patch of patches) {
      // 旧正文被替换的范围（byte offset）→ 用 oldSnapshot.text 转 UTF-16
      const oldUtf16Start = TextOffsetMapper.utf8ToUtf16(oldText, patch.replaceByteStart)
      const oldUtf16End = TextOffsetMapper.utf8ToUtf16(oldText, patch.replaceByteEndExclusive)
      // 新正文插入的范围（byte offset）→ 用 newSnapshot.text 转 UTF-16
      const newUtf16Start = TextOffsetMapper.utf8ToUtf16(newText, patch.insertedByteStart)
      const newUtf16End = TextOffsetMapper.utf8ToUtf16(newText, patch.insertedByteEndExclusive)

      // 旧正文字形源（吞字/Forward Delete 时需要）——用 oldLayout 计算几何
      // 跨行按视觉行分别生成 DeletedRun
      if (oldUtf16Start < oldUtf16End) {
        const oldRects = EditorMotionPlanner.computeGlyphRectsByLines(
          oldText, oldUtf16Start, oldUtf16End, oldLayout
        )
        for (const rect of oldRects) {
          // Issue #879 复核评论问题4：glyphIds 取该范围内所有字符簇身份列表，
          // windowId 由簇身份序列派生——不再仅用起始字符簇身份代表整窗口。
          const glyphIds = EditorMotionPlanner.resolveGlyphIds(
            oldIdentities, rect.utf16Start, rect.utf16End, oldText
          )
          const windowId = windowIdForGlyphIds(glyphIds)
          // Issue #879 复核评论6078682695 问题1：起始状态按字形算，返回 RunStartPiece[]。
          const pieces = deletedRunStartState(
            EditorMotionPlanner.runGeometry(glyphIds, oldText, rect.utf16Start, rect.utf16End, rect.rect, oldLayout),
            displayedContext,
            isForwardDelete ? lastVisibleFrame.caretX : rect.rect.x
          )
          const compatLeft = pieces.length > 0 ? pieces[0].startClipLeft : rect.rect.x
          const compatRight = pieces.length > 0 ? pieces[pieces.length - 1].startClipRight : rect.rect.x
          deletedRuns.push({
            utf16Start: rect.utf16Start,
            utf16End: rect.utf16End,
            sourceKind: 'old',
            rect: rect.rect,
            isForward: isForwardDelete,
            startClipLeft: compatLeft,
            startClipRight: compatRight,
            glyphIds: glyphIds,
            windowId: windowId,
            pieces: pieces
          })
        }
      }

      // 新正文字形目标（吐字时需要）——用 newLayout 计算几何
      // 跨行按视觉行分别生成 InsertRun
      if (newUtf16Start < newUtf16End) {
        const newRects = EditorMotionPlanner.computeGlyphRectsByLines(
          newText, newUtf16Start, newUtf16End, newLayout
        )
        for (const rect of newRects) {
          // Issue #879 复核评论问题4：glyphIds 取该范围内所有字符簇身份列表，
          // windowId 由簇身份序列派生。
          const glyphIds = EditorMotionPlanner.resolveGlyphIds(
            newIdentities, rect.utf16Start, rect.utf16End, newText
          )
          const windowId = windowIdForGlyphIds(glyphIds)
          // Issue #879 复核评论6078682695 问题1：吐字起点按字形算，返回 RunStartPiece[]。
          const pieces = insertRunStartState(
            EditorMotionPlanner.runGeometry(glyphIds, newText, rect.utf16Start, rect.utf16End, rect.rect, newLayout),
            displayedContext
          )
          const compatLeft = pieces.length > 0 ? pieces[0].startClipLeft : rect.rect.x
          const compatRight = pieces.length > 0 ? pieces[pieces.length - 1].startClipRight : rect.rect.x
          insertRuns.push({
            utf16Start: rect.utf16Start,
            utf16End: rect.utf16End,
            sourceKind: 'new',
            rect: rect.rect,
            startClipLeft: compatLeft,
            startClipRight: compatRight,
            glyphIds: glyphIds,
            windowId: windowId,
            pieces: pieces
          })
        }
      }
    }

    // 用 offsetMap 识别保留文字的 reflow（RetainedMove）
    if (editResult.offsetMap) {
      const moves = EditorMotionPlanner.identifyRetainedMoves(
        editResult.offsetMap, oldText, newText, oldLayout, newLayout
      )
      for (const move of moves) {
        // Issue #879 复核评论问题4：glyphIds 取该范围内所有字符簇身份列表，
        // windowId 由簇身份序列派生。
        const glyphIds = EditorMotionPlanner.resolveGlyphIds(
          oldIdentities, move.oldUtf16Start, move.oldUtf16End, oldText
        )
        const windowId = windowIdForGlyphIds(glyphIds)
        // Issue #879 复核评论6078682695 问题1：起点按字形交集校正，返回 RunStartPiece[]。
        const pieces = retainedMoveStartState(
          EditorMotionPlanner.runGeometry(
            glyphIds, oldText, move.oldUtf16Start, move.oldUtf16End, move.oldRect, oldLayout
          ),
          displayedContext,
          {
            startClipLeft: move.oldRect.x,
            startClipRight: move.oldRect.x + move.oldRect.width,
            startPositionX: move.oldRect.x,
            startPositionY: move.oldRect.y
          }
        )
        const firstPiece = pieces[0]
        retainedMoves.push({
          oldUtf16Start: move.oldUtf16Start,
          oldUtf16End: move.oldUtf16End,
          newUtf16Start: move.newUtf16Start,
          newUtf16End: move.newUtf16End,
          sourceKind: 'old',
          oldRect: move.oldRect,
          newRect: move.newRect,
          startPositionX: firstPiece.startPositionX,
          startPositionY: firstPiece.startPositionY,
          glyphIds: glyphIds,
          windowId: windowId,
          pieces: pieces
        })
      }
    }

    // 推断主要运动类型（供 needsMotionLayer 判断）
    const motionKind = EditorMotionPlanner.inferPrimaryMotionKind(insertRuns, deletedRuns, retainedMoves, editResult)

    return {
      insertRuns,
      deletedRuns,
      retainedMoves,
      caretPath: [],
      durationMs: 0, // 由 policy 决定实际时长
      motionKind,
      carryOverGhostRuns: [],
      carryOverMotionRuns: []
    }
  }

  /**
   * 判断是否为 Forward Delete。
   * Forward Delete 的特征：operationKind=Delete，且逻辑光标位置不变（newSelectionAnchor === oldSelectionAnchor）。
   */
  private static isForwardDelete(editResult: EditorEditResult): boolean {
    if (editResult.operationKind !== EditorOperationKind.Delete) {
      return false
    }
    // Forward Delete：光标位置不变，删除的是光标右侧的文字
    return editResult.newSelectionAnchor === editResult.oldSelectionAnchor &&
           editResult.newSelectionHead === editResult.oldSelectionHead
  }

  /**
   * 推断主要运动类型（供 needsMotionLayer 判断）。
   *
   * Issue #879 本轮复核评论修复（问题5）：
   * 不再用单一 motionKind 决定整个事务，但仍需一个主要类型供 needsMotionLayer 判断。
   * 优先级：有吐字 → Reveal；有吞字 → Conceal/ConcealForward；有保留字移动 → Reflow；否则 None。
   */
  private static inferPrimaryMotionKind(
    insertRuns: InsertRun[],
    deletedRuns: DeletedRun[],
    retainedMoves: RetainedMove[],
    editResult: EditorEditResult
  ): MotionKind {
    if (insertRuns.length > 0) {
      return MotionKind.Reveal
    }
    if (deletedRuns.length > 0) {
      // 检查是否全部为 Forward Delete
      const allForward = deletedRuns.every((r: DeletedRun): boolean => r.isForward)
      return allForward ? MotionKind.ConcealForward : MotionKind.Conceal
    }
    if (retainedMoves.length > 0) {
      return MotionKind.Reflow
    }
    return MotionKind.None
  }

  /**
   * Issue #879 复核评论问题4：解析一个字形范围内所有字符簇的跨修订稳定身份列表。
   *
   * 使用身份表的 idsForRange 方法获取范围内所有字符簇的 GlyphIdentityEntry，
   * 提取 glyphId 列表。身份表缺失时退回单个内容 hash 作为兜底。
   */
  private static resolveGlyphIds(
    identities: GlyphIdentityTable | null,
    utf16Start: number,
    utf16End: number,
    text: string
  ): string[] {
    if (identities !== null) {
      const entries = identities.idsForRange(utf16Start, utf16End)
      if (entries.length > 0) {
        return entries.map((e: GlyphIdentityEntry): string => e.glyphId)
      }
    }
    // 兜底：单个内容 hash
    return [EditorMotionPlanner.fallbackGlyphId(utf16Start, utf16End, text)]
  }

  /**
   * Issue #879 复核评论6075662695问题3：把冻结帧的运动窗口转成起始状态计算用的可见窗口。
   *
   * 只带出「可见性判定」需要的字段：字形身份列表 + 当前裁切区间 + 字形基准 x。
   * 不做匹配、不取窗口整体裁切范围——交给 editor_commit_start 按字形逐个判定。
   */
  private static frozenWindowSpans(
    frame: EditorVisualFrame,
    oldText: string,
    oldLayout: LineLayout[],
    newText: string,
    newLayout: LineLayout[],
    oldRevision: number,
    newRevision: number
  ): FrozenWindowSpan[] {
    const spans: FrozenWindowSpan[] = []
    if (!frame.isAnimating) {
      // 旧动画已结束：屏幕上没有运动窗口，displayed 正文全部静态可见。
      return spans
    }
    for (const w of frame.motionGlyphWindows) {
      // Issue #879 复核评论6078682695 问题2：按 w.sourceRevision 查源布局，
      // 不按 sourceKind 猜 oldText/newText——跨修订残影时 sourceKind==='old' 不等于本轮 oldText。
      let sourceText: string
      let sourceLayout: LineLayout[]
      if (w.sourceRevision === oldRevision) {
        sourceText = oldText
        sourceLayout = oldLayout
      } else if (w.sourceRevision === newRevision) {
        sourceText = newText
        sourceLayout = newLayout
      } else {
        // 跨修订残影：无法从本轮快照获取源布局，退回 sourceKind 猜测
        sourceText = w.sourceKind === 'old' ? oldText : newText
        sourceLayout = w.sourceKind === 'old' ? oldLayout : newLayout
      }

      // 用 clusterBoundaries 把窗口的 UTF-16 范围拆成字符簇边界，
      // 再与 glyphIds 一一对应，构建 glyphUtf16Ranges。
      const glyphUtf16Ranges = EditorMotionPlanner.buildGlyphUtf16Ranges(
        w.glyphIds, w.utf16Start, w.utf16End, sourceText
      )

      spans.push({
        glyphIds: w.glyphIds,
        clipLeft: w.currentClipLeft,
        clipRight: w.currentClipRight,
        // 吐字/吞字窗口 offsetX 为 0（clip 只表达可见范围）；
        // 保留字平移窗口 offsetX = 当前位置 - 布局位置（clip 即平移后的位置）。
        offsetX: w.offsetX,
        offsetY: w.offsetY,
        sourceRevision: w.sourceRevision,
        glyphUtf16Ranges: glyphUtf16Ranges,
        sourceLayout: sourceLayout
      })
    }
    return spans
  }

  /**
   * 为 FrozenWindowSpan 的 glyphUtf16Ranges 字段构建映射。
   * 用 clusterBoundaries 把窗口的 UTF-16 范围拆成字符簇边界，
   * 再与 glyphIds 一一对应：glyphIds[i] → [boundaries[i], boundaries[i+1]]。
   */
  private static buildGlyphUtf16Ranges(
    glyphIds: string[],
    utf16Start: number,
    utf16End: number,
    sourceText: string
  ): Map<string, [number, number]> | null {
    if (glyphIds.length === 0) {
      return null
    }
    const boundaries = clusterBoundaries(sourceText, utf16Start, utf16End)
    // 簇数 = boundaries.length - 1，应与 glyphIds.length 一致
    if (boundaries.length !== glyphIds.length + 1) {
      // 簇数与身份数对不上——返回 null 表示窗口不提供此映射
      return null
    }
    const ranges: Map<string, [number, number]> = new Map()
    for (let i = 0; i < glyphIds.length; i++) {
      ranges.set(glyphIds[i], [boundaries[i], boundaries[i + 1]])
    }
    return ranges
  }

  /**
   * Issue #879 复核评论6075662695问题3：组装一个 run 参与起始状态计算的几何信息。
   */
  private static runGeometry(
    glyphIds: string[],
    ownText: string,
    ownUtf16Start: number,
    ownUtf16End: number,
    ownRect: GlyphRect,
    ownLayout: LineLayout[]
  ): RunGeometry {
    return {
      glyphIds: glyphIds,
      ownText: ownText,
      ownUtf16Start: ownUtf16Start,
      ownUtf16End: ownUtf16End,
      ownRect: ownRect,
      ownLayout: ownLayout
    }
  }

  /**
   * 解析一个字形范围的跨修订稳定身份。
   *
   * Issue #879 最新复核评论问题3：
   * 优先使用身份表——它由每笔编辑逐笔搬运字符簇身份得到，
   * 位置平移、old/new 角色变化都不改身份，相同位置的不同字也不会互相误配。
   * 身份表缺失该位置时（例如表本身不可用）退回内容 hash：
   * 它只能保证「位置 + 内容」相同才算同一个字，不如身份表可靠，但不会错配到别的字。
   */
  private static resolveGlyphId(
    identities: GlyphIdentityTable | null,
    utf16Start: number,
    utf16End: number,
    text: string
  ): string {
    if (identities !== null) {
      const glyphId = identities.glyphIdAt(utf16Start)
      if (glyphId !== null) {
        return glyphId
      }
    }
    return EditorMotionPlanner.fallbackGlyphId(utf16Start, utf16End, text)
  }

  /**
   * 身份表不可用时的兜底身份（位置 + 内容 hash）。
   */
  private static fallbackGlyphId(
    utf16Start: number,
    utf16End: number,
    text: string
  ): string {
    const content = text.substring(utf16Start, utf16End)
    let hash = 0
    for (let i = 0; i < content.length; i++) {
      hash = ((hash << 5) - hash + content.charCodeAt(i)) | 0
    }
    return `${utf16Start}-${utf16End}-${hash}`
  }

  /**
   * 从行布局计算光标位置。
   */
  private static findCaretPosition(
    cursorUtf16: number,
    lineLayouts: LineLayout[],
    text: string
  ): CaretPosition {
    for (const line of lineLayouts) {
      if (cursorUtf16 >= line.startUtf16 && cursorUtf16 <= line.endUtf16) {
        // 在此行内：用 caretStops 找最近的 x
        let x = line.left
        for (const stop of line.caretStops) {
          if (stop.utf16Offset <= cursorUtf16) {
            x = stop.x
          }
        }
        return { x, y: line.y, height: line.height } as CaretPosition
      }
    }
    // 默认返回第一行
    if (lineLayouts.length > 0) {
      const firstLine = lineLayouts[0]
      return { x: firstLine.left, y: firstLine.y, height: firstLine.height } as CaretPosition
    }
    return { x: 0, y: 0, height: 0 } as CaretPosition
  }

  /**
   * 根据 y 坐标找到行索引。
   */
  private static findLineIndexByY(y: number, lineLayouts: LineLayout[]): number {
    for (let i = 0; i < lineLayouts.length; i++) {
      const line = lineLayouts[i]
      if (y >= line.y && y < line.y + line.height) {
        return i
      }
    }
    return -1
  }

  /**
   * 获取行末的 x 坐标（最后一个 caretStop 的 x）。
   */
  private static getLineEndX(line: LineLayout, text: string): number {
    if (line.caretStops.length === 0) {
      return 0
    }
    return line.caretStops[line.caretStops.length - 1].x - line.left
  }

  /**
   * 按视觉行分别计算字形矩形列表。
   *
   * Issue #879 本轮复核评论修复（问题5）：
   * 跨行时按视觉行分别计算裁切窗口——每个返回项对应一个视觉行内的连续字片段，
   * 不拿一个 x0→x1 横向跨行。
   *
   * @returns 每项包含 utf16Start/utf16End（该视觉行内的子区间）和 rect（该视觉行内的几何）
   */
  private static computeGlyphRectsByLines(
    text: string,
    utf16Start: number,
    utf16End: number,
    lineLayouts: LineLayout[]
  ): GlyphRectSegment[] {
    const result: GlyphRectSegment[] = []
    if (utf16Start >= utf16End) {
      return result
    }

    // 遍历行，找到与 [utf16Start, utf16End) 有交集的行
    for (const line of lineLayouts) {
      // 行与区间无交集
      if (line.endUtf16 <= utf16Start || line.startUtf16 >= utf16End) {
        continue
      }
      // 该行内的子区间
      const segStart = Math.max(utf16Start, line.startUtf16)
      const segEnd = Math.min(utf16End, line.endUtf16)
      if (segStart >= segEnd) {
        continue
      }

      // 用 caretStops 找起始 x 和结束 x
      let x = line.left
      for (const stop of line.caretStops) {
        if (stop.utf16Offset <= segStart) {
          x = stop.x
        }
      }
      let endX = x
      for (const stop of line.caretStops) {
        if (stop.utf16Offset <= segEnd) {
          endX = stop.x
        }
      }

      const rect: GlyphRect = {
        x,
        y: line.y,
        width: Math.max(0, endX - x),
        height: line.height
      }
      const segment: GlyphRectSegment = {
        utf16Start: segStart,
        utf16End: segEnd,
        rect: rect
      }
      result.push(segment)
    }
    return result
  }

  /**
   * 计算指定 UTF-16 子范围在给定行布局中的精确矩形。
   *
   * 与 computeGlyphRectsByLines 不同，本方法返回的是子范围对应的精确 rect，
   * 不是整行片段的 rect。用于双指针配对时获取每个子片段的精确几何。
   *
   * caretStops 的 utf16Offset 是全局 offset，x 是绝对 x 坐标（含 line.left）。
   */
  private static computeGlyphRectForRange(
    text: string,
    utf16Start: number,
    utf16End: number,
    layout: LineLayout[]
  ): GlyphRect {
    // 找到包含 utf16Start 的行
    let lineIndex = -1
    for (let i = 0; i < layout.length; i++) {
      if (utf16Start >= layout[i].startUtf16 && utf16Start < layout[i].endUtf16) {
        lineIndex = i
        break
      }
    }
    // utf16Start 可能恰好在最后一行的末尾
    if (lineIndex === -1 && layout.length > 0) {
      const lastLine = layout[layout.length - 1]
      if (utf16Start === lastLine.endUtf16) {
        lineIndex = layout.length - 1
      }
    }
    if (lineIndex === -1) {
      return { x: 0, y: 0, width: 0, height: 0 }
    }
    const line = layout[lineIndex]

    // 没有 caretStops——fallback 用行左边界，width 为 0
    if (line.caretStops.length === 0) {
      return {
        x: line.left,
        y: line.y,
        width: 0,
        height: line.height
      }
    }

    // 在该行内，用 caretStops 计算子范围的精确 x 和 width
    // caretStops 的 utf16Offset 是全局 offset，x 是绝对 x 坐标
    const effectiveEnd = Math.min(utf16End, line.endUtf16)

    let x = line.left
    for (const stop of line.caretStops) {
      if (stop.utf16Offset <= utf16Start) {
        x = stop.x
      }
    }

    let xEnd = x
    for (const stop of line.caretStops) {
      if (stop.utf16Offset <= effectiveEnd) {
        xEnd = stop.x
      }
    }

    const width = Math.max(0, xEnd - x)

    return {
      x: x,
      y: line.y,
      width: width,
      height: line.height
    }
  }

  /**
   * 用 offsetMap 识别保留文字的 reflow，生成 RetainedMove 列表。
   *
   * offsetMap 记录旧正文 → 新正文的字符身份映射。
   * Identity 条目表示字符位置不变（无 reflow）；
   * Shifted 条目表示字符位置发生了偏移（需要 reflow 动画）。
   *
   * Issue #879 本轮复核评论修复（问题5）：
   * - 用 oldLayout 算 oldRect，用 newLayout 算 newRect，做真正的平移动画。
   * - 旧实现"直接在新位置画，不做平移动画"——输入导致后续字换行时，
   *   后续字不会从旧行移到新行。现在用 RetainedMove 记录 oldRect → newRect。
   * - 跨行按视觉行分别生成 RetainedMove。
   *
   * Issue #879 复核评论修复：
   * - 旧正文范围用 oldText 解码，旧字形几何用 oldLayout 计算
   * - 新正文范围用 newText 解码，新字形几何用 newLayout 计算
   */
  private static identifyRetainedMoves(
    offsetMap: OffsetMap,
    oldText: string,
    newText: string,
    oldLayout: LineLayout[],
    newLayout: LineLayout[]
  ): RetainedMoveSpan[] {
    const moves: RetainedMoveSpan[] = []
    const entries: OffsetMapEntry[] = offsetMap.entries

    for (const entry of entries) {
      if (entry.kind !== OffsetMapKind.Shifted) {
        continue
      }
      // Shifted 条目：旧正文 [oldByteOffset, oldByteOffset+length) 的字符
      // 在新正文中移到了 [newByteOffset, newByteOffset+length)
      const oldUtf16Start = TextOffsetMapper.utf8ToUtf16(oldText, entry.oldByteOffset)
      const oldUtf16End = TextOffsetMapper.utf8ToUtf16(oldText, entry.oldByteOffset + entry.length)
      const newUtf16Start = TextOffsetMapper.utf8ToUtf16(newText, entry.newByteOffset)
      const newUtf16End = TextOffsetMapper.utf8ToUtf16(newText, entry.newByteOffset + entry.length)

      // 跨行按视觉行分别生成 RetainedMove
      // 旧字源按 oldLayout 算 oldRect，新位置按 newLayout 算 newRect
      const oldRects = EditorMotionPlanner.computeGlyphRectsByLines(
        oldText, oldUtf16Start, oldUtf16End, oldLayout
      )
      const newRects = EditorMotionPlanner.computeGlyphRectsByLines(
        newText, newUtf16Start, newUtf16End, newLayout
      )

      // 不再用 UTF-16 offset 是否改变判断视觉是否发生位移——
      // offset 未变但物理换行位置可能已变（例如前面插入了一个字导致后面文字换行位置变化）。
      // 改为比较 oldRect 与 newRect 的 (x,y) 是否真的不同。
      // 不能仅比较第一个片段——首段没动、后段换行时会漏动画。
      // 只有当所有片段对的位置都相同时才跳过。
      if (oldRects.length > 0 && newRects.length > 0) {
        let hasDisplacement = false
        const minLen = Math.min(oldRects.length, newRects.length)
        for (let i = 0; i < minLen; i++) {
          if (oldRects[i].rect.x !== newRects[i].rect.x ||
              oldRects[i].rect.y !== newRects[i].rect.y) {
            hasDisplacement = true
            break
          }
        }
        // 片段数不同也说明有位移（换行/合行）
        if (!hasDisplacement && oldRects.length !== newRects.length) {
          hasDisplacement = true
        }
        if (!hasDisplacement) {
          continue
        }
      }

      // 双指针配对：按字符顺序把 old 片段和 new 片段配对。
      // oldRects 和 newRects 覆盖相同数量的 UTF-16 字符（同一 Shifted entry），
      // 但可能被拆成不同数量的行片段（如旧版一行、新版两行）。
      // 用双指针确保每个字符都有对应的 old 和 new 片段，不丢弃任何片段。
      let oldIdx = 0
      let newIdx = 0
      let oldCharConsumed = 0  // oldRects[oldIdx] 中已配对的字符数
      let newCharConsumed = 0  // newRects[newIdx] 中已配对的字符数

      while (oldIdx < oldRects.length && newIdx < newRects.length) {
        const oldSeg = oldRects[oldIdx]
        const newSeg = newRects[newIdx]
        const oldSegLen = oldSeg.utf16End - oldSeg.utf16Start
        const newSegLen = newSeg.utf16End - newSeg.utf16Start
        const oldRemaining = oldSegLen - oldCharConsumed
        const newRemaining = newSegLen - newCharConsumed

        // 配对较小的剩余字符数，确保不遗漏任何片段
        const pairLen = Math.min(oldRemaining, newRemaining)

        // 为配对的子范围重新计算精确 rect，不用整行片段 rect。
        // 旧版一行 5 字新版拆成 2+3 字时，双指针会产生两个 RetainedMove，
        // 如果直接用 oldSeg.rect（5 字宽的整行 rect），二者会引用同一个宽 rect，
        // 导致动画把同一行字复制到两个位置。改为对每个子范围取精确几何。
        const oldSubStart = oldSeg.utf16Start + oldCharConsumed
        const oldSubEnd = oldSeg.utf16Start + oldCharConsumed + pairLen
        const newSubStart = newSeg.utf16Start + newCharConsumed
        const newSubEnd = newSeg.utf16Start + newCharConsumed + pairLen

        const oldRect = EditorMotionPlanner.computeGlyphRectForRange(
          oldText, oldSubStart, oldSubEnd, oldLayout
        )
        const newRect = EditorMotionPlanner.computeGlyphRectForRange(
          newText, newSubStart, newSubEnd, newLayout
        )

        moves.push({
          oldUtf16Start: oldSubStart,
          oldUtf16End: oldSubEnd,
          newUtf16Start: newSubStart,
          newUtf16End: newSubEnd,
          sourceKind: 'old',
          oldRect: oldRect,
          newRect: newRect,
          startPositionX: oldRect.x,
          startPositionY: oldRect.y
        })

        oldCharConsumed += pairLen
        newCharConsumed += pairLen

        if (oldCharConsumed >= oldSegLen) {
          oldIdx++
          oldCharConsumed = 0
        }
        if (newCharConsumed >= newSegLen) {
          newIdx++
          newCharConsumed = 0
        }
      }
    }

    return moves
  }
}
