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

import { EditorVisualFrame, MotionKind, MotionGlyphWindow, MotionSegment, StaticVisibleRange, InsertRun, DeletedRun, RetainedMove, GlyphRect, CarryOverGhostRun } from './EditorVisualFrame'
import { EditorMotionPolicy } from './EditorMotionPolicy'
import { EditorMotionPlan, EditorMotionPlanner } from './EditorMotionPlanner'
import { EditorLayoutSnapshot } from '../render/EditorLayoutSnapshot'
import { LineLayout, CaretRect } from '../render/editor_render_geometry'
import { TextOffsetMapper } from '../input/TextOffsetMapper'
import { EditorEditResult, DisplayPatch, OffsetMap } from '../../../corebridge/dto/EditorDtos'
import { EditorMotionTransactionComposer, ComposedTransaction } from './EditorMotionTransactionComposer'
import type { GlyphCarryPatch } from './editor_patch_carry'
import { applyPatchesToText } from './editor_patch_carry'
import { GlyphIdentityTable, WindowInstanceIdAllocator, clusterBoundaries } from './editor_glyph_identity'
import {
  DisplayedContext, FrozenWindowSpan, RunGeometry, RunStartState, RunStartPiece,
  deletedRunStartState, insertRunStartState, retainedMoveStartState
} from './editor_commit_start'

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
 * Issue #879 复核评论6075662695问题3：提交瞬间的「在屏」上下文。
 *
 * retargetCandidate 用它按字形重算起始 clip/position——
 * 必须是**提交那一刻**真实显示的正文、布局和身份表，
 * 而不是 prepareCandidate 时刻的快照（候选等待期间旧动画可能已经推进/结束）。
 */
export interface DisplayedSnapshotContext {
  /** 提交瞬间正在显示的正文快照（链基准 revision） */
  readonly snapshot: EditorLayoutSnapshot
  /** 提交瞬间正在显示的行布局 */
  readonly layout: LineLayout[]
  /** 描述在屏正文的字形身份表；null 表示身份不可信 */
  readonly identities: GlyphIdentityTable | null
}

/**
 * Issue #879 复核评论6078682695 问题2：不可变源排版记录。
 *
 * 按 sourceRevision 标识，供冻结窗口查它自己的布局。
 * 不按 sourceKind 猜 displayed/newSnapshot——跨修订残影时二者都不对。
 */
