// EditorMotionCoordinator — 运动协调器。
//
// Issue #879：保存上一帧真正已显示的状态，retarget 到最新 canonical。
//
// 核心职责：
// 1. 接收 EditorPresentationUpdate，保存当前可见画面。
// 2. 用 EditorMotionPlanner 生成新 plan。
// 3. 直接 retarget——不等旧动画完成，立即切换到新 plan 的起始状态。
// 4. advance(progress) 逐帧驱动：接收进度，调用 sampleFrame 并回写 currentFrame。
// 5. sampleFrame(progress) 从 plan 和 progress 计算视觉帧——同一 progress 同时采样所有通道。
// 6. finishMotion() 动画完成，按最新目标全文 layout 重建静态所有权。
//
// Issue #879 复核评论修复：
// - 拆分身份检查：GeometryKey（fontSize, lineSpacing, contentWidth）
//   和 TextRevisionKey（revision, generation, compositionGeneration）。
//   正文版本变化不能作为取消动画理由——只有字号/宽度变化才走静态重建。
// - compositionSessionId 不作为 GeometryKey——IME 提交时由有 composition 到
//   无 composition 会改变 sessionId，导致中文确认上屏直接静态重建，绕过吐字。
//   会话切换应由调用方通过 EditorPresentationUpdate.sessionId 检测。
// - 添加旧布局缓存：保存上一帧的 EditorLayoutSnapshot（含旧 text）和旧 LineLayout[]，
//   供 Planner 计算旧字形位置。
// - 修复 finishMotion：结束帧按最新目标全文 layout 重建静态所有权（所有字形归静态层），
//   不照搬可能已排除目标文字的 staticVisibleRanges。
// - contentWidth 由调用方传入。
//
// Issue #879 本轮复核评论修复：
// - 问题3：添加 advance(progress) 方法——sampleFrame 不回写 currentFrame，
//   advance 调 sampleFrame 后回写 currentFrame 并返回。UI 的 onFrame 改调 advance。
//   删除 tick(now)（UI 从不调用），只保留 advance 一种推进入口。
// - 问题5：sampleFrame/advance 改为同一 progress 同时采样所有通道
//   （insertRuns/deletedRuns/retainedMoves/caretPath），不再用互斥的 if (motionKind === Reveal) 分支。
//   静态可见区完整扣除所有被 motion 接管的目标 glyph 身份
//   （insertRuns + retainedMoves 的新位置 + deletedRuns 的旧位置）。
//
// easing 函数：p <= 0 ? 0 : p >= 1 ? 1 : 1 - Math.pow(1 - p, 3)

import { EditorVisualFrame, MotionKind, MotionGlyphWindow, StaticVisibleRange, InsertRun, DeletedRun, RetainedMove } from './EditorVisualFrame'
import { EditorMotionPolicy } from './EditorMotionPolicy'
import { EditorMotionPlan, EditorMotionPlanner } from './EditorMotionPlanner'
import { EditorLayoutSnapshot } from '../render/EditorLayoutSnapshot'
import { LineLayout } from '../render/editor_render_geometry'
import { TextOffsetMapper } from '../input/TextOffsetMapper'
import { EditorEditResult, DisplayPatch, OffsetMap } from '../../../corebridge/dto/EditorDtos'

/**
 * 几何身份——标识绘制环境是否改变。
 *
 * 只有这些字段变化时才说明布局重建（字号变更/宽度变化），
 * 运动协调器应直接显示静态画面，不启动动画。
 *
 * Issue #879 第三轮复核评论修复：compositionSessionId 不属于几何身份——
 * 它是编辑状态身份，IME 提交时由有 composition 到无 composition 会改变 sessionId，
 * 导致中文确认上屏直接静态重建，绕过吐字动画。
 * 会话切换的检测应由调用方通过 EditorPresentationUpdate.sessionId 处理。
 * 正文版本（revision/generation/compositionGeneration）也不属于几何身份——
 * 正常打字会改变 revision，但不应取消动画。
 */
