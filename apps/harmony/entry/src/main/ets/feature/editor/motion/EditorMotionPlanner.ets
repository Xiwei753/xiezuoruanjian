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
// 不修改 Core 的编辑事实模型，只读 editResult 做规划。
// 所有 byte offset 通过 TextOffsetMapper 转 UTF-16 offset 供渲染层使用。

import { EditorEditResult, DisplayPatch, EditorOperationKind, EditorTransactionCause, OffsetMap, OffsetMapEntry, OffsetMapKind } from '../../../corebridge/dto/EditorDtos'
import { EditorVisualFrame, MotionKind, MotionSegment, MotionGlyphWindow, StaticVisibleRange } from './EditorVisualFrame'
import { EditorLayoutSnapshot } from '../render/EditorLayoutSnapshot'
import { TextOffsetMapper } from '../input/TextOffsetMapper'
import { LineLayout } from '../render/editor_render_geometry'

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
 * 字形源——描述一个字形的排版位置和来源。
 *
 * - sourceKind='old'：字形来自旧正文（base revision），用于吞字动画。
 * - sourceKind='new'：字形来自新正文（new revision），用于吐字动画和 reflow。
 */
export interface GlyphSource {
  /** UTF-16 起始 offset */
  utf16Start: number
  /** UTF-16 结束 offset（exclusive） */
  utf16End: number
  /** 排版位置 x（vp） */
  x: number
  /** 排版位置 y（vp） */
  y: number
  /** 字形宽度（vp） */
  width: number
  /** 字形高度（vp） */
  height: number
  /** 来源：'old' 或 'new' */
  sourceKind: 'old' | 'new'
}

/**
 * 运动规划结果。
 *
 * 描述一次编辑事务应该产生的文字运动：
 * - motionKind：运动类型（吐字/吞字/Forward Delete/Reflow/CaretMove/None）。
 * - segments：跨行光标移动的分段轨迹。
 * - affectedRanges：受影响的正文范围（UTF-16 offset）。
 * - oldGlyphSources：旧正文中需要运动层绘制的字形源列表。
 * - newGlyphTargets：新正文中需要运动层绘制的字形目标列表。
 * - durationMs：动画时长。
 */
export interface EditorMotionPlan {
  /** 运动类型 */
  motionKind: MotionKind
  /** 跨行光标移动的分段轨迹 */
  segments: MotionSegment[]
  /** 受影响的正文范围（UTF-16 offset 对） */
  affectedRanges: AffectedRange[]
  /** 旧正文字形源列表 */
  oldGlyphSources: GlyphSource[]
  /** 新正文字形目标列表 */
  newGlyphTargets: GlyphSource[]
  /** 动画时长（ms） */
  durationMs: number
}

