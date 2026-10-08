// EditorMotionCoordinator — 运动协调器。
//
// Issue #879：保存上一帧真正已显示的状态，retarget 到最新 canonical。
//
// 核心职责：
// 1. 接收 EditorPresentationUpdate，保存当前可见画面。
// 2. 用 EditorMotionPlanner 生成新 plan。
// 3. 直接 retarget——不等旧动画完成，立即切换到新 plan 的起始状态。
// 4. sampleFrame(progress) 从 plan 和 progress 计算视觉帧：
//    - 吐字：front = x0 + (x1-x0) * easing(p)
//    - 吞字：front = x1 + (x0-x1) * easing(p)
//    - 光标画在 front
// 5. finishMotion() 动画完成，归还静态层。
//
// 渲染身份：revision + generation + compositionSessionId + compositionGeneration + fontSize + lineSpacing + contentWidth。
// 渲染身份变化时说明布局重建（字号变更/切换章节等），直接显示静态画面。
//
// easing 函数：p <= 0 ? 0 : p >= 1 ? 1 : 1 - Math.pow(1 - p, 3)

import { EditorVisualFrame, MotionKind, MotionGlyphWindow, StaticVisibleRange } from './EditorVisualFrame'
import { EditorPresentationUpdate } from './EditorPresentationUpdate'
import { EditorMotionPolicy } from './EditorMotionPolicy'
import { EditorMotionPlan, EditorMotionPlanner, GlyphSource } from './EditorMotionPlanner'
import { EditorLayoutSnapshot } from '../render/EditorLayoutSnapshot'
import { LineLayout } from '../render/editor_render_geometry'
import { TextOffsetMapper } from '../input/TextOffsetMapper'

/**
 * 渲染身份——标识一次布局重建。
 *
 * 当 revision + generation + compositionSessionId + compositionGeneration + fontSize + lineSpacing + contentWidth
 * 中任一字段变化时，说明布局重建（字号变更/切换章节/内容宽度变化等），
 * 运动协调器应直接显示静态画面，不启动动画。
 */
export interface LayoutIdentity {
  readonly revision: number
  readonly generation: number
  readonly compositionSessionId: number
  readonly compositionGeneration: number
  readonly fontSize: number
  readonly lineSpacing: number
  readonly contentWidth: number
}

/**
 * 运动协调器。
 *
 * 保存上一帧真正已显示的状态，接收新的编辑更新后 retarget 到最新 canonical。
 * 不等旧动画完成——如果用户快速连续输入，直接切换到新 plan 的起始状态。
 */
export class EditorMotionCoordinator {
  /** 当前视觉帧 */
  private currentFrame: EditorVisualFrame
  /** 当前 revision */
  private currentRevision: number
  /** 动画开始时间（ms，performance.now 语义） */
  private motionStartTime: number
  /** 动画时长（ms） */
  private motionDurationMs: number
  /** 当前运动规划 */
  private currentPlan: EditorMotionPlan
  /** 当前渲染身份 */
  private layoutIdentity: LayoutIdentity

  constructor() {
    this.currentFrame = {
      caretX: 0,
      caretY: 0,
      caretHeight: 0,
      staticVisibleRanges: [],
      motionGlyphWindows: [],
      isAnimating: false,
      motionKind: MotionKind.None
    }
    this.currentRevision = 0
    this.motionStartTime = 0
    this.motionDurationMs = 0
    this.currentPlan = {
      motionKind: MotionKind.None,
      segments: [],
      affectedRanges: [],
      oldGlyphSources: [],
      newGlyphTargets: [],
      durationMs: 0
    }
    this.layoutIdentity = {
      revision: 0,
      generation: 0,
      compositionSessionId: 0,
      compositionGeneration: -1,
      fontSize: 16,
      lineSpacing: 1.8,
      contentWidth: 0
    }
  }