export interface SourceLayoutRecord {
  /** 该记录对应的正文 revision */
  readonly revision: number
  /** 该 revision 的完整正文 */
  readonly text: string
  /** 该 revision 的行布局 */
  readonly lineLayouts: LineLayout[]
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
  /**
   * Issue #879 复核评论问题5：Core 编辑会话 ID。
   * 用于 sourceLayoutRecords 的复合键，防止跨会话 revision 重复时取到旧布局。
   */
  sessionId: number
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
   * Planner 用它给出 InsertRun/DeletedRun/RetainedMove 的 glyphIds（字形单元身份）
   * 与 windowId（窗口身份），连续编辑时按幸存字形身份交集查找上一帧窗口，
   * 而不是按「位置 + 内容」。
   */
  private glyphIdentities: GlyphIdentityTable | null
  /**
   * Issue #879 复核评论6077187962 问题4：窗口实例 ID 分配器。
   *
   * 为 ArkUI 节点分配有限长度的 `win-<number>` 格式 ID，
   * 替代随 glyphIds 数量线性增长的 windowId 作为渲染节点 ID。
   * 完整身份序列（windowIdForGlyphIds() 的结果）留在算法内部用于匹配/查找。
   */
  private windowIdAllocator: WindowInstanceIdAllocator
  /**
   * Issue #879 复核评论6078682695 问题2：按 sourceRevision 标识的不可变源排版记录缓存。
   *
   * 每个冻结窗口按 w.sourceRevision 查它自己的布局，不能按 sourceKind 猜 displayed/newSnapshot。
   * 跨修订残影时（如 rev10 的旧窗口在 rev12 仍在消退），通过此缓存获取 rev10 的源排版。
   *
   * Issue #879 复核评论问题5：缓存键改为 sessionId+revision（复合键），
   * 防止跨会话 revision 重复时取到上一个作品的旧布局。
   */
  private sourceLayoutRecords: Map<string, SourceLayoutRecord>
  /**
   * Issue #879 复核评论问题5：当前 Core 编辑会话 ID。
   *
   * 用于 sourceLayoutRecords 的复合键（sessionId+revision），
   * 防止跨会话 revision 重复时取到上一个作品的旧布局。
   */
  private currentSessionId: number = 0

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
    this.windowIdAllocator = new WindowInstanceIdAllocator()
    this.sourceLayoutRecords = new Map()
    this.currentPlan = {
      insertRuns: [],
      deletedRuns: [],
      retainedMoves: [],
      caretPath: [],
      durationMs: 0,
      motionKind: MotionKind.None,
      carryOverGhostRuns: []
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
    displayedFrame: EditorVisualFrame,
    sessionId: number
  ): PreparedCandidate {
    const state = this.computeUpdateState(
      oldSnapshot, oldLayout, newSnapshot, newLayout, editResults,
      contentWidth, policy, displayedFrame, firstCaretRect,
      sessionId
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
   * Issue #879 复核评论6075662695问题3：重定向候选——按**提交瞬间**的真实在屏状态
   * 重算起始 clip / position / 光标起点，替换候选 plan 里过时的起始状态。
   *
   * 在 executeDisplaySwitch 中（窗口 ready 后才调）：
   * 先 ++motionGeneration + cancel 旧 animator → 冻结当前 displayedVisualFrame →
   * 调 retargetCandidate 重新规划候选 → commitCandidate → 更新主 Text。
   *
   * 起点不是「抄旧窗口的绝对 clip/position」，而是按字形逐个判定可见性后
   * 换算到 run 自己的布局坐标（见 editor_commit_start）：
   * - 被冻结窗口覆盖的字形：可见区间 = 字形在屏区间 ∩ 窗口 clip；
   * - 没被任何窗口覆盖的字形：displayed 主文本静态绘制 → 完整可见
   *   （旧动画自然结束后没有运动窗口，这一条给出真正的静态完整状态，不会倒跳）；
   * - 保留字平移：起点按可见字形交集做局部校正，让第一个仍可见的字形停在原处；
   * - 光标轨迹首段起点同样以冻结帧为准。
   *
   * @param prepared prepareCandidate 返回的不可变候选
   * @param latestDisplayedFrame 冻结时刻的 displayedVisualFrame（旧 animator 停止后的真实帧）
   * @param displayed 提交瞬间正在显示的正文/布局/身份表；null 时保留 plan 起点
   * @returns 重定向后的新候选
   */
  retargetCandidate(
    prepared: PreparedCandidate,
    latestDisplayedFrame: EditorVisualFrame,
    displayed: DisplayedSnapshotContext | null
  ): PreparedCandidate {
    if (prepared.plan.durationMs <= 0) {
      // 无动画（动效关闭 / 时长为 0）：候选起始帧已经是静态终态帧。
      // 不能再用 buildFrame(progress=0) 重建一个「动画中」的裁切帧——
      // 没有 animator 会推进它，主 Text 会停在半裁切状态。
      return prepared
    }
    // Issue #879 复核评论6075662695问题3：起始状态改为**按字形重算**——
    // 由 editor_commit_start 逐个 glyphId 判断它此刻在屏可见不可见、在哪里，
    // 再把可见段换算成 run 自己布局里的 startClip/startPosition。
    // 不再「找到交集最多的旧窗口 → 抄它的绝对 clip/position」，
    // 因此旧窗口被折行拆段、目标窗口更短、或旧动画已结束（无运动窗口）都不会多露字/倒跳。
    //
    // 上下文用提交瞬间的真实 displayed 正文 + 布局 + 身份表；
    // 身份表缺失或 revision 对不上（链基准已移动）时不猜几何，保留 plan 里的起点。
    const context: DisplayedContext | null = this.commitDisplayedContext(
      prepared, latestDisplayedFrame, displayed
    )

    const newInsertRuns: InsertRun[] = []
    for (const run of prepared.plan.insertRuns) {
      const pieces: RunStartPiece[] = context === null
        ? [{
            firstIndex: 0,
            lastIndex: run.glyphIds.length - 1,
            startClipLeft: run.startClipLeft,
            startClipRight: run.startClipRight,
            startPositionX: run.rect.x,
            startPositionY: run.rect.y,
            glyphIds: [...run.glyphIds]
          }]
        : insertRunStartState(
            EditorMotionCoordinator.runGeometry(
              run.glyphIds, prepared.state.newSnapshot.text, run.utf16Start, run.utf16End, run.rect,
              prepared.state.newLayout
            ),
            context
          )
      // Issue #879 复核评论6078682695 问题1：保留兼容的 startClipLeft/Right（取 pieces 的并集），
      // 同时把 pieces 传给 run，buildFrame 按 piece 分别采样。
      const compatLeft = pieces.length > 0 ? pieces[0].startClipLeft : run.startClipLeft
      const compatRight = pieces.length > 0 ? pieces[pieces.length - 1].startClipRight : run.startClipRight
      newInsertRuns.push({
        utf16Start: run.utf16Start,
        utf16End: run.utf16End,
        sourceKind: run.sourceKind,
        rect: run.rect,
        startClipLeft: compatLeft,
        startClipRight: compatRight,
        glyphIds: run.glyphIds,
        windowId: run.windowId,
        pieces: pieces
      })
    }

    const newDeletedRuns: DeletedRun[] = []
    for (const run of prepared.plan.deletedRuns) {
      // 已经全被吞掉（起点＝终点）时塌到该 run 的目标边缘：
      // Forward Delete 塌到光标，Backspace 塌到 run 左边界——与 buildFrame 的目标一致。
      const collapseX = run.isForward
        ? latestDisplayedFrame.caretX
        : run.rect.x
      const pieces: RunStartPiece[] = context === null
        ? [{
            firstIndex: 0,
            lastIndex: run.glyphIds.length - 1,
            startClipLeft: run.startClipLeft,
            startClipRight: run.startClipRight,
            startPositionX: run.rect.x,
            startPositionY: run.rect.y,
            glyphIds: [...run.glyphIds]
          }]
        : deletedRunStartState(
            EditorMotionCoordinator.runGeometry(
              run.glyphIds, prepared.state.oldSnapshot.text, run.utf16Start, run.utf16End, run.rect,
              prepared.state.oldLayout
            ),
            context,
            collapseX
          )
      const compatLeft = pieces.length > 0 ? pieces[0].startClipLeft : run.startClipLeft
      const compatRight = pieces.length > 0 ? pieces[pieces.length - 1].startClipRight : run.startClipRight
      newDeletedRuns.push({
        utf16Start: run.utf16Start,
        utf16End: run.utf16End,
        sourceKind: run.sourceKind,
        rect: run.rect,
        isForward: run.isForward,
        startClipLeft: compatLeft,
        startClipRight: compatRight,
        glyphIds: run.glyphIds,
        windowId: run.windowId,
        pieces: pieces
      })
    }

    const newRetainedMoves: RetainedMove[] = []
    for (const move of prepared.plan.retainedMoves) {
      // 起点按字形交集校正：让第一个仍可见的字形停在它此刻在屏的位置上。
      // 一个共享字形都不在屏上（旧帧已静态化且全部消失）→ 保留 run 自己的位置。
      const pieces: RunStartPiece[] = context === null
        ? [{
            firstIndex: 0,
            lastIndex: move.glyphIds.length - 1,
            startClipLeft: move.oldRect.x,
            startClipRight: move.oldRect.x + move.oldRect.width,
            startPositionX: move.startPositionX,
            startPositionY: move.startPositionY,
            glyphIds: [...move.glyphIds]
          }]
        : retainedMoveStartState(
            EditorMotionCoordinator.runGeometry(
              move.glyphIds, prepared.state.oldSnapshot.text, move.oldUtf16Start, move.oldUtf16End,
              move.oldRect, prepared.state.oldLayout
            ),
            context,
            {
              startClipLeft: move.oldRect.x,
              startClipRight: move.oldRect.x + move.oldRect.width,
              startPositionX: move.oldRect.x,
              startPositionY: move.oldRect.y
            }
          )
      const firstPiece = pieces[0]
      newRetainedMoves.push({
        oldUtf16Start: move.oldUtf16Start,
        oldUtf16End: move.oldUtf16End,
        newUtf16Start: move.newUtf16Start,
        newUtf16End: move.newUtf16End,
        sourceKind: move.sourceKind,
        oldRect: move.oldRect,
        newRect: move.newRect,
        startPositionX: firstPiece.startPositionX,
        startPositionY: firstPiece.startPositionY,
        glyphIds: move.glyphIds,
        windowId: move.windowId,
        pieces: pieces
      })
    }

    // Issue #879 复核评论问题3：光标轨迹起点同样以冻结帧为准。
    // plan.caretPath 首段起点来自 prepareCandidate 时刻的 lastVisibleFrame.caretX/Y，
    // 但候选等待期间旧 animator 仍在推进，提交瞬间可见光标已前移——
    // 不重定向起点，新动画会从旧起点重跑，出现倒跳。
    const retargetedCaretPath: MotionSegment[] = []
    for (let i = 0; i < prepared.plan.caretPath.length; i++) {
      const seg = prepared.plan.caretPath[i]
      if (i === 0) {
        retargetedCaretPath.push({
          startCaretX: latestDisplayedFrame.caretX,
          startCaretY: latestDisplayedFrame.caretY,
          endCaretX: seg.endCaretX,
          endCaretY: seg.endCaretY
        })
      } else {
        retargetedCaretPath.push(seg)
      }
    }

    // Issue #879 复核评论6078682695 问题2：识别 carryOver ghost 残影。
    // 上一帧仍有非零 clip、且尚未归还静态层的旧窗口作为残影继承到候选 plan。
    // buildFrame 每帧同时采样本轮新 run 与 carryOver ghost，
    // 保持其旧源 TextController/StyledString 有效，直到残影真正隐藏才释放。
    //
    // Issue #879 问题1修复：planGlyphIds 来自本轮新 plan 的 run 集合，
    // 而不是冻结帧自身的窗口 glyphIds——否则 allGhost 永远为 false。
    const planGlyphIds: Set<string> = new Set()
    for (const run of prepared.plan.insertRuns) {
      for (const gid of run.glyphIds) {
        planGlyphIds.add(gid)
      }
    }
    for (const run of prepared.plan.deletedRuns) {
      for (const gid of run.glyphIds) {
        planGlyphIds.add(gid)
      }
    }
    for (const move of prepared.plan.retainedMoves) {
      for (const gid of move.glyphIds) {
        planGlyphIds.add(gid)
      }
    }
    const carryOverGhostRuns = this.identifyCarryOverGhosts(
      latestDisplayedFrame, prepared.state.newSnapshot.revision, planGlyphIds
    )

    const newPlan: EditorMotionPlan = {
      insertRuns: newInsertRuns,
      deletedRuns: newDeletedRuns,
      retainedMoves: newRetainedMoves,
      caretPath: retargetedCaretPath,
      durationMs: prepared.plan.durationMs,
      motionKind: prepared.plan.motionKind,
      carryOverGhostRuns: carryOverGhostRuns
    }

    // 用更新后的 plan 和冻结帧构建新的起始帧（progress=0）
    const newStartFrame = EditorMotionCoordinator.buildFrame(
      newPlan,
      prepared.state.oldSnapshot,
      prepared.state.oldLayout,
      prepared.state.newSnapshot,
      prepared.state.newLayout,
      latestDisplayedFrame,
      0,
      this.windowIdAllocator
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
      glyphIdentities: prepared.state.glyphIdentities,
      sessionId: prepared.state.sessionId
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
    firstCaretRect?: CaretRect | null,
    sessionId?: number
  ): EditorVisualFrame {
    const state = this.computeUpdateState(
      oldSnapshot, oldLayout, newSnapshot, newLayout, editResults,
      contentWidth, policy, this.currentFrame, firstCaretRect ?? null,
      sessionId ?? this.currentSessionId
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
    firstCaretRect?: CaretRect | null,
    sessionId?: number
  ): EditorVisualFrame {
    const chain: EditorEditResult[] = []
    if (editResult !== null) {
      chain.push(editResult)
    }
    const state = this.computeUpdateState(
      oldSnapshot, oldLayout, newSnapshot, newLayout, chain,
      contentWidth, policy, this.currentFrame, firstCaretRect ?? null,
      sessionId ?? this.currentSessionId
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
    firstCaretRect: CaretRect | null,
    sessionId: number
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
        glyphIdentities: identities.newTable,
        sessionId: sessionId
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
          plan, oldSnapshot, oldLayout, newSnapshot, newLayout, startFrame, 0,
          this.windowIdAllocator
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
      glyphIdentities: identities.newTable,
      sessionId: sessionId
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
    // Issue #879 复核评论问题5：记录当前会话 ID，用于 sourceLayoutRecords 复合键。
    this.currentSessionId = state.sessionId
    // Issue #879 复核评论6078682695 问题2：记录不可变源排版，供跨修订残影查源布局。
    this.recordSourceLayout(state.oldSnapshot, state.oldLayout)
    this.recordSourceLayout(state.newSnapshot, state.newLayout)
  }

  /**
   * Issue #879 复核评论6078682695 问题2：记录一个 revision 的不可变源排版。
   *
   * 按 sessionId+revision 标识缓存 {text, lineLayouts}，供冻结窗口按 w.sourceRevision 查它自己的布局。
   *
   * Issue #879 复核评论问题5：缓存键改为 sessionId+revision（复合键），
   * 防止跨会话 revision 重复时取到上一个作品的旧布局。
   */
  private recordSourceLayout(snapshot: EditorLayoutSnapshot, layout: LineLayout[]): void {
    const key = `${this.currentSessionId}-${snapshot.revision}`
    if (!this.sourceLayoutRecords.has(key)) {
      this.sourceLayoutRecords.set(key, {
        revision: snapshot.revision,
        text: snapshot.text,
        lineLayouts: layout
      })
    }
  }

  /**
   * Issue #879 复核评论6078682695 问题2：按 sourceRevision 查不可变源排版记录。
   *
   * Issue #879 复核评论问题5：缓存键改为 sessionId+revision（复合键），
   * 防止跨会话 revision 重复时取到上一个作品的旧布局。
   *
   * @returns 匹配的源排版记录，或 null（缓存中没有该 revision）
   */
  private getSourceLayout(revision: number): SourceLayoutRecord | null {
    const key = `${this.currentSessionId}-${revision}`
    const record = this.sourceLayoutRecords.get(key)
    return record ?? null
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
      motionKind: MotionKind.None,
      carryOverGhostRuns: []
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
      this.oldLayout,
      this.newSnapshot,
      this.newLayout,
      this.currentFrame,
      progress,
      this.windowIdAllocator
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
   * Issue #879 复核评论问题4：窗口身份与字形身份分开——renderNodeKey 用 plan 计算出的
   * windowId（窗口身份），跨帧接续用窗口的 glyphIds（字形单元身份）交集，
   * 不再用 utf16 区间 + 文本 hash 现场重算，也不拿窗口首字形代表整窗口。
   *
   * Issue #879 复核评论6077187962 问题4：renderNodeKey 改用 windowInstanceId（win-<number>）
   * 替代 windowId（长度随 glyphIds 线性增长）。windowInstanceId 由 WindowInstanceIdAllocator
   * 分配，同一 glyphIds 序列在活跃期间始终映射到同一个 windowInstanceId。
   * windowId 仍保留在 MotionGlyphWindow 中作为算法内部的精确身份。
   */
  private static buildFrame(
    plan: EditorMotionPlan,
    oldSnapshot: EditorLayoutSnapshot | null,
    oldLayout: LineLayout[],
    newSnapshot: EditorLayoutSnapshot | null,
    newLayout: LineLayout[],
    caretBase: EditorVisualFrame,
    progress: number,
    allocator: WindowInstanceIdAllocator
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
        plan.carryOverGhostRuns.length === 0 &&
        plan.caretPath.length === 0) {
      return caretBase
    }

    // 同一 progress 同时采样所有通道
    const motionGlyphWindows: MotionGlyphWindow[] = []

    // 1. 吐字通道：新字源完整排版，裁切窗口从起始状态扩展到目标状态
    //    Issue #879 复核评论6078682695 问题1：按 piece 分别采样，
    //    每个 piece 生成独立的 MotionGlyphWindow，区间之间的空洞绝不填补。
    for (const run of plan.insertRuns) {
      const runBoundaries = clusterBoundaries(newText, run.utf16Start, run.utf16End)
      for (const piece of run.pieces) {
        // 计算 piece 的目标 clip（该 piece 覆盖的字形在目标布局中的完整边界）
        let targetLeft = run.rect.x
        let targetRight = run.rect.x + run.rect.width
        let pieceUtf16Start = run.utf16Start
        let pieceUtf16End = run.utf16End
        if (runBoundaries.length === run.glyphIds.length + 1 &&
            piece.firstIndex >= 0 && piece.lastIndex + 1 < runBoundaries.length) {
          pieceUtf16Start = runBoundaries[piece.firstIndex]
          pieceUtf16End = runBoundaries[piece.lastIndex + 1]
          targetLeft = EditorMotionCoordinator.xAtOffsetLocal(newLayout, pieceUtf16Start, run.rect.x)
          targetRight = EditorMotionCoordinator.xAtOffsetLocal(newLayout, pieceUtf16End, run.rect.x + run.rect.width)
        } else {
          // Issue #879 复核评论6080604353 问题2coord：fallback 分支用 piece 自身的起始 clip
          // 作为目标左右边界参考——gap piece 的起始位置应该是 piece 自身的左边界，
          // 而不是整 run 的左边界。
          targetLeft = piece.startClipLeft
          targetRight = piece.startClipRight
        }
        const currentLeft = piece.startClipLeft + (targetLeft - piece.startClipLeft) * p
        const currentRight = piece.startClipRight + (targetRight - piece.startClipRight) * p
        const windowInstanceId = allocator.allocate(piece.glyphIds)
        motionGlyphWindows.push({
          sourceKind: 'new',
          utf16Start: pieceUtf16Start,
          utf16End: pieceUtf16End,
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
          glyphIds: piece.glyphIds,
          windowId: run.windowId,
          windowInstanceId: windowInstanceId,
          renderNodeKey: `new-${newRevision}-${windowInstanceId}`,
          currentClipLeft: currentLeft,
          currentClipRight: currentRight,
          currentPositionX: run.rect.x,
          currentPositionY: run.rect.y,
          motionOwnerKind: 'insert'
        })
      }
    }

    // 2. 吞字通道：旧字源留在动画层，裁切窗口从起始状态缩到目标状态
    //    Issue #879 复核评论6078682695 问题1：按 piece 分别采样。
    //    Issue #879 问题3修复：每个 piece 的 utf16Start/End 按 piece 的
    //    firstIndex/lastIndex 和 clusterBoundaries 计算，不再写整 run 范围。
    //    clipTop/Bottom/glyphX/Y/Width/Height 也按 piece 对应的 oldLayout 几何产生。
    for (const run of plan.deletedRuns) {
      const runBoundaries = clusterBoundaries(oldText, run.utf16Start, run.utf16End)
      for (const piece of run.pieces) {
        // 计算 piece 的 utf16 范围
        let pieceUtf16Start = run.utf16Start
        let pieceUtf16End = run.utf16End
        if (runBoundaries.length === run.glyphIds.length + 1 &&
            piece.firstIndex >= 0 && piece.lastIndex + 1 < runBoundaries.length) {
          pieceUtf16Start = runBoundaries[piece.firstIndex]
          pieceUtf16End = runBoundaries[piece.lastIndex + 1]
        }
        // 目标 clip = 零宽度（collapseX）
        const collapseX = run.isForward ? caretBase.caretX : run.rect.x
        const targetLeft = collapseX
        const targetRight = collapseX
        const currentLeft = piece.startClipLeft + (targetLeft - piece.startClipLeft) * p
        const currentRight = piece.startClipRight + (targetRight - piece.startClipRight) * p
        const windowInstanceId = allocator.allocate(piece.glyphIds)
        motionGlyphWindows.push({
          sourceKind: 'old',
          utf16Start: pieceUtf16Start,
          utf16End: pieceUtf16End,
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
          glyphIds: piece.glyphIds,
          windowId: run.windowId,
          windowInstanceId: windowInstanceId,
          renderNodeKey: `old-${oldRevision}-${windowInstanceId}`,
          currentClipLeft: currentLeft,
          currentClipRight: currentRight,
          currentPositionX: run.rect.x,
          currentPositionY: run.rect.y,
          motionOwnerKind: 'deleted'
        })
      }
    }

    // 3. 保留字移动通道：旧字源绘制后整体偏移到当前插值位置
    //    Issue #879 复核评论6078682695 问题1：按 piece 分别采样。
    //    Issue #879 问题3修复：每个 piece 的 utf16Start/End 按 piece 的
    //    firstIndex/lastIndex 和 clusterBoundaries 计算，不再写整 move 范围。
    //    clipLeft/Right 也按 piece 对应的 oldLayout 几何产生，而非整块 oldRect.width。
    //    Issue #879 复核评论6080604353 问题3修复：每个 piece 运动到各自的新版目标位置，
    //    而不是都奔向同一个 move.newRect.x/y。用 newLayout 的 clusterBoundaries
    //    查出每个 piece 在新布局中的 pieceNewX/pieceNewY/pieceNewWidth。
    for (const move of plan.retainedMoves) {
      const moveBoundaries = clusterBoundaries(oldText, move.oldUtf16Start, move.oldUtf16End)
      // 同时计算新正文的 clusterBoundaries——用于查出每个 piece 在新布局中的位置
      const newMoveBoundaries = clusterBoundaries(newText, move.newUtf16Start, move.newUtf16End)
      for (const piece of move.pieces) {
        // 计算 piece 的 utf16 范围
        let pieceUtf16Start = move.oldUtf16Start
        let pieceUtf16End = move.oldUtf16End
        let pieceOldLeft = move.oldRect.x
        let pieceOldRight = move.oldRect.x + move.oldRect.width
        // Issue #879 复核评论6080604353 问题3：每个 piece 运动到各自的新版目标位置
        let pieceNewX = move.newRect.x
        let pieceNewY = move.newRect.y
        if (moveBoundaries.length === move.glyphIds.length + 1 &&
            piece.firstIndex >= 0 && piece.lastIndex + 1 < moveBoundaries.length) {
          pieceUtf16Start = moveBoundaries[piece.firstIndex]
          pieceUtf16End = moveBoundaries[piece.lastIndex + 1]
          // 按 piece 在 oldLayout 中的位置计算裁切宽度
          pieceOldLeft = EditorMotionCoordinator.xAtOffsetLocal(
            oldLayout, pieceUtf16Start, move.oldRect.x)
          pieceOldRight = EditorMotionCoordinator.xAtOffsetLocal(
            oldLayout, pieceUtf16End, move.oldRect.x + move.oldRect.width)
          // 用新正文的 clusterBoundaries 计算每个 piece 在新布局中的位置
          if (newMoveBoundaries.length === move.glyphIds.length + 1 &&
              piece.firstIndex >= 0 && piece.lastIndex + 1 < newMoveBoundaries.length) {
            const pieceNewUtf16Start = newMoveBoundaries[piece.firstIndex]
            const pieceNewUtf16End = newMoveBoundaries[piece.lastIndex + 1]
            pieceNewX = EditorMotionCoordinator.xAtOffsetLocal(
              newLayout, pieceNewUtf16Start, move.newRect.x)
            pieceNewY = EditorMotionCoordinator.yAtOffsetLocal(
              newLayout, pieceNewUtf16Start, move.newRect.y)
          }
        }
        const pieceOldWidth = pieceOldRight - pieceOldLeft
        // 每个 piece 运动到各自的新版目标位置
        const currentX = piece.startPositionX + (pieceNewX - piece.startPositionX) * p
        const currentY = piece.startPositionY + (pieceNewY - piece.startPositionY) * p
        const windowInstanceId = allocator.allocate(piece.glyphIds)
        motionGlyphWindows.push({
          sourceKind: 'old',
          utf16Start: pieceUtf16Start,
          utf16End: pieceUtf16End,
          clipLeft: currentX,
          clipRight: currentX + pieceOldWidth,
          clipTop: currentY,
          clipBottom: currentY + move.oldRect.height,
          glyphX: move.oldRect.x,
          glyphY: move.oldRect.y,
          glyphWidth: move.oldRect.width,
          glyphHeight: move.oldRect.height,
          // Issue #879 复核评论6080604353 问题3：offsetX 基于 piece 的旧左边界，而非整 move 的旧左边界
          offsetX: currentX - pieceOldLeft,
          offsetY: currentY - move.oldRect.y,
          runId: `old-${oldRevision}`,
          sourceRevision: oldRevision,
          glyphIds: piece.glyphIds,
          windowId: move.windowId,
          windowInstanceId: windowInstanceId,
          renderNodeKey: `old-${oldRevision}-${windowInstanceId}`,
          currentClipLeft: currentX,
          currentClipRight: currentX + pieceOldWidth,
          currentPositionX: currentX,
          currentPositionY: currentY,
          motionOwnerKind: 'reflow'
        })
      }
    }

    // Issue #879 复核评论6078682695 问题2：carryOver ghost 残影采样。
    // 每帧同时采样本轮新 run 与 carryOver ghost，
    // 从 currentClipLeft/Right 插值到 targetClipLeft/Right（零宽度），
    // 保持其旧源 TextController/StyledString 有效，直到残影真正隐藏才释放。
    for (const ghost of plan.carryOverGhostRuns) {
      const currentLeft = ghost.currentClipLeft + (ghost.targetClipLeft - ghost.currentClipLeft) * p
      const currentRight = ghost.currentClipRight + (ghost.targetClipRight - ghost.currentClipRight) * p
      const windowInstanceId = allocator.allocate(ghost.glyphIds)
      motionGlyphWindows.push({
        sourceKind: 'old',
        utf16Start: ghost.utf16Start,
        utf16End: ghost.utf16End,
        clipLeft: currentLeft,
        clipRight: currentRight,
        clipTop: ghost.clipTop,
        clipBottom: ghost.clipBottom,
        glyphX: ghost.glyphX,
        glyphY: ghost.glyphY,
        glyphWidth: ghost.glyphWidth,
        glyphHeight: ghost.glyphHeight,
        offsetX: ghost.offsetX,
        offsetY: ghost.offsetY,
        runId: `old-${ghost.sourceRevision}`,
        sourceRevision: ghost.sourceRevision,
        glyphIds: ghost.glyphIds,
        windowId: ghost.windowId,
        windowInstanceId: windowInstanceId,
        renderNodeKey: `old-${ghost.sourceRevision}-${windowInstanceId}`,
        currentClipLeft: currentLeft,
        currentClipRight: currentRight,
        currentPositionX: ghost.glyphX + ghost.offsetX,
        currentPositionY: ghost.glyphY + ghost.offsetY,
        motionOwnerKind: 'carryGhost'
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

    // Issue #879 复核评论问题4：buildFrame 不再登记 lease——
    // lease 在 Text.onAppear 后才 acquire（由 SujianEditor 调 acquireRenderNodeLease）。
    // 候选被更高 revision 作废时 ArkUI 可能从未创建那个 Text，
    // 此时不存在对应 onDisAppear，lease 不应被提前登记。
    // buildFrame 只构造不可变窗口描述与匹配 ID。

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
   * Issue #879 复核评论6078682695 问题2：识别 carryOver ghost 残影。
   *
   * 上一帧仍有非零 clip、且尚未归还静态层的旧窗口作为残影继承到候选 plan。
   * 条件：
   * - 窗口的 motionOwnerKind 是 'deleted' 或 'carryGhost'（只继承删除残影和上一帧的 carryGhost）
   * - 窗口的 sourceRevision 不等于新正文 revision（不是本轮新插入的）
   * - 窗口仍有非零 clip 宽度（尚未完全消退）
   * - 窗口的 glyphIds 全部不在本轮新 plan 的 run 集合中（是真正的残影）
   *
   * Issue #879 复核评论6080604353 问题1修复：
   * - 增加了 motionOwnerKind 检查——排除 'reflow'（保留字移动）和 'insert'（吐字）类型的窗口
   *   被误判为 ghost。'reflow' 类型的窗口虽然 sourceKind==='old'，但它们仍存在于新版正文中，
   *   应该继续平移或回到新版静态层，不应作为 ghost 消退。
   * - motionOwnerKind 检查在 sourceKind 检查之前（更语义化）。
   *
   * 残影的目标 clip = 零宽度（继续消退到完全隐藏）。
   *
   * @param latestDisplayedFrame 冻结时刻的 displayedVisualFrame
   * @param newRevision 新正文 revision
   * @param planGlyphIds 本轮新 plan 中所有 run 的 glyphIds 集合（用于排除正在处理的字形）
   */
  private identifyCarryOverGhosts(
    latestDisplayedFrame: EditorVisualFrame,
    newRevision: number,
    planGlyphIds: Set<string>
  ): CarryOverGhostRun[] {
    const ghosts: CarryOverGhostRun[] = []
    if (!latestDisplayedFrame.isAnimating) {
      return ghosts
    }
    for (const w of latestDisplayedFrame.motionGlyphWindows) {
      // Issue #879 复核评论6080604353 问题1：只继承删除残影和上一帧的 carryGhost——
      // 不继承 reflow（保留字移动）和 insert（吐字）。
      // reflow 类型的窗口虽然 sourceKind==='old'，但它们仍存在于新版正文中，
      // 应该继续平移或回到新版静态层，不应作为 ghost 消退。
      if (w.motionOwnerKind !== 'deleted' && w.motionOwnerKind !== 'carryGhost') {
        continue
      }
      if (w.sourceRevision === newRevision) {
        continue
      }
      // 仍有非零 clip 宽度
      if (w.currentClipRight - w.currentClipLeft <= 0.01) {
        continue
      }
      // 该窗口的 glyphIds 全部不在本轮新 plan 的 run 集合中（是真正的残影）
      let allGhost = true
      for (const gid of w.glyphIds) {
        if (planGlyphIds.has(gid)) {
          allGhost = false
          break
        }
      }
      if (!allGhost) {
        continue
      }
      // 目标 clip = 零宽度（继续消退到完全隐藏）
      const targetClipLeft = w.currentClipLeft
      const targetClipRight = w.currentClipLeft
      ghosts.push({
        glyphIds: w.glyphIds,
        windowId: w.windowId,
        sourceRevision: w.sourceRevision,
        utf16Start: w.utf16Start,
        utf16End: w.utf16End,
        currentClipLeft: w.currentClipLeft,
        currentClipRight: w.currentClipRight,
        targetClipLeft: targetClipLeft,
        targetClipRight: targetClipRight,
        clipTop: w.clipTop,
        clipBottom: w.clipBottom,
        glyphX: w.glyphX,
        glyphY: w.glyphY,
        glyphWidth: w.glyphWidth,
        glyphHeight: w.glyphHeight,
        offsetX: w.offsetX,
        offsetY: w.offsetY
      })
    }
    return ghosts
  }

  /**
   * Issue #879 复核评论6075662695问题3：组装提交瞬间的「在屏」上下文。
   *
   * 上下文 = 提交瞬间真实的 displayed 正文 + 布局 + 身份表 + 冻结帧的运动窗口。
   * 身份表缺失、或者 displayed 的 revision 与候选链基准不一致（链基准已移动）时
   * 返回 null——此时不猜几何，调用方保留 plan 里的起点。
   *
   * 冻结帧已经没有运动窗口（旧动画正常结束）是**合法**情况：
   * 此时 empty windows 表示 displayed 正文全部静态可见，
   * 起点由 displayed 布局的真实字形几何给出，而不是 prepareCandidate 时刻的半裁切值。
   */
  private commitDisplayedContext(
    prepared: PreparedCandidate,
    latestDisplayedFrame: EditorVisualFrame,
    displayed: DisplayedSnapshotContext | null
  ): DisplayedContext | null {
    if (displayed === null || displayed.identities === null) {
      return null
    }
    if (displayed.snapshot.revision !== prepared.state.oldSnapshot.revision) {
      return null
    }
    const frozenWindows: FrozenWindowSpan[] = []
    for (const w of latestDisplayedFrame.motionGlyphWindows) {
      // Issue #879 复核评论6078682695 问题2：按 w.sourceRevision 查不可变源排版记录，
      // 不按 sourceKind 猜 displayed/newSnapshot——跨修订残影时二者都不对。
      const sourceRecord = this.getSourceLayout(w.sourceRevision)
      let sourceText: string
      let sourceLayout: LineLayout[]
      if (sourceRecord !== null) {
        sourceText = sourceRecord.text
        sourceLayout = sourceRecord.lineLayouts
      } else {
        // 兜底：缓存中没有该 revision，退回 sourceKind 猜测
        sourceText = w.sourceKind === 'old' ? displayed.snapshot.text : prepared.state.newSnapshot.text
        sourceLayout = w.sourceKind === 'old' ? displayed.layout : prepared.state.newLayout
      }

      // 用 clusterBoundaries 把窗口的 UTF-16 范围拆成字符簇边界，
      // 再与 glyphIds 一一对应，构建 glyphUtf16Ranges。
      const glyphUtf16Ranges = EditorMotionCoordinator.buildGlyphUtf16Ranges(
        w.glyphIds, w.utf16Start, w.utf16End, sourceText
      )

      frozenWindows.push({
        glyphIds: w.glyphIds,
        clipLeft: w.currentClipLeft,
        clipRight: w.currentClipRight,
        offsetX: w.offsetX,
        offsetY: w.offsetY,
        sourceRevision: w.sourceRevision,
        glyphUtf16Ranges: glyphUtf16Ranges,
        sourceLayout: sourceLayout
      })
    }
    return {
      text: displayed.snapshot.text,
      layout: displayed.layout,
      identities: displayed.identities,
      frozenWindows: frozenWindows
    }
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
      motionKind: MotionKind.None,
      carryOverGhostRuns: []
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
   * Issue #879 复核评论6075662695问题3：获取描述**当前已提交（在屏）正文**的字形身份表。
   *
   * 候选提交瞬间的位置重算要用它把 run 携带的 glyphId 映射回在屏正文的 UTF-16 区间。
   * 表描述的是 currentFrame 对应的 revision：候选准备期间不推进，
   * 提交后（applyCommittedState）才换成新版本的表。
   */
  getCommittedIdentities(): GlyphIdentityTable | null {
    return this.glyphIdentities
  }

  // ── 私有方法 ──

  /**
   * Issue #879 复核评论6078682695 问题1：取 offset 在一行布局里的 x（vp）。
   *
   * buildFrame 按 piece 采样时需要计算 piece 的目标 clip（该 piece 覆盖的字形在目标布局中的完整边界）。
   */
  private static xAtOffsetLocal(layout: LineLayout[], offset: number, fallback: number): number {
    for (const line of layout) {
      if (offset >= line.startUtf16 && offset <= line.endUtf16) {
        let x = line.left
        for (const stop of line.caretStops) {
          if (stop.utf16Offset <= offset) {
            x = stop.x
          }
        }
        return x
      }
    }
    return fallback
  }

  /**
   * Issue #879 复核评论6080604353 问题3：取 offset 在一行布局里的 y（vp）。
   *
   * RetainedMove 通道按 piece 采样时需要计算每个 piece 在新布局中的 y 坐标，
   * 用于让每个 piece 运动到各自的新版目标行位置（而非整 move 的 newRect.y）。
   */
  private static yAtOffsetLocal(layout: LineLayout[], offset: number, fallback: number): number {
    for (const line of layout) {
      if (offset >= line.startUtf16 && offset <= line.endUtf16) {
        return line.y
      }
    }
    return fallback
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

  /**
   * Issue #879 复核评论问题4：在 Text.onAppear 后获取渲染节点租约。
   *
   * buildFrame 只构造窗口描述与匹配 ID，不登记 lease。
   * lease 在 Text 真正挂载（onAppear）后才 acquire——
   * 候选被作废时 ArkUI 可能从未创建那个 Text，此时不存在对应 onDisAppear，
   * lease 不应被提前登记。
   *
   * @param renderNodeKey 渲染节点身份标识
   * @param windowInstanceId `win-<number>` 格式 ID
   */
  acquireRenderNodeLease(renderNodeKey: string, windowInstanceId: string): void {
    this.windowIdAllocator.addLease(renderNodeKey, windowInstanceId)
  }

  /**
   * Issue #879 复核评论6078682695 问题3：释放渲染节点租约。
   *
   * Text.onDisAppear 时调用——只释放该 renderNodeKey 持有的租约，
   * 同一 glyphIds 序列同时供多个节点使用时，只有最后一个租约被归还才真正释放 instanceId。
   *
   * @param renderNodeKey 渲染节点身份标识
   * @param windowInstanceId `win-<number>` 格式 ID
   */
  releaseRenderNodeLease(renderNodeKey: string, windowInstanceId: string): void {
    this.windowIdAllocator.releaseLease(renderNodeKey, windowInstanceId)
  }

  /**
   * Issue #879 复核评论6077187962 问题4：释放窗口实例 ID（兼容旧接口）。
   *
   * @param windowInstanceId `win-<number>` 格式 ID
   */
  releaseWindowInstance(windowInstanceId: string): void {
    this.windowIdAllocator.release(windowInstanceId)
  }

  /**
   * Issue #879 复核评论6077187962 问题4：清空所有窗口实例 ID 映射。
   *
   * 会话切换或编辑器重置时调用。
   */
  clearWindowInstances(): void {
    this.windowIdAllocator.clear()
  }

  /**
   * Issue #879 复核评论问题5：清空源排版记录缓存。
   *
   * 会话切换时调用——防止跨会话 revision 重复时取到上一个作品的旧布局。
   * 缓存键为 sessionId+revision，会话切换后新会话的键不会命中旧会话的记录，
   * 但清空可防止 Map 无限增长。
   */
  clearSourceLayoutRecords(): void {
    this.sourceLayoutRecords.clear()
  }

  /**
   * Issue #879 复核评论6080604353 问题5：会话切换时重置 Coordinator 状态。
   *
   * 清空当前 plan、已提交身份表、源版本缓存、窗口实例 ID。
   * 安装新会话的初始快照与布局，使 Coordinator 从新会话的静态状态开始工作。
   *
   * @param sessionId 新会话 ID
   * @param snapshot 新会话的初始正文快照
   * @param layout 新会话的初始行布局
   */
  resetForSession(sessionId: number, snapshot: EditorLayoutSnapshot, layout: LineLayout[]): void {
    this.currentPlan = EditorMotionCoordinator.emptyPlan()
    this.currentFrame = EditorMotionCoordinator.computeStaticFrame(snapshot, layout, null)
    this.glyphIdentities = GlyphIdentityTable.create(snapshot.text, snapshot.revision)
    this.sourceLayoutRecords.clear()
    this.windowIdAllocator.clear()
    this.currentSessionId = sessionId
    // 保存新会话的源排版记录
    this.sourceLayoutRecords.set(`${sessionId}-${snapshot.revision}`, {
      revision: snapshot.revision,
      text: snapshot.text,
      lineLayouts: layout
    })
    // 重置几何和版本身份
    this.geometryKey = {
      fontSize: snapshot.fontSize,
      lineSpacing: snapshot.lineSpacingMultiplier,
      contentWidth: 0
    }
    this.textRevisionKey = {
      revision: snapshot.revision,
      generation: snapshot.generation,
      compositionGeneration: snapshot.compositionGeneration
    }
    this.oldSnapshot = snapshot
    this.oldLayout = layout
    this.newSnapshot = snapshot
    this.newLayout = layout
    this.motionDurationMs = 0
    this.firstCaretRect = null
  }
}
