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
import { LineLayout, CaretRect } from '../render/editor_render_geometry'
import { TextOffsetMapper } from '../input/TextOffsetMapper'
import { EditorEditResult, DisplayPatch, OffsetMap } from '../../../corebridge/dto/EditorDtos'
import { EditorMotionTransactionComposer, ComposedTransaction } from './EditorMotionTransactionComposer'
import type { GlyphCarryPatch } from './editor_patch_carry'
import { applyPatchesToText } from './editor_patch_carry'
import { GlyphIdentityTable } from './editor_glyph_identity'

/**
 * Issue #879 最新复核评论问题4：prepareCandidate 的返回类型。
 *
 * prepareCandidate 只生成不可变候选（plan + 起始帧 + 候选提交状态），
 * 不修改 Coordinator 的 currentPlan/currentFrame/oldSnapshot/newSnapshot；
 * 候选窗口用 plan/startFrame 做 staging，全部 ready 后再 commitCandidate 一次性替换。
 */
export interface PreparedCandidate {
  /** 候选运动规划 */
  readonly plan: EditorMotionPlan
  /** 候选起始帧（progress=0 的帧，或时长为0时的终态帧） */
  readonly startFrame: EditorVisualFrame
  /** 候选提交状态——commitCandidate 一次性安装（内部数据，UI 不要读） */
  readonly state: MotionCommittedState
}

/**
 * Coordinator 的一次完整“已提交状态”。
 *
 * 把 acceptUpdate 会写入的所有字段打包成一个不可变对象：
 * - 普通路径（acceptUpdate）直接安装；
 * - 两阶段路径（prepareCandidate → commitCandidate）先持有候选状态，
 *   等候选源 ready 后一次性安装，期间旧动画继续使用旧的已提交状态。
 */
export interface MotionCommittedState {
  plan: EditorMotionPlan
  frame: EditorVisualFrame
  oldSnapshot: EditorLayoutSnapshot
  oldLayout: LineLayout[]
  newSnapshot: EditorLayoutSnapshot
  newLayout: LineLayout[]
  geometryKey: GeometryKey
  textRevisionKey: TextRevisionKey
  motionDurationMs: number
  firstCaretRect: CaretRect | null
  glyphIdentities: GlyphIdentityTable | null
}

/**
 * Issue #879 最新复核评论问题3：一次更新的字形身份推进结果。
 *
 * - oldTable：描述旧正文（匹配上一帧窗口用）
 * - newTable：描述新正文（提交后成为当前表）
 *
 * 任一为 null 表示身份不可信，Planner 应对该侧退回内容 hash。
 */