  /**
   * 接收编辑更新，生成新 plan，retarget 到最新 canonical。
   *
   * 先保存当前可见画面（作为新动画的起始状态），
   * 用 Planner 生成新 plan，直接 retarget——不等旧动画完成。
   *
   * @param update 本次编辑更新
   * @param lineLayouts 当前行布局
   * @param policy 运动策略
   * @returns 新的视觉帧（动画起始帧）
   */
  acceptUpdate(
    update: EditorPresentationUpdate,
    lineLayouts: LineLayout[],
    policy: EditorMotionPolicy
  ): EditorVisualFrame {
    const snapshot = update.layoutSnapshot

    // 计算新渲染身份
    const newIdentity: LayoutIdentity = {
      revision: snapshot.revision,
      generation: snapshot.generation,
      compositionSessionId: snapshot.compositionSessionId,
      compositionGeneration: snapshot.compositionGeneration,
      fontSize: snapshot.fontSize,
      lineSpacing: snapshot.lineSpacingMultiplier,
      contentWidth: 0 // 由调用方在 lineLayouts 推断或外部传入
    }

    // 渲染身份变化（字号变更/切换章节等）→ 直接显示静态画面
    if (!EditorMotionCoordinator.identityEquals(this.layoutIdentity, newIdentity)) {
      this.layoutIdentity = newIdentity
      this.currentPlan = EditorMotionPlanner.plan(this.currentFrame, update, lineLayouts)
      this.currentFrame = EditorMotionCoordinator.computeStaticFrame(snapshot, lineLayouts)
      this.currentRevision = snapshot.revision
      this.motionDurationMs = 0
      this.currentFrame.isAnimating = false
      return this.currentFrame
    }

    // 保存当前可见画面作为起始状态
    const startFrame = this.currentFrame

    // 用 Planner 生成新 plan
    this.currentPlan = EditorMotionPlanner.plan(startFrame, update, lineLayouts)

    // 确定动画时长
    if (this.currentPlan.motionKind === MotionKind.None ||
        this.currentPlan.motionKind === MotionKind.CaretMove) {
      // 纯光标移动用 cursorDurationMs
      this.motionDurationMs = policy.shouldAnimateCaret() ? policy.cursorDurationMs : 0
    } else {
      // 正文增删用 typingDurationMs
      this.motionDurationMs = policy.shouldAnimateText() ? policy.typingDurationMs : 0
    }

    // 设置 plan 的 durationMs
    this.currentPlan.durationMs = this.motionDurationMs

    // 记录动画开始时间
    this.motionStartTime = 0 // 由 sampleFrame 的 progress 参数驱动，不依赖绝对时间

    // 更新 revision
    this.currentRevision = snapshot.revision
    this.layoutIdentity = newIdentity

    // 如果不需要动画（时长为 0 或 reduceMotion），直接返回终态帧
    if (this.motionDurationMs <= 0) {
      this.currentFrame = EditorMotionCoordinator.computeStaticFrame(snapshot, lineLayouts)
      this.currentFrame.isAnimating = false
      return this.currentFrame
    }

    // 生成动画起始帧（progress=0）
    this.currentFrame = this.sampleFrame(0)
    return this.currentFrame
  }

