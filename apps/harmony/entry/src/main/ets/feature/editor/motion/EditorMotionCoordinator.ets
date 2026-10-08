// EditorMotionCoordinator — 运动协调器。
//
// Issue #879：保存上一帧真正已显示的状态，retarget 到最新 canonical。
//
// 核心职责：
// 1. 接收 EditorPresentationUpdate，保存当前可见画面。
// 2. 用 EditorMotionPlanner 生成新 plan。
// 3. 直接 retarget——不等旧动画完成，立即切换到新 plan 的起始状态。
// 4. tick(now) 逐帧驱动：接收时间戳，计算 progress，调用 sampleFrame 并回写 currentFrame。
// 5. sampleFrame(progress) 从 plan 和 progress 计算视觉帧：
//    - 吐字：front = x0 + (x1-x0) * easing(p)
//    - 吞字：front = x1 + (x0-x1) * easing(p)
//    - 光标画在 front
// 6. finishMotion() 动画完成，按最新目标全文 layout 重建静态所有权。
//
// Issue #879 复核评论修复：
// - 拆分身份检查：GeometryKey（fontSize, lineSpacing, contentWidth, compositionSessionId）
//   和 TextRevisionKey（revision, generation, compositionGeneration）。
//   正文版本变化不能作为取消动画理由——只有会话切换、字号/宽度变化才走静态重建。
// - 添加旧布局缓存：保存上一帧的 EditorLayoutSnapshot（含旧 text）和旧 LineLayout[]，
//   供 Planner 计算旧字形位置。
// - 添加 tick(now) 方法：接收当前时间戳，计算 progress，调用 sampleFrame 并回写 currentFrame。
//   progress 达到 1 时在同一帧调用 finishMotion()。
// - 修复 finishMotion：结束帧按最新目标全文 layout 重建静态所有权（所有字形归静态层），
//   不照搬可能已排除目标文字的 staticVisibleRanges。
// - contentWidth 由调用方传入。
//
// easing 函数：p <= 0 ? 0 : p >= 1 ? 1 : 1 - Math.pow(1 - p, 3)

import { EditorVisualFrame, MotionKind, MotionGlyphWindow, StaticVisibleRange } from './EditorVisualFrame'
import { EditorMotionPolicy } from './EditorMotionPolicy'
import { EditorMotionPlan, EditorMotionPlanner, GlyphSource } from './EditorMotionPlanner'
import { EditorLayoutSnapshot } from '../render/EditorLayoutSnapshot'
import { LineLayout } from '../render/editor_render_geometry'
import { TextOffsetMapper } from '../input/TextOffsetMapper'
import { EditorEditResult } from '../../../corebridge/dto/EditorDtos'

/**
 * 几何身份——标识绘制环境是否改变。
 *
 * 只有这些字段变化时才说明布局重建（字号变更/宽度变化/会话切换），
 * 运动协调器应直接显示静态画面，不启动动画。
 *
 * Issue #879 复核评论修复：正文版本（revision/generation/compositionGeneration）
 * 不属于几何身份——正常打字会改变 revision，但不应取消动画。
 */
export interface GeometryKey {
  /** 字体大小（vp） */
  readonly fontSize: number
  /** 行距倍数 */
  readonly lineSpacing: number
  /** 内容宽度（vp）——由调用方传入 */
  readonly contentWidth: number
  /** 编辑会话 ID——会话切换说明切换了章节/文档 */
  readonly compositionSessionId: number
}

/**
 * 正文版本身份——标识新旧布局是否对应本次编辑。
 *
 * 用于验证 editResult.baseRevision == cachedOldRevision、
 * editResult.newRevision == newSnapshot.revision，
 * 确保规划时使用的旧布局确实对应本次编辑的 base revision。
 */
export interface TextRevisionKey {
  /** 正文 revision */
  readonly revision: number
  /** 编辑 generation */
  readonly generation: number
  /** composition generation */
  readonly compositionGeneration: number
}

/**
 * 运动协调器。
 *
 * 保存上一帧真正已显示的状态，接收新的编辑更新后 retarget 到最新 canonical。
 * 不等旧动画完成——如果用户快速连续输入，直接切换到新 plan 的起始状态。
 *
 * Issue #879 复核评论修复：
 * - 拆分 GeometryKey / TextRevisionKey，正文版本变化不取消动画
 * - 添加旧布局缓存，供 Planner 计算旧字形位置
 * - 添加 tick(now) 逐帧驱动方法
 * - 修复 finishMotion 按最新 layout 重建静态所有权
 */