/**
 * 运动规划器。
 *
 * 从 oldSnapshot + oldLayout + newSnapshot + newLayout + editResult 出发，
 * 规划文字运动方案。核心是解读 displayPatches 和 offsetMap，
 * 判断哪些文字需要吐字/吞字动画，哪些文字需要 reflow 平移。
 *
 * Issue #879 复核评论修复：
 * - 旧正文范围（replaceByteStart/End）用 oldSnapshot.text 解码
 * - 新正文范围（insertedByteStart/End）用 newSnapshot.text 解码
 * - 旧字形几何用 oldLayout 计算，新字形几何用 newLayout 计算
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
   * @returns 运动规划结果
   */
  static plan(
    oldSnapshot: EditorLayoutSnapshot,
    oldLayout: LineLayout[],
    newSnapshot: EditorLayoutSnapshot,
    newLayout: LineLayout[],
    editResult: EditorEditResult | null,
    lastVisibleFrame: EditorVisualFrame
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
    return EditorMotionPlanner.planFromPatches(oldSnapshot, oldLayout, newSnapshot, newLayout, editResult, lastVisibleFrame)
  }

  /**
   * 空规划——无运动。
   */
  private static emptyPlan(): EditorMotionPlan {
    return {
      motionKind: MotionKind.None,
      segments: [],
      affectedRanges: [],
      oldGlyphSources: [],
      newGlyphTargets: [],
      durationMs: 0
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

    const segments: MotionSegment[] = []

    if (!isCrossLine) {
      // 同行移动：单段直线
      segments.push({
        startCaretX: oldCaretX,
        startCaretY: oldCaretY,
        endCaretX: newCaret.x,
        endCaretY: newCaret.y
      })
    } else {
      // 跨行移动：按实际停靠点分段
      // 1. 先在旧行水平移动到行末（或行首，取决于方向）
      const oldLine = oldLayout[oldLineIndex]
      const newLine = newLayout[newLineIndex]

      // 判断移动方向：向下移动还是向上移动
      const isMoveDown = newLineIndex > oldLineIndex

      if (isMoveDown) {
        // 向下移动：先在旧行水平移到行末，再垂直跳到新行，再水平移到目标
        // 段1：旧行内水平移动到行末
        segments.push({
          startCaretX: oldCaretX,
          startCaretY: oldCaretY,
          endCaretX: oldLine.left + EditorMotionPlanner.getLineEndX(oldLine, oldSnapshot.text),
          endCaretY: oldCaretY
        })
        // 段2：垂直跳转到新行首
        segments.push({
          startCaretX: oldLine.left + EditorMotionPlanner.getLineEndX(oldLine, oldSnapshot.text),
          startCaretY: oldCaretY,
          endCaretX: newLine.left,
          endCaretY: newCaret.y
        })
        // 段3：新行内水平移动到目标
        segments.push({
          startCaretX: newLine.left,
          startCaretY: newCaret.y,
          endCaretX: newCaret.x,
          endCaretY: newCaret.y
        })
      } else {
        // 向上移动：先在旧行水平移到行首，再垂直跳到新行末，再水平移到目标
        // 段1：旧行内水平移动到行首
        segments.push({
          startCaretX: oldCaretX,
          startCaretY: oldCaretY,
          endCaretX: oldLine.left,
          endCaretY: oldCaretY
        })
        // 段2：垂直跳转到新行末
        segments.push({
          startCaretX: oldLine.left,
          startCaretY: oldCaretY,
          endCaretX: newLine.left + EditorMotionPlanner.getLineEndX(newLine, newSnapshot.text),
          endCaretY: newCaret.y
        })
        // 段3：新行内水平移动到目标
        segments.push({
          startCaretX: newLine.left + EditorMotionPlanner.getLineEndX(newLine, newSnapshot.text),
          startCaretY: newCaret.y,
          endCaretX: newCaret.x,
          endCaretY: newCaret.y
        })
      }
    }

    return {
      motionKind: MotionKind.CaretMove,
      segments,
      affectedRanges: [],
      oldGlyphSources: [],
      newGlyphTargets: [],
      durationMs: 0 // 由 policy 决定实际时长
    }
  }

  /**
   * 从 displayPatches 分析正文增删，规划运动方案。
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
    lastVisibleFrame: EditorVisualFrame
  ): EditorMotionPlan {
    const patches: DisplayPatch[] = editResult.displayPatches
    const oldText = oldSnapshot.text
    const newText = newSnapshot.text

    // 无 patches → 无运动
    if (patches.length === 0) {
      return EditorMotionPlanner.emptyPlan()
    }

    // 判断运动类型
    const isForwardDelete: boolean = EditorMotionPlanner.isForwardDelete(editResult)
    const isInsert: boolean = EditorMotionPlanner.isInsert(editResult)
    const isDelete: boolean = EditorMotionPlanner.isDelete(editResult)

    let motionKind: MotionKind = MotionKind.None
    if (isForwardDelete) {
      motionKind = MotionKind.ConcealForward
    } else if (isInsert) {
      motionKind = MotionKind.Reveal
    } else if (isDelete) {
      motionKind = MotionKind.Conceal
    } else {
      // Replace 或其他类型 → 按主要操作判断
      motionKind = EditorMotionPlanner.inferMotionKind(editResult)
    }

    // 收集受影响范围和字形源/目标
    const affectedRanges: AffectedRange[] = []
    const oldGlyphSources: GlyphSource[] = []
    const newGlyphTargets: GlyphSource[] = []

    for (const patch of patches) {
      // 旧正文被替换的范围（byte offset）→ 用 oldSnapshot.text 转 UTF-16
      const oldUtf16Start = TextOffsetMapper.utf8ToUtf16(oldText, patch.replaceByteStart)
      const oldUtf16End = TextOffsetMapper.utf8ToUtf16(oldText, patch.replaceByteEndExclusive)
      // 新正文插入的范围（byte offset）→ 用 newSnapshot.text 转 UTF-16
      const newUtf16Start = TextOffsetMapper.utf8ToUtf16(newText, patch.insertedByteStart)
      const newUtf16End = TextOffsetMapper.utf8ToUtf16(newText, patch.insertedByteEndExclusive)

      affectedRanges.push({ utf16Start: oldUtf16Start, utf16End: oldUtf16End } as AffectedRange)

      // 旧正文字形源（吞字/Forward Delete 时需要）——用 oldLayout 计算几何
      if (oldUtf16Start < oldUtf16End) {
        const glyphSource = EditorMotionPlanner.computeGlyphSource(
          oldText, oldUtf16Start, oldUtf16End, oldLayout, 'old'
        )
        if (glyphSource !== null) {
          oldGlyphSources.push(glyphSource)
        }
      }

      // 新正文字形目标（吐字/Reflow 时需要）——用 newLayout 计算几何
      if (newUtf16Start < newUtf16End) {
        const glyphTarget = EditorMotionPlanner.computeGlyphSource(
          newText, newUtf16Start, newUtf16End, newLayout, 'new'
        )
        if (glyphTarget !== null) {
          newGlyphTargets.push(glyphTarget)
        }
      }
    }

    // 用 offsetMap 识别保留文字的 reflow
    if (editResult.offsetMap) {
      const reflowRanges = EditorMotionPlanner.identifyReflowRanges(
        editResult.offsetMap, oldText, newText, oldLayout, newLayout
      )
      for (const reflow of reflowRanges) {
        newGlyphTargets.push(reflow)
      }
    }

    return {
      motionKind,
      segments: [],
      affectedRanges,
      oldGlyphSources,
      newGlyphTargets,
      durationMs: 0 // 由 policy 决定实际时长
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
   * 判断是否为插入操作。
   */
  private static isInsert(editResult: EditorEditResult): boolean {
    return editResult.operationKind === EditorOperationKind.Insert
  }

  /**
   * 判断是否为删除操作（Backspace）。
   */
  private static isDelete(editResult: EditorEditResult): boolean {
    return editResult.operationKind === EditorOperationKind.Delete && !EditorMotionPlanner.isForwardDelete(editResult)
  }

  /**
   * 推断运动类型（用于 Replace 等复合操作）。
   */
  private static inferMotionKind(editResult: EditorEditResult): MotionKind {
    const delta = editResult.contentDelta
    if (delta.insertedChars > 0 && delta.deletedChars === 0) {
      return MotionKind.Reveal
    }
    if (delta.deletedChars > 0 && delta.insertedChars === 0) {
      return MotionKind.Conceal
    }
    if (delta.insertedChars > 0 && delta.deletedChars > 0) {
      // 替换：先吞后吐，按主要操作判断
      return delta.insertedChars >= delta.deletedChars ? MotionKind.Reveal : MotionKind.Conceal
    }
    return MotionKind.None
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
   * 计算字形源的排版位置。
   */
  private static computeGlyphSource(
    text: string,
    utf16Start: number,
    utf16End: number,
    lineLayouts: LineLayout[],
    sourceKind: 'old' | 'new'
  ): GlyphSource | null {
    if (utf16Start >= utf16End) {
      return null
    }
    // 找到字形所在的行
    for (const line of lineLayouts) {
      if (utf16Start >= line.startUtf16 && utf16Start < line.endUtf16) {
        // 用 caretStops 找起始 x
        let x = line.left
        for (const stop of line.caretStops) {
          if (stop.utf16Offset <= utf16Start) {
            x = stop.x
          }
        }
        // 找结束 x（字形宽度 = 结束 x - 起始 x）
        let endX = x
        for (const stop of line.caretStops) {
          if (stop.utf16Offset <= utf16End) {
            endX = stop.x
          }
        }
        return {
          utf16Start,
          utf16End,
          x,
          y: line.y,
          width: Math.max(0, endX - x),
          height: line.height,
          sourceKind
        }
      }
    }
    return null
  }

  /**
   * 用 offsetMap 识别保留文字的 reflow 范围。
   *
   * offsetMap 记录旧正文 → 新正文的字符身份映射。
   * Identity 条目表示字符位置不变（无 reflow）；
   * Shifted 条目表示字符位置发生了偏移（需要 reflow 动画）。
   *
   * Issue #879 复核评论修复：
   * - 旧正文范围用 oldText 解码，旧字形几何用 oldLayout 计算
   * - 新正文范围用 newText 解码，新字形几何用 newLayout 计算
   */
  private static identifyReflowRanges(
    offsetMap: OffsetMap,
    oldText: string,
    newText: string,
    oldLayout: LineLayout[],
    newLayout: LineLayout[]
  ): GlyphSource[] {
    const reflowTargets: GlyphSource[] = []
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

      // 只对有实际位移的字符生成 reflow 目标
      if (oldUtf16Start === newUtf16Start) {
        continue
      }

      // reflow 目标用新字形几何（newLayout），因为文字最终要在新位置显示
      const glyphTarget = EditorMotionPlanner.computeGlyphSource(
        newText, newUtf16Start, newUtf16End, newLayout, 'new'
      )
      if (glyphTarget !== null) {
        reflowTargets.push(glyphTarget)
      }
    }

    return reflowTargets
  }
}