  /**
   * 从 plan 和 progress 计算视觉帧。
   *
   * - 吐字（Reveal）：front = x0 + (x1-x0) * easing(p)，光标画在 front
   * - 吞字（Conceal）：front = x1 + (x0-x1) * easing(p)，光标画在 front
   * - Forward Delete（ConcealForward）：逻辑光标不动，被删字贴向固定 caret 缩回
   * - Reflow：保留文字从旧位置平移到新位置
   * - CaretMove：光标从旧位置插值到新位置
   *
   * @param progress 动画进度 [0, 1]
   * @returns 当前进度的视觉帧
   */
  sampleFrame(progress: number): EditorVisualFrame {
    const plan = this.currentPlan
    const p = EditorMotionCoordinator.easing(progress)

    if (plan.motionKind === MotionKind.None) {
      return this.currentFrame
    }

    if (plan.motionKind === MotionKind.CaretMove) {
      // 纯光标移动：从 segments 插值
      if (plan.segments.length > 0) {
        const seg = plan.segments[0]
        const caretX = seg.startCaretX + (seg.endCaretX - seg.startCaretX) * p
        const caretY = seg.startCaretY + (seg.endCaretY - seg.startCaretY) * p
        return {
          caretX,
          caretY,
          caretHeight: this.currentFrame.caretHeight,
          staticVisibleRanges: this.currentFrame.staticVisibleRanges,
          motionGlyphWindows: [],
          isAnimating: progress < 1,
          motionKind: MotionKind.CaretMove
        }
      }
      return this.currentFrame
    }

    // 正文增删动画：计算裁切前沿 front
    const motionGlyphWindows: MotionGlyphWindow[] = []

    if (plan.motionKind === MotionKind.Reveal) {
      // 吐字：新字完整排版，裁切窗口从 [x0, x0] 扩展到 [x0, x1]
      // front = x0 + (x1-x0) * easing(p)
      for (const target of plan.newGlyphTargets) {
        if (target.sourceKind === 'new') {
          const x0 = target.x
          const x1 = target.x + target.width
          const front = x0 + (x1 - x0) * p
          motionGlyphWindows.push({
            sourceKind: 'new',
            utf16Start: target.utf16Start,
            utf16End: target.utf16End,
            clipLeft: x0,
            clipRight: front,
            clipTop: target.y,
            clipBottom: target.y + target.height,
            glyphX: target.x,
            glyphY: target.y,
            glyphWidth: target.width,
            glyphHeight: target.height
          })
        }
      }
      // 光标画在 front
      const firstTarget = plan.newGlyphTargets.find((t: GlyphSource): boolean => t.sourceKind === 'new')
      if (firstTarget) {
        const front = firstTarget.x + firstTarget.width * p
        return {
          caretX: front,
          caretY: firstTarget.y,
          caretHeight: firstTarget.height,
          staticVisibleRanges: this.computeStaticRangesForReveal(plan),
          motionGlyphWindows,
          isAnimating: progress < 1,
          motionKind: MotionKind.Reveal
        }
      }
    }

    if (plan.motionKind === MotionKind.Conceal) {
      // 吞字：旧字源留在动画层，可见窗口 [x0, front]
      // front = x1 + (x0-x1) * easing(p) — 从 x1 退到 x0
      for (const source of plan.oldGlyphSources) {
        if (source.sourceKind === 'old') {
          const x0 = source.x
          const x1 = source.x + source.width
          const front = x1 + (x0 - x1) * p
          motionGlyphWindows.push({
            sourceKind: 'old',
            utf16Start: source.utf16Start,
            utf16End: source.utf16End,
            clipLeft: x0,
            clipRight: front,
            clipTop: source.y,
            clipBottom: source.y + source.height,
            glyphX: source.x,
            glyphY: source.y,
            glyphWidth: source.width,
            glyphHeight: source.height
          })
        }
      }
      // 光标画在 front
      const firstSource = plan.oldGlyphSources.find((s: GlyphSource): boolean => s.sourceKind === 'old')
      if (firstSource) {
        const x0 = firstSource.x
        const x1 = firstSource.x + firstSource.width
        const front = x1 + (x0 - x1) * p
        return {
          caretX: front,
          caretY: firstSource.y,
          caretHeight: firstSource.height,
          staticVisibleRanges: this.computeStaticRangesForConceal(plan),
          motionGlyphWindows,
          isAnimating: progress < 1,
          motionKind: MotionKind.Conceal
        }
      }
    }

    if (plan.motionKind === MotionKind.ConcealForward) {
      // Forward Delete：逻辑光标不移动，被删字贴向固定 caret 缩回
      // 光标位置 = 旧光标位置（不动），被删字的裁切窗口从 [caret, caret+width] 缩到 [caret, caret]
      for (const source of plan.oldGlyphSources) {
        if (source.sourceKind === 'old') {
          const caretX = this.currentFrame.caretX
          const x0 = caretX
          const x1 = source.x + source.width
          const front = x1 + (x0 - x1) * p
          motionGlyphWindows.push({
            sourceKind: 'old',
            utf16Start: source.utf16Start,
            utf16End: source.utf16End,
            clipLeft: caretX,
            clipRight: front,
            clipTop: source.y,
            clipBottom: source.y + source.height,
            glyphX: source.x,
            glyphY: source.y,
            glyphWidth: source.width,
            glyphHeight: source.height
          })
        }
      }
      // 光标不动
      return {
        caretX: this.currentFrame.caretX,
        caretY: this.currentFrame.caretY,
        caretHeight: this.currentFrame.caretHeight,
        staticVisibleRanges: this.computeStaticRangesForConceal(plan),
        motionGlyphWindows,
        isAnimating: progress < 1,
        motionKind: MotionKind.ConcealForward
      }
    }

    if (plan.motionKind === MotionKind.Reflow) {
      // Reflow：保留文字从旧位置平移到新位置
      for (const target of plan.newGlyphTargets) {
        if (target.sourceKind === 'new') {
          // 平移：从旧位置插值到新位置
          // 旧位置需要从 plan.oldGlyphSources 或 offsetMap 推断
          // 简化实现：直接在新位置画，不做平移动画（平移信息由 Planner 提供）
          motionGlyphWindows.push({
            sourceKind: 'new',
            utf16Start: target.utf16Start,
            utf16End: target.utf16End,
            clipLeft: target.x,
            clipRight: target.x + target.width,
            clipTop: target.y,
            clipBottom: target.y + target.height,
            glyphX: target.x,
            glyphY: target.y,
            glyphWidth: target.width,
            glyphHeight: target.height
          })
        }
      }
      return {
        caretX: this.currentFrame.caretX,
        caretY: this.currentFrame.caretY,
        caretHeight: this.currentFrame.caretHeight,
        staticVisibleRanges: this.currentFrame.staticVisibleRanges,
        motionGlyphWindows,
        isAnimating: progress < 1,
        motionKind: MotionKind.Reflow
      }
    }

    // 默认：返回当前帧
    return this.currentFrame
  }