export class EditorMotionCoordinator {
  /** 当前视觉帧 */
  private currentFrame: EditorVisualFrame
  /** 当前几何身份 */
  private geometryKey: GeometryKey
  /** 当前正文版本身份 */
  private textRevisionKey: TextRevisionKey
  /** 旧布局快照缓存（含旧 text）——供 Planner 计算旧字形位置 */
  private oldSnapshot: EditorLayoutSnapshot | null
  /** 旧行布局缓存——供 Planner 计算旧字形几何 */
  private oldLayout: LineLayout[]
  /** 新布局快照（含新 text）——供 Planner 计算新字形位置和 finishMotion 重建 */
  private newSnapshot: EditorLayoutSnapshot | null
  /** 新行布局——供 Planner 计算新字形几何和 finishMotion 重建 */
  private newLayout: LineLayout[]
  /** 动画开始时间（ms，performance.now 语义） */
  private motionStartTime: number
  /** 动画时长（ms） */
  private motionDurationMs: number
  /** 当前运动规划 */
  private currentPlan: EditorMotionPlan

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
    this.geometryKey = {
      fontSize: 16,
      lineSpacing: 1.8,
      contentWidth: 0,
      compositionSessionId: 0
    }
    this.textRevisionKey = {
      revision: 0,
      generation: 0,
      compositionGeneration: -1
    }
    this.oldSnapshot = null
    this.oldLayout = []
    this.newSnapshot = null
    this.newLayout = []
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
  }

  /**
   * 接收编辑更新，生成新 plan，retarget 到最新 canonical。
   *
   * Issue #879 复核评论修复：
   * - 先验证 editResult.baseRevision == cachedOldRevision、editResult.newRevision == newSnapshot.revision
   * - 只有 GeometryKey 变化（会话切换/字号/宽度变化）才走静态重建
   * - 正文版本变化不取消动画
   * - contentWidth 由调用方传入
   * - 入参改为 oldSnapshot + oldLayout + newSnapshot + newLayout + editResult + lastVisibleFrame
   *
   * @param oldSnapshot 旧布局快照（含旧正文 text）
   * @param oldLayout 旧行布局
   * @param newSnapshot 新布局快照（含新正文 text）
   * @param newLayout 新行布局
   * @param editResult 本次编辑事务结果（null 表示非编辑触发的更新）
   * @param contentWidth 内容宽度（vp）
   * @param policy 运动策略
   * @returns 新的视觉帧（动画起始帧）
   */
  acceptUpdate(
    oldSnapshot: EditorLayoutSnapshot,
    oldLayout: LineLayout[],
    newSnapshot: EditorLayoutSnapshot,
    newLayout: LineLayout[],
    editResult: EditorEditResult | null,
    contentWidth: number,
    policy: EditorMotionPolicy
  ): EditorVisualFrame {
    // 计算新几何身份
    const newGeometryKey: GeometryKey = {
      fontSize: newSnapshot.fontSize,
      lineSpacing: newSnapshot.lineSpacingMultiplier,
      contentWidth: contentWidth,
      compositionSessionId: newSnapshot.compositionSessionId
    }

    // 几何身份变化（字号变更/会话切换/宽度变化）→ 直接显示静态画面
    if (!EditorMotionCoordinator.geometryEquals(this.geometryKey, newGeometryKey)) {
      this.geometryKey = newGeometryKey
      this.textRevisionKey = {
        revision: newSnapshot.revision,
        generation: newSnapshot.generation,
        compositionGeneration: newSnapshot.compositionGeneration
      }
      this.oldSnapshot = newSnapshot
      this.oldLayout = newLayout
      this.newSnapshot = newSnapshot
      this.newLayout = newLayout
      this.currentFrame = EditorMotionCoordinator.computeStaticFrame(newSnapshot, newLayout)
      this.currentPlan = {
        motionKind: MotionKind.None,
        segments: [],
        affectedRanges: [],
        oldGlyphSources: [],
        newGlyphTargets: [],
        durationMs: 0
      }
      this.motionDurationMs = 0
      this.currentFrame.isAnimating = false
      return this.currentFrame
    }

    // 验证 editResult 的 baseRevision 和 newRevision 是否匹配
    if (editResult !== null) {
      // editResult.baseRevision 应该等于旧 snapshot 的 revision
      // editResult.newRevision 应该等于新 snapshot 的 revision
      // 如果不匹配，说明布局缓存已过期，走静态重建
      if (editResult.baseRevision !== oldSnapshot.revision ||
          editResult.newRevision !== newSnapshot.revision) {
        this.geometryKey = newGeometryKey
        this.textRevisionKey = {
          revision: newSnapshot.revision,
          generation: newSnapshot.generation,
          compositionGeneration: newSnapshot.compositionGeneration
        }
        this.oldSnapshot = newSnapshot
        this.oldLayout = newLayout
        this.newSnapshot = newSnapshot
        this.newLayout = newLayout
        this.currentFrame = EditorMotionCoordinator.computeStaticFrame(newSnapshot, newLayout)
        this.currentPlan = {
          motionKind: MotionKind.None,
          segments: [],
          affectedRanges: [],
          oldGlyphSources: [],
          newGlyphTargets: [],
          durationMs: 0
        }
        this.motionDurationMs = 0
        this.currentFrame.isAnimating = false
        return this.currentFrame
      }
    }

    // 保存当前可见画面作为起始状态
    const startFrame = this.currentFrame

    // 用 Planner 生成新 plan（传入 oldSnapshot + oldLayout + newSnapshot + newLayout）
    this.currentPlan = EditorMotionPlanner.plan(
      oldSnapshot, oldLayout, newSnapshot, newLayout, editResult, startFrame
    )

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

    // 更新缓存
    this.oldSnapshot = oldSnapshot
    this.oldLayout = oldLayout
    this.newSnapshot = newSnapshot
    this.newLayout = newLayout
    this.geometryKey = newGeometryKey
    this.textRevisionKey = {
      revision: newSnapshot.revision,
      generation: newSnapshot.generation,
      compositionGeneration: newSnapshot.compositionGeneration
    }

    // 如果不需要动画（时长为 0），直接返回终态帧
    if (this.motionDurationMs <= 0) {
      this.currentFrame = EditorMotionCoordinator.computeStaticFrame(newSnapshot, newLayout)
      this.currentFrame.isAnimating = false
      return this.currentFrame
    }

    // 记录动画开始时间——由 tick(now) 驱动
    this.motionStartTime = 0 // 将在第一次 tick 时设置

    // 生成动画起始帧（progress=0）并回写 currentFrame
    this.currentFrame = this.sampleFrame(0)
    return this.currentFrame
  }

  /**
   * 逐帧驱动——接收当前时间戳，计算 progress，调用 sampleFrame 并回写 currentFrame。
   *
   * Issue #879 复核评论修复：添加 tick(now) 方法实现逐帧驱动。
   * - 第一次调用时设置 motionStartTime = now
   * - progress = clamp((now - motionStartTime) / motionDurationMs, 0, 1)
   * - 调用 sampleFrame(progress) 并回写 currentFrame
   * - progress 达到 1 时在同一帧调用 finishMotion()
   *
   * @param now 当前时间戳（ms，performance.now 语义）
   * @returns 当前进度的视觉帧
   */
  tick(now: number): EditorVisualFrame {
    // 如果不在动画中，直接返回当前帧
    if (!this.currentFrame.isAnimating || this.motionDurationMs <= 0) {
      return this.currentFrame
    }

    // 第一次调用时设置动画开始时间
    if (this.motionStartTime === 0) {
      this.motionStartTime = now
    }

    // 计算 progress
    const rawProgress = (now - this.motionStartTime) / this.motionDurationMs
    const progress = Math.max(0, Math.min(1, rawProgress))

    // 调用 sampleFrame 并回写 currentFrame
    this.currentFrame = this.sampleFrame(progress)

    // progress 达到 1 时在同一帧调用 finishMotion()
    if (progress >= 1) {
      this.currentFrame = this.finishMotion()
    }

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
        // 多段轨迹：按 progress 分配到各段
        const segCount = plan.segments.length
        const segProgress = progress * segCount
        const segIndex = Math.min(segCount - 1, Math.floor(segProgress))
        const localProgress = Math.min(1, segProgress - segIndex)
        const seg = plan.segments[segIndex]
        const localEased = EditorMotionCoordinator.easing(localProgress)
        const caretX = seg.startCaretX + (seg.endCaretX - seg.startCaretX) * localEased
        const caretY = seg.startCaretY + (seg.endCaretY - seg.startCaretY) * localEased
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
   * Issue #879 复核评论修复：
   * 结束帧按最新目标全文 layout 重建静态所有权（所有字形归静态层），
   * 和撤窗口一次提交。不照搬可能已排除目标文字的 staticVisibleRanges。
   */
  finishMotion(): EditorVisualFrame {
    // 按最新目标全文 layout 重建静态所有权
    // 所有字形归静态层——用 newSnapshot + newLayout 计算完整的静态可见范围
    const staticVisibleRanges: StaticVisibleRange[] = []
    if (this.newSnapshot !== null && this.newLayout.length > 0) {
      // 所有字形都归静态层
      staticVisibleRanges.push({
        utf16Start: this.newLayout[0].startUtf16,
        utf16End: this.newLayout[this.newLayout.length - 1].endUtf16
      })
    }

    // 计算最终光标位置（从 newSnapshot + newLayout）
    let caretX = this.currentFrame.caretX
    let caretY = this.currentFrame.caretY
    let caretHeight = this.currentFrame.caretHeight
    if (this.newSnapshot !== null && this.newLayout.length > 0) {
      const cursorUtf16 = TextOffsetMapper.utf8ToUtf16(this.newSnapshot.text, this.newSnapshot.cursorByteOffset)
      for (const line of this.newLayout) {
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
    }

    this.currentFrame = {
      caretX,
      caretY,
      caretHeight,
      staticVisibleRanges,
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
    this.motionStartTime = 0
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
   * 判断两个几何身份是否相等。
   *
   * Issue #879 复核评论修复：只比较几何字段（fontSize, lineSpacing, contentWidth, compositionSessionId）。
   * 正文版本（revision/generation/compositionGeneration）不参与比较——
   * 正常打字会改变 revision，但不应取消动画。
   */
  private static geometryEquals(a: GeometryKey, b: GeometryKey): boolean {
    return a.fontSize === b.fontSize &&
           a.lineSpacing === b.lineSpacing &&
           a.contentWidth === b.contentWidth &&
           a.compositionSessionId === b.compositionSessionId
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