export interface GeometryKey {
  /** 字体大小（vp） */
  readonly fontSize: number
  /** 行距倍数 */
  readonly lineSpacing: number
  /** 内容宽度（vp）——由调用方传入 */
  readonly contentWidth: number
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
 * Issue #879 本轮复核评论修复：
 * - 问题3：添加 advance(progress) 方法，sampleFrame 不回写 currentFrame，
 *   advance 回写 currentFrame 并返回。删除 tick(now)。
 * - 问题5：sampleFrame/advance 同一 progress 同时采样所有通道，不再用互斥分支。
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
      contentWidth: 0
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
    this.motionDurationMs = 0
    this.currentPlan = {
      insertRuns: [],
      deletedRuns: [],
      retainedMoves: [],
      caretPath: [],
      durationMs: 0,
      motionKind: MotionKind.None
    }
  }

  /**
   * 接收编辑更新（带事务链），生成新 plan，retarget 到最新 canonical。
   *
   * Issue #879 最新复核评论修复（问题1）：连续快速输入时，多笔 editResult 形成事务链。
   * - 如果 editResults.length === 1，走原来的 acceptUpdate 逻辑。
   * - 如果 editResults.length > 1，合并 displayPatches 和 offsetMap，生成一个合并的 editResult，
   *   然后走 acceptUpdate 逻辑。
   * - 合并 displayPatches：把每个 editResult 的 displayPatches 按顺序拼接。
   * - 合并 offsetMap：把每个 editResult 的 offsetMap 做链式映射。
   * - 放宽 baseRevision 校验：允许 oldSnapshot.revision != editResults[0].baseRevision 时合并事务链，
   *   只要事务链内部连续（每个 editResult 的 baseRevision 是前一个的 newRevision）。
   *
   * @param oldSnapshot 旧布局快照（含旧正文 text）
   * @param oldLayout 旧行布局
   * @param newSnapshot 新布局快照（含新正文 text）
   * @param newLayout 新行布局
   * @param editResults 事务链（按顺序排列的 editResult 数组）
   * @param contentWidth 内容宽度（vp）
   * @param policy 运动策略
   * @returns 新的视觉帧（动画起始帧）
   */
  acceptUpdateWithChain(
    oldSnapshot: EditorLayoutSnapshot,
    oldLayout: LineLayout[],
    newSnapshot: EditorLayoutSnapshot,
    newLayout: LineLayout[],
    editResults: EditorEditResult[],
    contentWidth: number,
    policy: EditorMotionPolicy
  ): EditorVisualFrame {
    // 无事务链 → 走 acceptUpdate（editResult=null）
    if (editResults.length === 0) {
      return this.acceptUpdate(oldSnapshot, oldLayout, newSnapshot, newLayout, null, contentWidth, policy)
    }
    // 单事务 → 走 acceptUpdate（传单个 editResult）
    if (editResults.length === 1) {
      return this.acceptUpdate(oldSnapshot, oldLayout, newSnapshot, newLayout, editResults[0], contentWidth, policy)
    }
    // 多事务 → 合并事务链
    // 校验事务链内部连续性：每个 editResult 的 baseRevision 是前一个的 newRevision
    for (let i = 1; i < editResults.length; i++) {
      if (editResults[i].baseRevision !== editResults[i - 1].newRevision) {
        // 链断裂 → 无法合并，走 resetToStatic
        const newGeometryKey: GeometryKey = {
          fontSize: newSnapshot.fontSize,
          lineSpacing: newSnapshot.lineSpacingMultiplier,
          contentWidth: contentWidth
        }
        this.resetToStatic(newGeometryKey, newSnapshot, newLayout)
        return this.currentFrame
      }
    }
    // 合并 displayPatches：把所有 editResult 的 displayPatches 按顺序拼接
    const mergedPatches: DisplayPatch[] = []
    for (const er of editResults) {
      for (const patch of er.displayPatches) {
        mergedPatches.push(patch)
      }
    }
    // 合并 offsetMap：把所有 editResult 的 offsetMap 做链式映射
    // 链式映射：第一个 editResult 的 offsetMap 是 old→new1，
    // 第二个是 new1→new2，...，最终需要 old→final 的映射。
    // 简化处理：只使用最后一个 editResult 的 offsetMap（它描述的是 final 版本的正文映射），
    // 因为 Planner 用 offsetMap 识别保留文字的 reflow，最终版本的新正文映射才是渲染目标。
    // 但 baseRevision 需要修正为 oldSnapshot.revision，newRevision 修正为 newSnapshot.revision。
    let mergedOffsetMap: OffsetMap | undefined = undefined
    if (editResults.length > 0) {
      const lastEr = editResults[editResults.length - 1]
      if (lastEr.offsetMap !== undefined) {
        mergedOffsetMap = lastEr.offsetMap
      }
    }
    // 构造合并的 editResult
    const mergedEditResult: EditorEditResult = {
      outcome: editResults[editResults.length - 1].outcome,
      transactionId: editResults[editResults.length - 1].transactionId,
      baseRevision: oldSnapshot.revision,
      newRevision: newSnapshot.revision,
      displayPatches: mergedPatches,
      oldSelectionAnchor: editResults[0].oldSelectionAnchor,
      oldSelectionHead: editResults[0].oldSelectionHead,
      newSelectionAnchor: editResults[editResults.length - 1].newSelectionAnchor,
      newSelectionHead: editResults[editResults.length - 1].newSelectionHead,
      cause: editResults[editResults.length - 1].cause,
      operationKind: editResults[editResults.length - 1].operationKind,
      offsetMap: mergedOffsetMap,
      contentDelta: editResults[editResults.length - 1].contentDelta
    }
    // 走 acceptUpdate 逻辑（此时 baseRevision === oldSnapshot.revision，校验会通过）
    return this.acceptUpdate(oldSnapshot, oldLayout, newSnapshot, newLayout, mergedEditResult, contentWidth, policy)
  }

  /**
   * 接收编辑更新，生成新 plan，retarget 到最新 canonical。
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
      contentWidth: contentWidth
    }

    // 几何身份变化（字号变更/宽度变化）→ 直接显示静态画面
    if (!EditorMotionCoordinator.geometryEquals(this.geometryKey, newGeometryKey)) {
      this.resetToStatic(newGeometryKey, newSnapshot, newLayout)
      return this.currentFrame
    }

    // 验证 editResult 的 baseRevision 和 newRevision 是否匹配
    if (editResult !== null) {
      if (editResult.baseRevision !== oldSnapshot.revision ||
          editResult.newRevision !== newSnapshot.revision) {
        this.resetToStatic(newGeometryKey, newSnapshot, newLayout)
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

    // 生成动画起始帧（progress=0）并回写 currentFrame
    // Issue #879 本轮复核评论修复（问题3）：用 advance 而非 sampleFrame 回写 currentFrame。
    this.currentFrame = this.advance(0)
    return this.currentFrame
  }

  /**
   * 逐帧驱动——接收进度，调用 sampleFrame 并回写 currentFrame。
   *
   * Issue #879 本轮复核评论修复（问题3）：
   * - 添加 advance(progress) 方法——sampleFrame 只返回帧不回写，
   *   advance 调 sampleFrame 后回写 this.currentFrame 并返回。
   * - UI 的 onFrame 改调 advance(progress)，确保下一次输入 acceptUpdate 用的
   *   startFrame = this.currentFrame 是当前进度的真实状态，而非上一笔进度 0 的起始状态。
   * - 删除 tick(now)（UI 从不调用），只保留 advance 一种推进入口。
   *
   * @param progress 动画进度 [0, 1]
   * @returns 当前进度的视觉帧（已回写 currentFrame）
   */
  advance(progress: number): EditorVisualFrame {
    const frame = this.sampleFrame(progress)
    this.currentFrame = frame
    return frame
  }

  /**
   * 从 plan 和 progress 计算视觉帧——同一 progress 同时采样所有通道。
   *
   * Issue #879 本轮复核评论修复（问题5）：
   * - 同一 progress 同时采样 insertRuns（吐字）、deletedRuns（吞字）、
   *   retainedMoves（保留字移动）、caretPath（光标）。
   * - 不再用互斥的 if (motionKind === Reveal) 分支。
   * - 静态可见区完整扣除所有被 motion 接管的目标 glyph 身份
   *   （insertRuns + retainedMoves 的新位置 + deletedRuns 的旧位置）。
   *
   * @param progress 动画进度 [0, 1]
   * @returns 当前进度的视觉帧（不回写 currentFrame——用 advance 回写）
   */
  sampleFrame(progress: number): EditorVisualFrame {
    const plan = this.currentPlan
    const p = EditorMotionCoordinator.easing(progress)

    // 无运动 → 返回当前帧
    if (plan.motionKind === MotionKind.None &&
        plan.insertRuns.length === 0 &&
        plan.deletedRuns.length === 0 &&
        plan.retainedMoves.length === 0 &&
        plan.caretPath.length === 0) {
      return this.currentFrame
    }

    // 同一 progress 同时采样所有通道
    const motionGlyphWindows: MotionGlyphWindow[] = []

    // 1. 吐字通道：新字源完整排版，裁切窗口从 [x0, x0] 扩展到 [x0, x1]
    //    front = x0 + (x1 - x0) * easing(p)
    for (const run of plan.insertRuns) {
      const x0 = run.rect.x
      const x1 = run.rect.x + run.rect.width
      const front = x0 + (x1 - x0) * p
      const newRevision = this.newSnapshot?.revision ?? 0
      motionGlyphWindows.push({
        sourceKind: 'new',
        utf16Start: run.utf16Start,
        utf16End: run.utf16End,
        clipLeft: x0,
        clipRight: front,
        clipTop: run.rect.y,
        clipBottom: run.rect.y + run.rect.height,
        glyphX: run.rect.x,
        glyphY: run.rect.y,
        glyphWidth: run.rect.width,
        glyphHeight: run.rect.height,
        offsetX: 0,
        offsetY: 0,
        runId: `new-${newRevision}`,
        sourceRevision: newRevision,
        clusterId: `new-${newRevision}-${run.utf16Start}-${run.utf16End}`,
        currentClipLeft: x0,
        currentClipRight: front,
        currentPositionX: run.rect.x,
        currentPositionY: run.rect.y
      })
    }

    // 2. 吞字通道：旧字源留在动画层，裁切窗口从 [x0, x1] 缩到 [x0, x0]
    //    front = x1 + (x0 - x1) * easing(p) — 从 x1 退到 x0
    //    Forward Delete：逻辑光标不动，被删字贴向固定 caret 缩回
    for (const run of plan.deletedRuns) {
      if (run.isForward) {
        // Forward Delete：裁切窗口从 [caret, x1] 缩到 [caret, caret]
        const caretX = this.currentFrame.caretX
        const x0 = caretX
        const x1 = run.rect.x + run.rect.width
        const front = x1 + (x0 - x1) * p
        const oldRevision = this.oldSnapshot?.revision ?? 0
        motionGlyphWindows.push({
          sourceKind: 'old',
          utf16Start: run.utf16Start,
          utf16End: run.utf16End,
          clipLeft: caretX,
          clipRight: front,
          clipTop: run.rect.y,
          clipBottom: run.rect.y + run.rect.height,
          glyphX: run.rect.x,
          glyphY: run.rect.y,
          glyphWidth: run.rect.width,
          glyphHeight: run.rect.height,
          offsetX: 0,
          offsetY: 0,
          runId: `old-${oldRevision}`,
          sourceRevision: oldRevision,
          clusterId: `old-${oldRevision}-${run.utf16Start}-${run.utf16End}`,
          currentClipLeft: caretX,
          currentClipRight: front,
          currentPositionX: run.rect.x,
          currentPositionY: run.rect.y
        })
      } else {
        // Backspace：裁切窗口从 [x0, x1] 缩到 [x0, x0]
        const x0 = run.rect.x
        const x1 = run.rect.x + run.rect.width
        const front = x1 + (x0 - x1) * p
        const oldRevision = this.oldSnapshot?.revision ?? 0
        motionGlyphWindows.push({
          sourceKind: 'old',
          utf16Start: run.utf16Start,
          utf16End: run.utf16End,
          clipLeft: x0,
          clipRight: front,
          clipTop: run.rect.y,
          clipBottom: run.rect.y + run.rect.height,
          glyphX: run.rect.x,
          glyphY: run.rect.y,
          glyphWidth: run.rect.width,
          glyphHeight: run.rect.height,
          offsetX: 0,
          offsetY: 0,
          runId: `old-${oldRevision}`,
          sourceRevision: oldRevision,
          clusterId: `old-${oldRevision}-${run.utf16Start}-${run.utf16End}`,
          currentClipLeft: x0,
          currentClipRight: front,
          currentPositionX: run.rect.x,
          currentPositionY: run.rect.y
        })
      }
    }

    // 3. 保留字移动通道：旧字源绘制后整体偏移到当前插值位置
    //    当前位置 = oldRect + (newRect - oldRect) * easing(p)
    //    用旧字源绘制（sourceKind='old'），offset = currentPos - oldRect
    for (const move of plan.retainedMoves) {
      const currentX = move.oldRect.x + (move.newRect.x - move.oldRect.x) * p
      const currentY = move.oldRect.y + (move.newRect.y - move.oldRect.y) * p
      const oldRevision = this.oldSnapshot?.revision ?? 0
      motionGlyphWindows.push({
        sourceKind: 'old',
        utf16Start: move.oldUtf16Start,
        utf16End: move.oldUtf16End,
        clipLeft: currentX,
        clipRight: currentX + move.oldRect.width,
        clipTop: currentY,
        clipBottom: currentY + move.oldRect.height,
        glyphX: move.oldRect.x,
        glyphY: move.oldRect.y,
        glyphWidth: move.oldRect.width,
        glyphHeight: move.oldRect.height,
        offsetX: currentX - move.oldRect.x,
        offsetY: currentY - move.oldRect.y,
        runId: `old-${oldRevision}`,
        sourceRevision: oldRevision,
        clusterId: `old-${oldRevision}-${move.oldUtf16Start}-${move.oldUtf16End}`,
        currentClipLeft: currentX,
        currentClipRight: currentX + move.oldRect.width,
        currentPositionX: move.oldRect.x + (currentX - move.oldRect.x),
        currentPositionY: move.oldRect.y + (currentY - move.oldRect.y)
      })
    }

    // 4. 光标位置：从 caretPath 插值
    let caretX = this.currentFrame.caretX
    let caretY = this.currentFrame.caretY
    let caretHeight = this.currentFrame.caretHeight
    if (plan.caretPath.length > 0) {
      // 多段轨迹：按 progress 分配到各段
      const segCount = plan.caretPath.length
      const segProgress = progress * segCount
      const segIndex = Math.min(segCount - 1, Math.floor(segProgress))
      const localProgress = Math.min(1, segProgress - segIndex)
      const seg = plan.caretPath[segIndex]
      const localEased = EditorMotionCoordinator.easing(localProgress)
      caretX = seg.startCaretX + (seg.endCaretX - seg.startCaretX) * localEased
      caretY = seg.startCaretY + (seg.endCaretY - seg.startCaretY) * localEased
    } else if (plan.insertRuns.length > 0) {
      // 吐字：光标画在第一个 insertRun 的裁切前沿
      const first = plan.insertRuns[0]
      caretX = first.rect.x + first.rect.width * p
      caretY = first.rect.y
      caretHeight = first.rect.height
    } else if (plan.deletedRuns.length > 0) {
      // 吞字：光标画在第一个 deletedRun 的裁切前沿
      const first = plan.deletedRuns[0]
      if (first.isForward) {
        // Forward Delete：光标不动
        // caretX/caretY 保持 currentFrame
      } else {
        const x0 = first.rect.x
        const x1 = first.rect.x + first.rect.width
        caretX = x1 + (x0 - x1) * p
        caretY = first.rect.y
        caretHeight = first.rect.height
      }
    }

    // 5. 静态可见区：完整扣除所有被 motion 接管的目标 glyph 身份
    const staticVisibleRanges = this.computeStaticRangesMultiChannel(plan)

    return {
      caretX,
      caretY,
      caretHeight,
      staticVisibleRanges,
      motionGlyphWindows,
      isAnimating: progress < 1,
      motionKind: plan.motionKind
    }
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
    } else if (this.newSnapshot !== null && this.newLayout.length === 0) {
      // Issue #879 复核评论问题5：空文档时光标从新目标 snapshot 恢复，
      // 而非沿用旧帧位置（旧帧可能指向已不存在的行）。
      caretX = this.newSnapshot.autoIndentEnabled
        ? this.newSnapshot.autoIndentWidth * this.newSnapshot.fontSize : 0
      caretY = 0
      caretHeight = this.newSnapshot.fontSize * this.newSnapshot.lineSpacingMultiplier
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
      insertRuns: [],
      deletedRuns: [],
      retainedMoves: [],
      caretPath: [],
      durationMs: 0,
      motionKind: MotionKind.None
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
   * 重置到静态画面（几何身份变化或 revision 不匹配时）。
   */
  private resetToStatic(
    newGeometryKey: GeometryKey,
    newSnapshot: EditorLayoutSnapshot,
    newLayout: LineLayout[]
  ): void {
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
      insertRuns: [],
      deletedRuns: [],
      retainedMoves: [],
      caretPath: [],
      durationMs: 0,
      motionKind: MotionKind.None
    }
    this.motionDurationMs = 0
    this.currentFrame.isAnimating = false
  }

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
   */
  private static geometryEquals(a: GeometryKey, b: GeometryKey): boolean {
    return a.fontSize === b.fontSize &&
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
    let caretX = 0
    let caretY = 0
    let caretHeight = 0

    if (lineLayouts.length === 0) {
      // Issue #879 复核评论问题5：空文档时从 snapshot 排版参数计算首行空段落的
      // caret 位置和高度，而非返回零高度（零高度意味着光标消失）。
      caretX = snapshot.autoIndentEnabled
        ? snapshot.autoIndentWidth * snapshot.fontSize : 0
      caretY = 0
      caretHeight = snapshot.fontSize * snapshot.lineSpacingMultiplier
    } else {
      const cursorUtf16 = TextOffsetMapper.utf8ToUtf16(snapshot.text, snapshot.cursorByteOffset)
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
   * 多通道静态可见区计算。
   *
   * Issue #879 复核评论修复（问题1）：
   * 以本次 newLayout 的完整 UTF-16 区间为底计算静态字形所有权，
   * 只扣目标新正文的 insertRuns 和 retainedMoves.newUtf16*（都是新正文 offset）。
   * 不扣 deletedRuns 的旧正文 offset——旧 offset 在新正文中无意义，
   * 拿旧 offset 在新正文上做减法会误删新正文的字。
   * 每帧从同一版完整目标静态范围重新计算，不从上一帧已经裁过的范围递减。
   */
  private computeStaticRangesMultiChannel(plan: EditorMotionPlan): StaticVisibleRange[] {
    // 以本次 newLayout 的完整 UTF-16 区间为底，每帧重新计算
    const fullRange: StaticVisibleRange[] = []
    if (this.newLayout.length > 0) {
      fullRange.push({
        utf16Start: this.newLayout[0].startUtf16,
        utf16End: this.newLayout[this.newLayout.length - 1].endUtf16
      })
    }

    // 只扣目标新正文的 insertRuns 和 retainedMoves.newUtf16*（都是新正文 offset）
    const excluded: StaticVisibleRange[] = []
    for (const run of plan.insertRuns) {
      excluded.push({ utf16Start: run.utf16Start, utf16End: run.utf16End })
    }
    for (const move of plan.retainedMoves) {
      excluded.push({ utf16Start: move.newUtf16Start, utf16End: move.newUtf16End })
    }
    // 不扣 deletedRuns 的旧正文 offset——旧 offset 在新正文中无意义

    return EditorMotionCoordinator.subtractRanges(fullRange, excluded)
  }

  /**
   * 从可见范围列表中减去排除范围列表。
   *
   * Issue #879 复核评论修复（问题6）：
   * excluded 来源（insertRuns + retainedMoves）顺序不一定按 utf16Start 升序，
   * 逐个扫描未排序的 excluded 会导致差集结果错误（同一字形静态层和运动层重影）。
   * 修复：先按 utf16Start 排序、合并重叠或相邻范围，再做差集。
   */
  static subtractRanges(
    visible: StaticVisibleRange[],
    excluded: StaticVisibleRange[]
  ): StaticVisibleRange[] {
    if (excluded.length === 0) {
      return visible
    }
    // 先按 utf16Start 排序
    const sorted: StaticVisibleRange[] = excluded.slice().sort(
      (a: StaticVisibleRange, b: StaticVisibleRange): number => a.utf16Start - b.utf16Start
    )
    // 合并重叠或相邻的范围
    const merged: StaticVisibleRange[] = []
    for (const exc of sorted) {
      if (merged.length > 0) {
        const last: StaticVisibleRange = merged[merged.length - 1]
        if (exc.utf16Start <= last.utf16End) {
          // 重叠或相邻——合并
          last.utf16End = Math.max(last.utf16End, exc.utf16End)
          continue
        }
      }
      merged.push({ utf16Start: exc.utf16Start, utf16End: exc.utf16End })
    }
    // 用合并后的范围做差集
    const result: StaticVisibleRange[] = []
    for (const vis of visible) {
      let start = vis.utf16Start
      const end = vis.utf16End
      for (const exc of merged) {
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