  /**
   * 是否正在动画中。
   */
  isAnimating(): boolean {
    return this.currentFrame.isAnimating
  }

  /**
   * 动画完成，归还静态层。
   *
   * 清除所有运动层字形窗口，把所有字形归还给静态层绘制。
   * 光标移动到最终位置。
   */
  finishMotion(): EditorVisualFrame {
    this.currentFrame = {
      caretX: this.currentFrame.caretX,
      caretY: this.currentFrame.caretY,
      caretHeight: this.currentFrame.caretHeight,
      staticVisibleRanges: this.currentFrame.staticVisibleRanges,
      motionGlyphWindows: [],
      isAnimating: false,
      motionKind: MotionKind.None
    }
    this.currentPlan = {
      motionKind: MotionKind.None,
      segments: [],
      affectedRanges: [],
      oldGlyphSources: [],
      newGlyphTargets: [],
      durationMs: 0
    }
    this.motionDurationMs = 0
    return this.currentFrame
  }

  /**
   * 获取当前视觉帧。
   */
  getCurrentFrame(): EditorVisualFrame {
    return this.currentFrame
  }

  /**
   * 获取当前运动规划。
   */
  getCurrentPlan(): EditorMotionPlan {
    return this.currentPlan
  }

  // ── 私有方法 ──

  /**
   * easing 函数：三次方缓出。
   * p <= 0 ? 0 : p >= 1 ? 1 : 1 - Math.pow(1 - p, 3)
   */
  private static easing(p: number): number {
    if (p <= 0) {
      return 0
    }
    if (p >= 1) {
      return 1
    }
    return 1 - Math.pow(1 - p, 3)
  }

  /**
   * 判断两个渲染身份是否相等。
   */
  private static identityEquals(a: LayoutIdentity, b: LayoutIdentity): boolean {
    return a.revision === b.revision &&
           a.generation === b.generation &&
           a.compositionSessionId === b.compositionSessionId &&
           a.compositionGeneration === b.compositionGeneration &&
           a.fontSize === b.fontSize &&
           a.lineSpacing === b.lineSpacing &&
           a.contentWidth === b.contentWidth
  }

