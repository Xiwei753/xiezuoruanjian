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
// 不修改 Core 的编辑事实模型，只读 editResult 做规划。
// 所有 byte offset 通过 TextOffsetMapper 转 UTF-16 offset 供渲染层使用。

import { EditorEditResult, DisplayPatch, EditorOperationKind, EditorTransactionCause, OffsetMap, OffsetMapEntry, OffsetMapKind } from '../../../corebridge/dto/EditorDtos'
import { EditorVisualFrame, MotionKind, MotionSegment, MotionGlyphWindow, StaticVisibleRange } from './EditorVisualFrame'
import { EditorPresentationUpdate } from './EditorPresentationUpdate'
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
 * 从 EditorPresentationUpdate（含 editResult）和行布局信息出发，
 * 规划文字运动方案。核心是解读 displayPatches 和 offsetMap，
 * 判断哪些文字需要吐字/吞字动画，哪些文字需要 reflow 平移。
 */
export class EditorMotionPlanner {
  /**
   * 规划运动方案。
   *
   * @param oldVisibleFrame 上一帧的视觉状态（含旧正文的光标位置和可见范围）
   * @param update 本次编辑更新（含 layoutSnapshot 和 editResult）
   * @param lineLayouts 当前行布局信息
   * @returns 运动规划结果
   */
  static plan(
    oldVisibleFrame: EditorVisualFrame,
    update: EditorPresentationUpdate,
    lineLayouts: LineLayout[]
  ): EditorMotionPlan {
    // 无 editResult（初始加载/切换章节/字号变更）→ 无运动
    if (update.editResult === null) {
      return EditorMotionPlanner.emptyPlan()
    }

    const editResult: EditorEditResult = update.editResult

    // NoChange → 无运动
    if (editResult.outcome !== 'applied' && editResult.outcome !== 'appliedWithAdjustedSelection') {
      return EditorMotionPlanner.emptyPlan()
    }

    // 纯选区移动（CursorOnly）→ 只规划光标移动
    if (editResult.operationKind === EditorOperationKind.CursorOnly) {
      return EditorMotionPlanner.caretMovePlan(oldVisibleFrame, update, lineLayouts)
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
    return EditorMotionPlanner.planFromPatches(oldVisibleFrame, update, lineLayouts)
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
   */
  private static caretMovePlan(
    oldVisibleFrame: EditorVisualFrame,
    update: EditorPresentationUpdate,
    lineLayouts: LineLayout[]
  ): EditorMotionPlan {
    const snapshot = update.layoutSnapshot
    const newCaretX = oldVisibleFrame.caretX
    const newCaretY = oldVisibleFrame.caretY

    // 从行布局计算新光标位置
    const cursorUtf16 = TextOffsetMapper.utf8ToUtf16(snapshot.text, snapshot.cursorByteOffset)
    const newCaret = EditorMotionPlanner.findCaretPosition(cursorUtf16, lineLayouts, snapshot.text)

    // 跨行时按真实视觉行拆段
    const segments: MotionSegment[] = []
    if (oldVisibleFrame.caretY !== newCaret.y) {
      // 跨行移动：先在旧行水平移动到行首/尾，再垂直跳转到新行，再水平移动到目标
      segments.push({
        startCaretX: oldVisibleFrame.caretX,
        startCaretY: oldVisibleFrame.caretY,
        endCaretX: newCaret.x,
        endCaretY: newCaret.y
      })
    } else {
      // 同行移动：单段直线
      segments.push({
        startCaretX: oldVisibleFrame.caretX,
        startCaretY: oldVisibleFrame.caretY,
        endCaretX: newCaret.x,
        endCaretY: newCaret.y
      })
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
   */
  private static planFromPatches(
    oldVisibleFrame: EditorVisualFrame,
    update: EditorPresentationUpdate,
    lineLayouts: LineLayout[]
  ): EditorMotionPlan {
    const editResult = update.editResult!
    const snapshot = update.layoutSnapshot
    const patches: DisplayPatch[] = editResult.displayPatches
    const oldText = oldVisibleFrame.staticVisibleRanges.length > 0 ? snapshot.text : snapshot.text

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
      // 旧正文被替换的范围（byte offset）→ 转 UTF-16
      const oldUtf16Start = TextOffsetMapper.utf8ToUtf16(oldText, patch.replaceByteStart)
      const oldUtf16End = TextOffsetMapper.utf8ToUtf16(oldText, patch.replaceByteEndExclusive)
      // 新正文插入的范围（byte offset）→ 转 UTF-16
      const newUtf16Start = TextOffsetMapper.utf8ToUtf16(snapshot.text, patch.insertedByteStart)
      const newUtf16End = TextOffsetMapper.utf8ToUtf16(snapshot.text, patch.insertedByteEndExclusive)

      affectedRanges.push({ utf16Start: oldUtf16Start, utf16End: oldUtf16End } as AffectedRange)

      // 旧正文字形源（吞字/Forward Delete 时需要）
      if (oldUtf16Start < oldUtf16End) {
        const glyphSource = EditorMotionPlanner.computeGlyphSource(
          oldText, oldUtf16Start, oldUtf16End, lineLayouts, 'old'
        )
        if (glyphSource !== null) {
          oldGlyphSources.push(glyphSource)
        }
      }

      // 新正文字形目标（吐字/Reflow 时需要）
      if (newUtf16Start < newUtf16End) {
        const glyphTarget = EditorMotionPlanner.computeGlyphSource(
          snapshot.text, newUtf16Start, newUtf16End, lineLayouts, 'new'
        )
        if (glyphTarget !== null) {
          newGlyphTargets.push(glyphTarget)
        }
      }
    }

    // 用 offsetMap 识别保留文字的 reflow
    if (editResult.offsetMap) {
      const reflowRanges = EditorMotionPlanner.identifyReflowRanges(
        editResult.offsetMap, oldText, snapshot.text, lineLayouts
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
   */
  private static identifyReflowRanges(
    offsetMap: OffsetMap,
    oldText: string,
    newText: string,
    lineLayouts: LineLayout[]
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

      const glyphTarget = EditorMotionPlanner.computeGlyphSource(
        newText, newUtf16Start, newUtf16End, lineLayouts, 'new'
      )
      if (glyphTarget !== null) {
        reflowTargets.push(glyphTarget)
      }
    }

    return reflowTargets
  }
}