export interface IdentityAdvance {
  oldTable: GlyphIdentityTable | null
  newTable: GlyphIdentityTable | null
}

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
  /** 首行 caret 几何（可选，空正文时由调用方从 EditorPendingLayoutHost 传入） */
  private firstCaretRect: CaretRect | null
  /**
   * Issue #879 最新复核评论问题3：当前已提交正文的稳定字形身份表。
   *
   * 描述 newSnapshot.revision 的正文：每次编辑按 patch 逐笔搬运字符簇身份。
   * Planner 用它给出 InsertRun/DeletedRun/RetainedMove 的 stableGlyphId，
   * 连续编辑时按身份查找上一帧窗口，而不是按「位置 + 内容」。
   */
  private glyphIdentities: GlyphIdentityTable | null

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
    this.firstCaretRect = null
    this.glyphIdentities = null
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
   * Issue #879 最新复核评论问题4：准备候选——只返回不可变计划，不修改任何已提交状态。
   *
   * 两阶段发布期间屏幕仍在显示上一版（displayedVisualFrame），Coordinator 的
   * currentPlan/currentFrame 必须继续描述那份可见画面：
   * - 旧 animator 继续用它采样，不会拿到候选 plan；
   * - 候选被更高 revision 覆盖时直接丢弃 PreparedCandidate 即可，不影响当前显示。
   *
   * 只有 commitCandidate 才把候选状态一次性安装为已提交状态。
   *
   * @param displayedFrame 当前显示帧——候选 plan 以它为起始状态
   */
  prepareCandidate(
    oldSnapshot: EditorLayoutSnapshot,
    oldLayout: LineLayout[],
    newSnapshot: EditorLayoutSnapshot,
    newLayout: LineLayout[],
    editResults: EditorEditResult[],
    contentWidth: number,
    policy: EditorMotionPolicy,
    firstCaretRect: CaretRect | null,
    displayedFrame: EditorVisualFrame
  ): PreparedCandidate {
    const state = this.computeUpdateState(
      oldSnapshot, oldLayout, newSnapshot, newLayout, editResults,
      contentWidth, policy, displayedFrame, firstCaretRect
    )
    return {
      plan: state.plan,
      startFrame: state.frame,
      state: state
    }
  }

  /**
   * Issue #879 最新复核评论问题4：提交候选——一次性安装候选状态。
   *
   * 安装的 currentFrame 就是候选起始帧（frozenDisplayedFrame）：它的 clip/位置
   * 正是 prepareCandidate 那一刻冻结的可见状态，新 animator 从这一帧继续推进，
   * 无需调用方另传一帧——传别的帧只会让 plan 起点与已提交帧不一致。
   *
   * @param prepared prepareCandidate 返回的不可变候选
   * @returns 提交后的当前帧
   */
  commitCandidate(prepared: PreparedCandidate): EditorVisualFrame {
    this.applyCommittedState(prepared.state)
    return this.currentFrame
  }

  /**
   * Issue #879 复核评论修复（问题3）：重定向候选——用最新冻结帧的 clip/position
   * 替换候选 plan 中各 run 的 startClipLeft/Right/startPositionX/Y。
   *
   * 在 executeDisplaySwitch 中（窗口 ready 后才调）：
   * 先 ++motionGeneration + cancel 旧 animator → 冻结当前 displayedVisualFrame →
   * 调 retargetCandidate 重新规划候选 → commitCandidate → 更新主 Text。
   *
   * 旧 animator 被取消时 displayedVisualFrame 停在某一中间进度，其
   * MotionGlyphWindow.currentClipLeft/Right/currentPositionX/Y 反映真实屏幕状态。
   * 候选 plan 的 startClipLeft/Right/startPositionX/Y 是在 prepareCandidate 阶段
   * 用 displayedFrameForCandidate（当时帧）计算的——但实际提交时旧 animator 已推进，
   * 屏幕状态已变。retargetCandidate 用冻结帧的真实值替换候选 plan 的起始值，
   * 确保新动画从真实屏幕状态开始，而非从过时的起始状态跳变。
   *
   * @param prepared prepareCandidate 返回的不可变候选
   * @param latestDisplayedFrame 冻结时刻的 displayedVisualFrame（旧 animator 停止后的真实帧）
   * @returns 重定向后的新候选
   */
  retargetCandidate(prepared: PreparedCandidate, latestDisplayedFrame: EditorVisualFrame): PreparedCandidate {
    // Issue #879 复核评论问题4：构建 glyphId → frozen window 映射，
    // 用于按字形身份列表逐个查找冻结帧中的当前状态。
    // 不再仅用 clusterId（首字符簇身份）匹配——用 glyphIds 交集最大匹配。
    const frozenByGlyphId: Map<string, MotionGlyphWindow> = new Map()
    for (const w of latestDisplayedFrame.motionGlyphWindows) {
      for (const gid of w.glyphIds) {
        frozenByGlyphId.set(gid, w)
      }
    }

    // 深拷贝 plan，替换起始值
    const newInsertRuns: InsertRun[] = []
    for (const run of prepared.plan.insertRuns) {
      // Issue #879 复核评论问题4：用 glyphIds 逐个查，取交集最大的窗口
      const frozen = EditorMotionCoordinator.findBestMatchFrozenWindow(frozenByGlyphId, run.glyphIds)
      newInsertRuns.push({
        utf16Start: run.utf16Start,
        utf16End: run.utf16End,
        sourceKind: run.sourceKind,
        rect: run.rect,
        startClipLeft: frozen !== undefined ? frozen.currentClipLeft : run.startClipLeft,
        startClipRight: frozen !== undefined ? frozen.currentClipRight : run.startClipRight,
        stableGlyphId: run.stableGlyphId,
        glyphIds: run.glyphIds,
        windowId: run.windowId
      })
    }

    const newDeletedRuns: DeletedRun[] = []
    for (const run of prepared.plan.deletedRuns) {
      // Issue #879 复核评论问题4：用 glyphIds 逐个查，取交集最大的窗口
      const frozen = EditorMotionCoordinator.findBestMatchFrozenWindow(frozenByGlyphId, run.glyphIds)
      newDeletedRuns.push({
        utf16Start: run.utf16Start,
        utf16End: run.utf16End,
        sourceKind: run.sourceKind,
        rect: run.rect,
        isForward: run.isForward,
        startClipLeft: frozen !== undefined ? frozen.currentClipLeft : run.startClipLeft,
        startClipRight: frozen !== undefined ? frozen.currentClipRight : run.startClipRight,
        stableGlyphId: run.stableGlyphId,
        glyphIds: run.glyphIds,
        windowId: run.windowId
      })
    }

    const newRetainedMoves: RetainedMove[] = []
    for (const move of prepared.plan.retainedMoves) {
      // Issue #879 复核评论问题4：用 glyphIds 逐个查，取交集最大的窗口
      const frozen = EditorMotionCoordinator.findBestMatchFrozenWindow(frozenByGlyphId, move.glyphIds)
      newRetainedMoves.push({
        oldUtf16Start: move.oldUtf16Start,
        oldUtf16End: move.oldUtf16End,
        newUtf16Start: move.newUtf16Start,
        newUtf16End: move.newUtf16End,
        sourceKind: move.sourceKind,
        oldRect: move.oldRect,
        newRect: move.newRect,
        startPositionX: frozen !== undefined ? frozen.currentPositionX : move.startPositionX,
        startPositionY: frozen !== undefined ? frozen.currentPositionY : move.startPositionY,
        stableGlyphId: move.stableGlyphId,
        glyphIds: move.glyphIds,
        windowId: move.windowId
      })
    }

    const newPlan: EditorMotionPlan = {
      insertRuns: newInsertRuns,
      deletedRuns: newDeletedRuns,
      retainedMoves: newRetainedMoves,
      caretPath: prepared.plan.caretPath,
      durationMs: prepared.plan.durationMs,
      motionKind: prepared.plan.motionKind
    }

    // 用更新后的 plan 和冻结帧构建新的起始帧（progress=0）
    const newStartFrame = EditorMotionCoordinator.buildFrame(
      newPlan,
      prepared.state.oldSnapshot,
      prepared.state.newSnapshot,
      prepared.state.newLayout,
      latestDisplayedFrame,
      0
    )

    // 创建新的 state（复制旧 state，替换 plan 和 frame）
    const newState: MotionCommittedState = {
      plan: newPlan,
      frame: newStartFrame,
      oldSnapshot: prepared.state.oldSnapshot,
      oldLayout: prepared.state.oldLayout,
      newSnapshot: prepared.state.newSnapshot,
      newLayout: prepared.state.newLayout,
      geometryKey: prepared.state.geometryKey,
      textRevisionKey: prepared.state.textRevisionKey,
      motionDurationMs: prepared.state.motionDurationMs,
      firstCaretRect: prepared.state.firstCaretRect,
      glyphIdentities: prepared.state.glyphIdentities
    }

    return {
      plan: newPlan,
      startFrame: newStartFrame,
      state: newState
    }
  }

  /**
   * 接收编辑更新（带事务链），生成新 plan，retarget 到最新 canonical。
   *
   * Issue #879 最新复核评论问题1：连续快速输入时，多笔 editResult 形成事务链。
   * 使用 EditorMotionTransactionComposer 逐笔转换/复合 UTF-8 字符身份映射，
   * 输出起始显示正文→最终正文的统一可见差分和保留字符映射。
   * 不再简单拼接 displayPatches 或把最后一笔 offsetMap 当整条链的 offsetMap——
   * 每一笔 patch 的 UTF-8 坐标系不同，简单拼接必然错位。
   *
   * Issue #879 最新复核评论问题4：本方法只在“提交”时调用（单笔编辑或无需 staging 的场景）；
   * 两阶段发布走 prepareCandidate + commitCandidate。
   *
   * @returns 新的视觉帧（动画起始帧）
   */
  acceptUpdateWithChain(
    oldSnapshot: EditorLayoutSnapshot,
    oldLayout: LineLayout[],
    newSnapshot: EditorLayoutSnapshot,
    newLayout: LineLayout[],
    editResults: EditorEditResult[],
    contentWidth: number,
    policy: EditorMotionPolicy,
    firstCaretRect?: CaretRect | null
  ): EditorVisualFrame {
    const state = this.computeUpdateState(
      oldSnapshot, oldLayout, newSnapshot, newLayout, editResults,
      contentWidth, policy, this.currentFrame, firstCaretRect ?? null
    )
    this.applyCommittedState(state)
    return this.currentFrame
  }

  /**
   * 接收编辑更新，生成新 plan，retarget 到最新 canonical。
   *
   * @param editResult 本次编辑事务结果（null 表示非编辑触发的更新）
   * @returns 新的视觉帧（动画起始帧）
   */
  acceptUpdate(
    oldSnapshot: EditorLayoutSnapshot,
    oldLayout: LineLayout[],
    newSnapshot: EditorLayoutSnapshot,
    newLayout: LineLayout[],
    editResult: EditorEditResult | null,
    contentWidth: number,
    policy: EditorMotionPolicy,
    firstCaretRect?: CaretRect | null
  ): EditorVisualFrame {
    const chain: EditorEditResult[] = []
    if (editResult !== null) {
      chain.push(editResult)
    }
    const state = this.computeUpdateState(
      oldSnapshot, oldLayout, newSnapshot, newLayout, chain,
      contentWidth, policy, this.currentFrame, firstCaretRect ?? null
    )
    this.applyCommittedState(state)
    return this.currentFrame
  }

  /**
   * 计算一次更新后的完整已提交状态——不修改任何字段。
   *
   * 两阶段发布（prepareCandidate）与直接提交（acceptUpdate）共用这条路径，
   * 因此候选 plan 与直接提交的 plan 使用完全相同的规则，不会出现两套行为。
   *
   * @param editResults 事务链（空数组表示非编辑触发的更新）
   * @param startFrame 起始可见帧——plan 的起始状态
   * @returns 该次更新的完整状态
   */
  private computeUpdateState(
    oldSnapshot: EditorLayoutSnapshot,
    oldLayout: LineLayout[],
    newSnapshot: EditorLayoutSnapshot,
    newLayout: LineLayout[],
    editResults: EditorEditResult[],
    contentWidth: number,
    policy: EditorMotionPolicy,
    startFrame: EditorVisualFrame,
    firstCaretRect: CaretRect | null
  ): MotionCommittedState {
    const newGeometryKey: GeometryKey = {
      fontSize: newSnapshot.fontSize,
      lineSpacing: newSnapshot.lineSpacingMultiplier,
      contentWidth: contentWidth
    }
    const newTextRevisionKey: TextRevisionKey = {
      revision: newSnapshot.revision,
      generation: newSnapshot.generation,
      compositionGeneration: newSnapshot.compositionGeneration
    }

    // 身份表始终尽量向前推进（即使本次不做动画）——下一次动画要靠它认字。
    const editResult = EditorMotionCoordinator.composeChain(editResults, oldSnapshot, newSnapshot)
    const identities = EditorMotionCoordinator.advanceIdentities(
      this.glyphIdentities, editResults.length === 0, oldSnapshot, newSnapshot, editResult
    )

    // 几何身份变化（字号变更/宽度变化）或 revision 对不上 → 直接显示静态画面
    let reset = false
    if (!EditorMotionCoordinator.geometryEquals(this.geometryKey, newGeometryKey)) {
      reset = true
    }
    if (editResults.length > 0 && editResult === null) {
      reset = true
    }
    if (editResult !== null &&
        (editResult.baseRevision !== oldSnapshot.revision ||
         editResult.newRevision !== newSnapshot.revision)) {
      reset = true
    }

    if (reset) {
      return {
        plan: EditorMotionCoordinator.emptyPlan(),
        frame: EditorMotionCoordinator.computeStaticFrame(newSnapshot, newLayout, firstCaretRect),
        oldSnapshot: newSnapshot,
        oldLayout: newLayout,
        newSnapshot: newSnapshot,
        newLayout: newLayout,
        geometryKey: newGeometryKey,
        textRevisionKey: newTextRevisionKey,
        motionDurationMs: 0,
        firstCaretRect: firstCaretRect,
        glyphIdentities: identities.newTable
      }
    }

    // 用 Planner 生成新 plan（传入 oldSnapshot + oldLayout + newSnapshot + newLayout）
    const plan = EditorMotionPlanner.plan(
      oldSnapshot, oldLayout, newSnapshot, newLayout, editResult, startFrame,
      identities.oldTable, identities.newTable
    )

    // 确定动画时长
    let durationMs = 0
    if (plan.motionKind === MotionKind.None || plan.motionKind === MotionKind.CaretMove) {
      // 纯光标移动用 cursorDurationMs
      durationMs = policy.shouldAnimateCaret() ? policy.cursorDurationMs : 0
    } else {
      // 正文增删用 typingDurationMs
      durationMs = policy.shouldAnimateText() ? policy.typingDurationMs : 0
    }
    plan.durationMs = durationMs

    const frame = durationMs > 0
      ? EditorMotionCoordinator.buildFrame(
          plan, oldSnapshot, newSnapshot, newLayout, startFrame, 0
        )
      : EditorMotionCoordinator.computeStaticFrame(newSnapshot, newLayout, firstCaretRect)

    return {
      plan: plan,
      frame: frame,
      oldSnapshot: oldSnapshot,
      oldLayout: oldLayout,
      newSnapshot: newSnapshot,
      newLayout: newLayout,
      geometryKey: newGeometryKey,
      textRevisionKey: newTextRevisionKey,
      motionDurationMs: durationMs,
      firstCaretRect: firstCaretRect,
      glyphIdentities: identities.newTable
    }
  }

  /**
   * 安装已提交状态。
   */
  private applyCommittedState(state: MotionCommittedState): void {
    this.currentPlan = state.plan
    this.currentFrame = state.frame
    this.oldSnapshot = state.oldSnapshot
    this.oldLayout = state.oldLayout
    this.newSnapshot = state.newSnapshot
    this.newLayout = state.newLayout
    this.geometryKey = state.geometryKey
    this.textRevisionKey = state.textRevisionKey
    this.motionDurationMs = state.motionDurationMs
    this.firstCaretRect = state.firstCaretRect
    this.glyphIdentities = state.glyphIdentities
  }

  /**
   * 空规划。
   */
  private static emptyPlan(): EditorMotionPlan {
    return {
      insertRuns: [],
      deletedRuns: [],
      retainedMoves: [],
      caretPath: [],
      durationMs: 0,
      motionKind: MotionKind.None
    }
  }

  /**
   * 把事务链复合成一个统一坐标系的 editResult。
   *
   * 单笔/空链直接返回原值；多笔走 EditorMotionTransactionComposer，
   * 校验失败返回 null（调用方按静态重置处理，不得发布缺片的动画）。
   */
  private static composeChain(
    editResults: EditorEditResult[],
    oldSnapshot: EditorLayoutSnapshot,
    newSnapshot: EditorLayoutSnapshot
  ): EditorEditResult | null {
    if (editResults.length === 0) {
      return null
    }
    if (editResults.length === 1) {
      return editResults[0]
    }
    const composed = EditorMotionTransactionComposer.compose(
      editResults,
      oldSnapshot.text,
      newSnapshot.text,
      oldSnapshot.revision,
      newSnapshot.revision
    )
    if (!composed.isValid) {
      return null
    }
    const lastEr = editResults[editResults.length - 1]
    return {
      outcome: lastEr.outcome,
      transactionId: lastEr.transactionId,
      baseRevision: oldSnapshot.revision,
      newRevision: newSnapshot.revision,
      displayPatches: composed.displayPatches,
      oldSelectionAnchor: editResults[0].oldSelectionAnchor,
      oldSelectionHead: editResults[0].oldSelectionHead,
      newSelectionAnchor: lastEr.newSelectionAnchor,
      newSelectionHead: lastEr.newSelectionHead,
      cause: lastEr.cause,
      operationKind: lastEr.operationKind,
      offsetMap: composed.offsetMap,
      contentDelta: lastEr.contentDelta
    }
  }

  /**
   * Issue #879 最新复核评论问题3：把字符簇身份表向前搬运到 newSnapshot。
   *
   * - 旧表与 oldSnapshot 不匹配时（首次、会话切换、几何重置）用 oldSnapshot.text 重建；
   * - 有真实编辑时按该笔 patch 逐笔搬运；搬运失败（越界/非字符边界/文本不符）时
   *   退化为 null——宁可让本次匹配退回内容 hash，也不能发布错身份。
   *
   * @param includeOld 仅当没有编辑时（纯光标/样式更新）才需要旧表用于匹配
   * @returns oldTable（匹配上一帧用）与 newTable（提交后成为当前表）
   */
  private static advanceIdentities(
    current: GlyphIdentityTable | null,
    includeOld: boolean,
    oldSnapshot: EditorLayoutSnapshot,
    newSnapshot: EditorLayoutSnapshot,
    editResult: EditorEditResult | null
  ): IdentityAdvance {
    let oldTable: GlyphIdentityTable | null = null
    if (includeOld) {
      if (current !== null && current.revision === newSnapshot.revision && current.text === newSnapshot.text) {
        // 没有编辑：旧表已经描述新正文（同一 revision）——直接复用，身份不变
        return { oldTable: current, newTable: current }
      }
      oldTable = current !== null && current.revision === oldSnapshot.revision &&
        current.text === oldSnapshot.text
        ? current
        : GlyphIdentityTable.create(oldSnapshot.text, oldSnapshot.revision)
      // 非编辑更新：正文没变就沿用同一份身份，只把 revision 标签推进——
      // 否则下一次编辑会因 revision 对不上而重建整张表，丢掉全部跨修订身份。
      return {
        oldTable: oldTable,
        newTable: newSnapshot.text === oldSnapshot.text
          ? oldTable.withRevision(newSnapshot.revision)
          : GlyphIdentityTable.create(newSnapshot.text, newSnapshot.revision)
      }
    }

    if (current !== null && current.revision === oldSnapshot.revision && current.text === oldSnapshot.text) {
      oldTable = current
    } else if (current !== null && current.revision === newSnapshot.revision && current.text === newSnapshot.text) {
      // 已提交到新正文（重复回调）——旧侧无法匹配，但仍保留表
      return { oldTable: null, newTable: current }
    } else {
      oldTable = GlyphIdentityTable.create(oldSnapshot.text, oldSnapshot.revision)
    }

    if (editResult === null) {
      return {
        oldTable: oldTable,
        newTable: newSnapshot.text === oldSnapshot.text
          ? oldTable.withRevision(newSnapshot.revision)
          : GlyphIdentityTable.create(newSnapshot.text, newSnapshot.revision)
      }
    }

    const patches = EditorMotionCoordinator.collectCarryPatches(editResult)
    const nextText = applyPatchesToText(oldSnapshot.text, patches)
    if (nextText === null || nextText !== newSnapshot.text) {
      // 搬运结果与 Core 给出的新正文不一致——身份不可信，退回内容 hash
      return { oldTable: oldTable, newTable: null }
    }
    const newTable = oldTable.applyPatches(patches, nextText, newSnapshot.revision)
    if (newTable === null) {
      return { oldTable: oldTable, newTable: null }
    }
    return { oldTable: oldTable, newTable: newTable }
  }

  /**
   * 把 DisplayPatch 投影成不依赖 DTO 的纯数据（供身份表 / 位置映射使用）。
   */
  static collectCarryPatches(editResult: EditorEditResult): GlyphCarryPatch[] {
    const patches: GlyphCarryPatch[] = []
    for (const patch of editResult.displayPatches) {
      patches.push({
        replaceStartByte: patch.replaceByteStart,
        replaceEndByte: patch.replaceByteEndExclusive,
        insertedText: patch.insertedText
      })
    }
    return patches
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
    return EditorMotionCoordinator.buildFrame(
      this.currentPlan,
      this.oldSnapshot,
      this.newSnapshot,
      this.newLayout,
      this.currentFrame,
      progress
    )
  }

  /**
   * 从 plan 和 progress 计算视觉帧——同一 progress 同时采样所有通道。
   *
   * Issue #879 最新复核评论问题4：显式传入快照/布局/起始帧，候选 plan 也能在不触碰
   * 已提交状态的前提下采样（prepareCandidate 用它生成候选起始帧）。
   *
   * 同一 progress 同时采样 insertRuns（吐字）、deletedRuns（吞字）、
   * retainedMoves（保留字移动）、caretPath（光标），静态可见区完整扣除所有
   * 被 motion 接管的目标 glyph 身份（insertRuns + retainedMoves 的新位置 +
   * deletedRuns 的旧位置）。
   *
   * Issue #879 最新复核评论问题3：clusterId/renderNodeKey 直接取 plan 计算出的
   * stableGlyphId（跨修订稳定的字符身份），不再用 utf16 区间 + 文本 hash 现场重算。
   */
  private static buildFrame(
    plan: EditorMotionPlan,
    oldSnapshot: EditorLayoutSnapshot | null,
    newSnapshot: EditorLayoutSnapshot | null,
    newLayout: LineLayout[],
    caretBase: EditorVisualFrame,
    progress: number
  ): EditorVisualFrame {
    const newRevision = newSnapshot?.revision ?? 0
    const newText = newSnapshot?.text ?? ''
    const oldRevision = oldSnapshot?.revision ?? 0
    const oldText = oldSnapshot?.text ?? ''

    const p = EditorMotionCoordinator.easing(progress)

    // 无运动 → 返回当前帧
    if (plan.motionKind === MotionKind.None &&
        plan.insertRuns.length === 0 &&
        plan.deletedRuns.length === 0 &&
        plan.retainedMoves.length === 0 &&
        plan.caretPath.length === 0) {
      return caretBase
    }

    // 同一 progress 同时采样所有通道
    const motionGlyphWindows: MotionGlyphWindow[] = []

    // 1. 吐字通道：新字源完整排版，裁切窗口从起始状态扩展到目标状态
    //    Issue #879 最新复核评论修复（问题2）：
    //    从 startClipLeft/Right 插值到 rect.x/rect.x+rect.width（而非从 x0→x1）。
    //    连续快速编辑时，起始状态来自上一帧的 MotionGlyphWindow.currentClipLeft/Right。
    for (const run of plan.insertRuns) {
      const targetLeft = run.rect.x
      const targetRight = run.rect.x + run.rect.width
      const currentLeft = run.startClipLeft + (targetLeft - run.startClipLeft) * p
      const currentRight = run.startClipRight + (targetRight - run.startClipRight) * p
      motionGlyphWindows.push({
        sourceKind: 'new',
        utf16Start: run.utf16Start,
        utf16End: run.utf16End,
        clipLeft: currentLeft,
        clipRight: currentRight,
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
        // Issue #879 复核评论问题4：clusterId 改为 windowId（独立窗口身份），
        // glyphIds 数组也传入——不再拿窗口第一个字代替整窗口身份
        clusterId: run.windowId,
        glyphIds: run.glyphIds,
        windowId: run.windowId,
        renderNodeKey: `new-${newRevision}-${run.stableGlyphId}`,
        currentClipLeft: currentLeft,
        currentClipRight: currentRight,
        currentPositionX: run.rect.x,
        currentPositionY: run.rect.y
      })
    }

    // 2. 吞字通道：旧字源留在动画层，裁切窗口从起始状态缩到目标状态
    //    Issue #879 最新复核评论修复（问题2）：
    //    从 startClipLeft/Right 插值到目标 clip 状态（而非从 x0→x1）。
    //    Forward Delete：目标 clip 状态是 [caret, caret]（零宽度）
    //    Backspace：目标 clip 状态是 [rect.x, rect.x]（零宽度）
    for (const run of plan.deletedRuns) {
      if (run.isForward) {
        // Forward Delete：目标裁切窗口是 [caret, caret]（零宽度）
        const caretX = caretBase.caretX
        const targetLeft = caretX
        const targetRight = caretX
        const currentLeft = run.startClipLeft + (targetLeft - run.startClipLeft) * p
        const currentRight = run.startClipRight + (targetRight - run.startClipRight) * p
        motionGlyphWindows.push({
          sourceKind: 'old',
          utf16Start: run.utf16Start,
          utf16End: run.utf16End,
          clipLeft: currentLeft,
          clipRight: currentRight,
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
          // Issue #879 复核评论问题4：clusterId 改为 windowId，glyphIds 数组也传入
          clusterId: run.windowId,
          glyphIds: run.glyphIds,
          windowId: run.windowId,
          renderNodeKey: `old-${oldRevision}-${run.stableGlyphId}`,
          currentClipLeft: currentLeft,
          currentClipRight: currentRight,
          currentPositionX: run.rect.x,
          currentPositionY: run.rect.y
        })
      } else {
        // Backspace：目标裁切窗口是 [rect.x, rect.x]（零宽度）
        const targetLeft = run.rect.x
        const targetRight = run.rect.x
        const currentLeft = run.startClipLeft + (targetLeft - run.startClipLeft) * p
        const currentRight = run.startClipRight + (targetRight - run.startClipRight) * p
        motionGlyphWindows.push({
          sourceKind: 'old',
          utf16Start: run.utf16Start,
          utf16End: run.utf16End,
          clipLeft: currentLeft,
          clipRight: currentRight,
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
          // Issue #879 复核评论问题4：clusterId 改为 windowId，glyphIds 数组也传入
          clusterId: run.windowId,
          glyphIds: run.glyphIds,
          windowId: run.windowId,
          renderNodeKey: `old-${oldRevision}-${run.stableGlyphId}`,
          currentClipLeft: currentLeft,
          currentClipRight: currentRight,
          currentPositionX: run.rect.x,
          currentPositionY: run.rect.y
        })
      }
    }

    // 3. 保留字移动通道：旧字源绘制后整体偏移到当前插值位置
    //    Issue #879 最新复核评论修复（问题2）：
    //    从 startPositionX/Y 插值到 newRect.x/y（而非从 oldRect→newRect）。
    //    连续快速编辑时，起始状态来自上一帧的 MotionGlyphWindow.currentPositionX/Y。
    for (const move of plan.retainedMoves) {
      const currentX = move.startPositionX + (move.newRect.x - move.startPositionX) * p
      const currentY = move.startPositionY + (move.newRect.y - move.startPositionY) * p
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
        // Issue #879 复核评论问题4：clusterId 改为 windowId，glyphIds 数组也传入
        clusterId: move.windowId,
        glyphIds: move.glyphIds,
        windowId: move.windowId,
        renderNodeKey: `old-${oldRevision}-${move.stableGlyphId}`,
        currentClipLeft: currentX,
        currentClipRight: currentX + move.oldRect.width,
        currentPositionX: currentX,
        currentPositionY: currentY
      })
    }

    // 4. 光标位置：从 caretPath 插值
    let caretX = caretBase.caretX
    let caretY = caretBase.caretY
    let caretHeight = caretBase.caretHeight
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
      // Issue #879 最新复核评论修复（问题2）：从起始 clip 状态插值到目标，
      // 而非从 rect.x→rect.x+rect.width。
      const first = plan.insertRuns[0]
      const targetCaretX = first.rect.x + first.rect.width
      const startCaretX = first.startClipRight
      caretX = startCaretX + (targetCaretX - startCaretX) * p
      caretY = first.rect.y
      caretHeight = first.rect.height
    } else if (plan.deletedRuns.length > 0) {
      // 吞字：光标画在第一个 deletedRun 的裁切前沿
      // Issue #879 最新复核评论修复（问题2）：从起始 clip 状态插值到目标，
      // 而非从 rect.x+rect.width→rect.x。
      const first = plan.deletedRuns[0]
      if (first.isForward) {
        // Forward Delete：光标不动
        // caretX/caretY 保持 currentFrame
      } else {
        const targetCaretX = first.rect.x
        const startCaretX = first.startClipRight
        caretX = startCaretX + (targetCaretX - startCaretX) * p
        caretY = first.rect.y
        caretHeight = first.rect.height
      }
    }

    // 5. 静态可见区：完整扣除所有被 motion 接管的目标 glyph 身份
    const staticVisibleRanges = EditorMotionCoordinator.computeStaticRangesMultiChannel(plan, newLayout)

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
   * Issue #879 复核评论问题4：用 glyphIds 数组逐个查 frozenByGlyphId，
   * 取交集最大的窗口作为匹配——不再仅靠 clusterId（首字符簇身份）匹配。
   */
  private static findBestMatchFrozenWindow(
    frozenByGlyphId: Map<string, MotionGlyphWindow>,
    glyphIds: string[]
  ): MotionGlyphWindow | undefined {
    if (glyphIds.length === 0) {
      return undefined
    }
    const windowHitCount: Map<MotionGlyphWindow, number> = new Map()
    for (const gid of glyphIds) {
      const w = frozenByGlyphId.get(gid)
      if (w !== undefined) {
        windowHitCount.set(w, (windowHitCount.get(w) ?? 0) + 1)
      }
    }
    let bestWindow: MotionGlyphWindow | undefined = undefined
    let bestCount = 0
    windowHitCount.forEach((count: number, w: MotionGlyphWindow): void => {
      if (count > bestCount) {
        bestWindow = w
        bestCount = count
      }
    })
    return bestWindow
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
      // Issue #879 最新复核评论修复（问题5）：空文档时优先使用 firstCaretRect
      // （由调用方从 EditorPendingLayoutHost 传入），fallback 才用估算。
      if (this.firstCaretRect !== null) {
        caretX = this.firstCaretRect.x
        caretY = this.firstCaretRect.y
        caretHeight = this.firstCaretRect.height
      } else {
        caretX = this.newSnapshot.autoIndentEnabled
          ? this.newSnapshot.autoIndentWidth * this.newSnapshot.fontSize : 0
        caretY = 0
        caretHeight = this.newSnapshot.fontSize * this.newSnapshot.lineSpacingMultiplier
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
   */
  private static geometryEquals(a: GeometryKey, b: GeometryKey): boolean {
    return a.fontSize === b.fontSize &&
           a.lineSpacing === b.lineSpacing &&
           a.contentWidth === b.contentWidth
  }

  /**
   * 计算静态帧（无动画）。
   *
   * Issue #879 最新复核评论修复（问题5）：空正文时优先使用 firstCaretRect
   * （由调用方从 EditorPendingLayoutHost 传入），fallback 才用估算。
   */
  private static computeStaticFrame(
    snapshot: EditorLayoutSnapshot,
    lineLayouts: LineLayout[],
    firstCaretRect?: CaretRect | null
  ): EditorVisualFrame {
    let caretX = 0
    let caretY = 0
    let caretHeight = 0

    if (lineLayouts.length === 0) {
      // Issue #879 最新复核评论修复（问题5）：空文档时优先使用 firstCaretRect
      // （由调用方从 EditorPendingLayoutHost 传入），fallback 才用估算。
      if (firstCaretRect !== null && firstCaretRect !== undefined) {
        caretX = firstCaretRect.x
        caretY = firstCaretRect.y
        caretHeight = firstCaretRect.height
      } else {
        caretX = snapshot.autoIndentEnabled
          ? snapshot.autoIndentWidth * snapshot.fontSize : 0
        caretY = 0
        caretHeight = snapshot.fontSize * snapshot.lineSpacingMultiplier
      }
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
  private static computeStaticRangesMultiChannel(
    plan: EditorMotionPlan,
    newLayout: LineLayout[]
  ): StaticVisibleRange[] {
    // 以本次 newLayout 的完整 UTF-16 区间为底，每帧重新计算
    const fullRange: StaticVisibleRange[] = []
    if (newLayout.length > 0) {
      fullRange.push({
        utf16Start: newLayout[0].startUtf16,
        utf16End: newLayout[newLayout.length - 1].endUtf16
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