  /**
   * 计算静态帧（无动画）。
   */
  private static computeStaticFrame(
    snapshot: EditorLayoutSnapshot,
    lineLayouts: LineLayout[]
  ): EditorVisualFrame {
    const cursorUtf16 = TextOffsetMapper.utf8ToUtf16(snapshot.text, snapshot.cursorByteOffset)
    let caretX = 0
    let caretY = 0
    let caretHeight = 0

    for (const line of lineLayouts) {
      if (cursorUtf16 >= line.startUtf16 && cursorUtf16 <= line.endUtf16) {
        let x = line.left
        for (const stop of line.caretStops) {
          if (stop.utf16Offset <= cursorUtf16) {
            x = stop.x
          }
        }
        caretX = x
        caretY = line.y
        caretHeight = line.height
        break
      }
    }

    // 静态帧：所有字形都由静态层绘制
    const staticVisibleRanges: StaticVisibleRange[] = []
    if (lineLayouts.length > 0) {
      staticVisibleRanges.push({
        utf16Start: lineLayouts[0].startUtf16,
        utf16End: lineLayouts[lineLayouts.length - 1].endUtf16
      })
    }

    return {
      caretX,
      caretY,
      caretHeight,
      staticVisibleRanges,
      motionGlyphWindows: [],
      isAnimating: false,
      motionKind: MotionKind.None
    }
  }

  /**
   * 吐字时计算静态层可见范围。
   * 吐字期间，被吐出的新字由运动层绘制，静态层不画这些字。
   */
  private computeStaticRangesForReveal(plan: EditorMotionPlan): StaticVisibleRange[] {
    // 简化实现：静态层画除新字目标外的所有字形
    // 实际实现需要根据 plan.newGlyphTargets 的范围排除
    if (plan.newGlyphTargets.length === 0) {
      return this.currentFrame.staticVisibleRanges
    }
    // 排除新字目标的范围
    const excludedRanges = plan.newGlyphTargets
      .filter((t: GlyphSource): boolean => t.sourceKind === 'new')
      .map((t: GlyphSource): StaticVisibleRange => ({
        utf16Start: t.utf16Start,
        utf16End: t.utf16End
      }))

    return EditorMotionCoordinator.subtractRanges(
      this.currentFrame.staticVisibleRanges,
      excludedRanges
    )
  }

  /**
   * 吞字时计算静态层可见范围。
   * 吞字期间，被吞的旧字由运动层绘制，静态层不画这些字。
   */
  private computeStaticRangesForConceal(plan: EditorMotionPlan): StaticVisibleRange[] {
    if (plan.oldGlyphSources.length === 0) {
      return this.currentFrame.staticVisibleRanges
    }
    // 排除旧字源的范围
    const excludedRanges = plan.oldGlyphSources
      .filter((s: GlyphSource): boolean => s.sourceKind === 'old')
      .map((s: GlyphSource): StaticVisibleRange => ({
        utf16Start: s.utf16Start,
        utf16End: s.utf16End
      }))

    return EditorMotionCoordinator.subtractRanges(
      this.currentFrame.staticVisibleRanges,
      excludedRanges
    )
  }

  /**
   * 从可见范围列表中减去排除范围列表。
   */
  private static subtractRanges(
    visible: StaticVisibleRange[],
    excluded: StaticVisibleRange[]
  ): StaticVisibleRange[] {
    if (excluded.length === 0) {
      return visible
    }
    const result: StaticVisibleRange[] = []
    for (const vis of visible) {
      let start = vis.utf16Start
      const end = vis.utf16End
      for (const exc of excluded) {
        if (exc.utf16End <= start || exc.utf16Start >= end) {
          continue
        }
        // 排除范围在可见范围内：拆分
        if (exc.utf16Start > start) {
          result.push({ utf16Start: start, utf16End: exc.utf16Start })
        }
        start = Math.max(start, exc.utf16End)
      }
      if (start < end) {
        result.push({ utf16Start: start, utf16End: end })
      }
    }
    return result
  }
}
