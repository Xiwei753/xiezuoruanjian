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

import { EditorVisualFrame, MotionKind, MotionGlyphWindow, MotionSegment, StaticVisibleRange, InsertRun, DeletedRun, RetainedMove, GlyphRect, CarryOverGhostRun, CarryOverMotionRun, ClipRect } from './EditorVisualFrame'
import { EditorMotionPolicy } from './EditorMotionPolicy'
import { EditorMotionPlan, EditorMotionPlanner } from './EditorMotionPlanner'
import { EditorLayoutSnapshot } from '../render/EditorLayoutSnapshot'
import { LineLayout, CaretRect, glyphRectForRange } from '../render/editor_render_geometry'
import { resolveVisualLineIndex, CaretAffinity } from '../render/editor_layout_math'
import type { LineRange } from '../render/editor_layout_math'
import { TextOffsetMapper } from '../input/TextOffsetMapper'
import { EditorEditResult, DisplayPatch, OffsetMap } from '../../../corebridge/dto/EditorDtos'
import { EditorMotionTransactionComposer, ComposedTransaction } from './EditorMotionTransactionComposer'
import type { GlyphCarryPatch } from './editor_patch_carry'
import { applyPatchesToText } from './editor_patch_carry'
import { GlyphIdentityTable, GlyphIdentityEntry, WindowInstanceIdAllocator, clusterBoundaries } from './editor_glyph_identity'
import {
  DisplayedContext, FrozenWindowSpan, RunGeometry, RunStartState, RunStartPiece, RunStartInterval,
  RectLike, retargetRunStarts, rectDifference, isFullyCovered, mapRectByRatio, FrozenGlyphMember
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
 * Issue #879 复核评论6084020082 问题1+2+3：冻结窗口按字形所有权拆分后的片段。
 *
 * splitFrozenWindowByGlyphOwnership 返回的每个 piece 包含独立的源/目标范围和几何，
 * 不再共享母窗口的整段范围。
 */
interface FrozenWindowPiece {
  /** 该片段覆盖的字符簇身份列表 */
  glyphIds: string[]
  /** 该片段在源正文中的 UTF-16 起始 offset */
  sourceUtf16Start: number
  /** 该片段在源正文中的 UTF-16 结束 offset（exclusive） */
  sourceUtf16End: number
  /** 该片段在源排版中的矩形（来自源布局，不是新版 targetRect） */
  sourceRect: GlyphRect | null
  /** 字形排版位置 x（vp）——来自源布局 */
  glyphX: number
  /** 字形排版位置 y（vp）——来自源布局 */
  glyphY: number
  /** 字形宽度（vp）——来自源布局 */
  glyphWidth: number
  /** 字形高度（vp）——来自源布局 */
  glyphHeight: number
  /** 冻结裁切左边界（vp）——源矩形与原窗口 clip 的交集 */
  clipLeft: number
  /** 冻结裁切右边界（vp）——源矩形与原窗口 clip 的交集 */
  clipRight: number
  /** 冻结裁切上边界（vp） */
  clipTop: number
  /** 冻结裁切下边界（vp） */
  clipBottom: number
  /** 绘制偏移 x（vp）——沿用原窗口的 offsetX */
  offsetX: number
  /** 绘制偏移 y（vp）——沿用原窗口的 offsetY */
  offsetY: number
  /** 当前位置 x（vp）——沿用原窗口的 currentPositionX */
  currentPositionX: number
  /** 当前位置 y（vp）——沿用原窗口的 currentPositionY */
  currentPositionY: number
  /** 该片段在新版正文中的 UTF-16 起始 offset（-1 表示未解析） */
  targetUtf16Start: number
  /** 该片段在新版正文中的 UTF-16 结束 offset（exclusive，-1 表示未解析） */
  targetUtf16End: number
  /** 目标范围是否已解析（newIdentityTable 中查到了对应范围） */
  targetResolved: boolean
  /** 该片段在新版布局中的矩形（null 表示未解析） */
  targetRect: GlyphRect | null
  /** 独立的渲染节点身份标识 */
  renderNodeKey: string
  /** 独立的窗口实例 ID */
  windowInstanceId: string
  /** Issue #879 评论6096633126 问题1：该片段在冻结帧的多岛裁切矩形（与原窗口 clipRects 求交） */
  clipRects: ClipRect[]
}

/**
 * Issue #879 复核评论6120849815：预计算 FrozenWindowGroupPlan 时的中间条目。
 * ArkTS 不允许对象字面量作为类型声明（arkts-no-obj-literals-as-types），
 * 必须用具名接口。
 */
interface FrozenMemberEntry {
  member: FrozenGlyphMember
  ownTargetX: number
  ownTargetY: number
  intervalGlyphIds: string[]
}

/**
 * Issue #879 复核评论6120195493：FrozenWindowGroupPlan 替代旧 frozenGroupDelta。
 *
 * 同一 renderNodeKey 的所有 frozen 成员共享一张 ArkUI Text，必须只有一份窗口级
 * offset/alpha/position，不能由各字先计算再保留第一个字的数值。
 * 各字只保留自己的裁切矩形和源 UTF-16 子区间。
 */
interface FrozenWindowGroupPlan {
  /** 窗口级起始 offsetX（来自原始 FrozenWindowSpan/MotionGlyphWindow，不是第一成员的值） */
  startOffsetX: number
  /** 窗口级起始 offsetY */
  startOffsetY: number
  /** 窗口级起始 alpha（来自原始窗口的 opacity） */
  startAlpha: number
  /** 窗口级目标 alpha */
  endAlpha: number
  /** 窗口级目标 offsetX */
  targetOffsetX: number
  /** 窗口级目标 offsetY */
  targetOffsetY: number
  /** 窗口级当前位置 X */
  currentX: number
  /** 窗口级当前位置 Y */
  currentY: number
  /** 窗口级当前 alpha（插值后的值） */
  currentAlpha: number
  /** 该组的 renderNodeKey */
  renderNodeKey: string
  /** 该组的 runId */
  runId: string
  /** 该组的 sourceRevision */
  sourceRevision: number
  /** 该组的 sourceKind */
  sourceKind: 'old' | 'new'
  /** 该组的 windowInstanceId */
  windowInstanceId: string
  /**
   * Issue #879 评论6128658045 问题1+3：物理源窗口的完整 glyphIds 列表。
   *
   * 合并 originalGlyphIds 和 fullWindowGlyphIds 为唯一字段。
   * 从 baseMember.fullWindowGlyphIds 取值，而非 baseEntry.intervalGlyphIds（只是单个 interval 的 glyphIds）。
   * 确保 MotionGlyphWindow.fullWindowGlyphIds 携带完整字形列表，
   * commitDisplayedContext 冻结时 frozenIndex 能为窗口内所有字形建立索引。
   */
  fullWindowGlyphIds: string[]
  /**
   * Issue #879 评论6128658045 问题1+3：物理源窗口的完整源 UTF-16 起始位置。
   *
   * 合并 originalUtf16Start 和 fullWindowUtf16Start 为唯一字段。
   * 从 baseMember.fullWindowUtf16Start 取值，而非 baseMember.sourceUtf16Start（只是单个成员的起始）。
   */
  fullWindowUtf16Start: number
  /**
   * Issue #879 评论6128658045 问题1+3：物理源窗口的完整源 UTF-16 结束位置。
   *
   * 合并 originalUtf16End 和 fullWindowUtf16End 为唯一字段。
   */
  fullWindowUtf16End: number
  /**
   * Issue #879 评论6128658045 问题1+3：物理源窗口的完整逐字 glyphId→UTF-16 映射。
   *
   * 合并 originalGlyphUtf16Ranges 和 fullWindowGlyphUtf16Ranges 为唯一字段。
   * 从 baseMember.fullWindowGlyphUtf16Ranges 取值，而非 baseMember.glyphUtf16Ranges。
   */
  fullWindowGlyphUtf16Ranges: Map<string, [number, number]> | null
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
      carryOverGhostRuns: [],
      carryOverMotionRuns: []
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
   * Issue #879 复核评论6088787701 问题4：按**稳定物理节点规划**列出候选提交后将占用的
   * renderNodeKey 集合。
   *
   * 身份策略与真正采样帧的 buildFrame 完全一致——同一个 run 的同一个 piece 在这里和
   * buildFrame 里调用同一个 windowIdAllocator.allocate(piece.glyphIds)，因此返回的 key
   * 就是提交后真实窗口的 key（无动画时为空数组）。
   *
   * 关键：key 只来自 plan 的 run/piece 结构与 sourceRevision，**不依赖任何一帧的可见性**。
   * retargetCandidate 只重算 piece 的起始 clip/position，不再重新拆分 piece，
   * 所以「staging 阶段检查的 key 集合」就是「提交后真实窗口的 key 集合」，
   * 全部节点可以在 cancel 旧 animator 之前挂载并完成布局。
   *
   * @param prepared prepareCandidate/retargetCandidate 返回的候选
   * @returns 提交后窗口的 renderNodeKey 数组（无动画时为空）
   */
  plannedWindowKeys(prepared: PreparedCandidate): string[] {
    const plan = prepared.plan
    // Issue #879 本轮复核评论（阻塞）：动效关闭 / 动画时长为 0 时，plan 仍可能含 run，
    // 但 buildFrame 生成的是 computeStaticFrame（isAnimating=false、motionGlyphWindows=[]），
    // 不会有任何运动 Text 需要 layout。这里直接返回空数组，与「无动画时返回空数组」的契约一致——
    // 否则会返回一批永远等不到 layout 的 key，让 allMotionWindowsReady 一直 false、正文永不提交。
    if (plan.durationMs <= 0) {
      return []
    }
    const keys: string[] = []
    const newRevision = prepared.state.newSnapshot?.revision ?? 0
    const oldRevision = prepared.state.oldSnapshot?.revision ?? 0
    for (const run of plan.insertRuns) {
      for (const piece of run.pieces) {
        keys.push(`new-${newRevision}-${this.windowIdAllocator.allocate(piece.glyphIds)}`)
      }
    }
    for (const run of plan.deletedRuns) {
      for (const piece of run.pieces) {
        keys.push(`old-${oldRevision}-${this.windowIdAllocator.allocate(piece.glyphIds)}`)
      }
    }
    // 问题2：冻结多字窗口重复 key 丢字——同一 renderNodeKey 的 frozen 成员
    // 跨多个 interval 只输出一次 key，buildFrame 会按 renderNodeKey 聚合合并 clipRects。
    // Issue #879 评论6128658045 问题2：buildSourceMembers 已确保 renderNodeKey 与 sourceRevision
    // 一致（从 sourceKind + sourceRevision + windowInstanceId 重建），不再需要冲突检测和
    // -conflict-N 后缀。plannedWindowKeys 和 buildFrame 都直接使用 member.renderNodeKey。
    const frozenKeysSeen: Set<string> = new Set()
    for (const move of plan.retainedMoves) {
      for (const piece of move.pieces) {
        // Issue #879 复核评论6099070438 问题1：retained 按 interval 分配 key，
        // 与 buildFrame retainedMoves 一致——多岛时 plannedWindowKeys 和帧输出 key 集合相等，
        // sameKeySet 不再恒 false，候选不再持续滞留，屏幕能切换到新版正文。
        const intervals = EditorMotionCoordinator.pieceIntervals(piece)
        for (const interval of intervals) {
          // Issue #879 评论6108900966 问题1：优先检查 sourceMembers，
          // 为每个成员输出对应的 renderNodeKey。
          if (interval.sourceMembers !== undefined && interval.sourceMembers.length > 0) {
            const winId = this.windowIdAllocator.allocate(interval.glyphIds)
            for (const member of interval.sourceMembers) {
              if (member.role === 'frozen') {
                // 问题2：同一 renderNodeKey 只输出一次——多个 interval 的 frozen 成员
                // 共享同一物理 Text 节点，buildFrame 按 renderNodeKey 聚合合并 clipRects。
                // Issue #879 评论6128658045 问题2：不再检测身份冲突或分配 -conflict-N 后缀。
                // buildSourceMembers 已从源头保证 renderNodeKey 与 sourceRevision 一致。
                if (!frozenKeysSeen.has(member.renderNodeKey)) {
                  frozenKeysSeen.add(member.renderNodeKey)
                  keys.push(member.renderNodeKey)
                }
              } else {
                // target 成员需要追加新分配的 windowInstanceId
                keys.push(`${member.renderNodeKey}-${winId}`)
              }
            }
            // 问题5：如果没有 newRevision 目标成员，额外输出 new key
            // buildFrame 会为没有 newRevision 成员的 interval 创建新目标成员，
            // plannedWindowKeys 必须同时输出对应的 key，否则 sameKeySet 检查失败
            const hasNewRevisionMember = interval.sourceMembers.some(
              m => m.sourceRevision === newRevision
            )
            if (!hasNewRevisionMember) {
              keys.push(`new-${newRevision}-${winId}`)
            }
          } else {
            keys.push(`old-${oldRevision}-${this.windowIdAllocator.allocate(interval.glyphIds)}`)
            // Issue #879 复核评论6104619192 问题1：交叉淡入淡出时，buildFrame 会额外输出新源窗口，
            // plannedWindowKeys 必须同时输出 new key，否则 sameKeySet 检查失败，候选无法提交。
            // Issue #879 复核评论6105482207 问题3：优先读取 per-interval needsCrossfade，
            // 整段总宽可能不变但单个字形宽高变化，needsCrossfade 必须按 interval 判断。
            const intervalNeedsCrossfade = interval.needsCrossfade !== undefined
              ? interval.needsCrossfade
              : move.needsCrossfade
            if (intervalNeedsCrossfade) {
              keys.push(`new-${newRevision}-${this.windowIdAllocator.allocate(interval.glyphIds)}`)
            }
          }
        }
      }
    }
    for (const ghost of plan.carryOverGhostRuns) {
      keys.push(`${ghost.sourceKind}-${ghost.sourceRevision}-${this.windowIdAllocator.allocate(ghost.glyphIds)}`)
    }
    for (const motionRun of plan.carryOverMotionRuns) {
      keys.push(`${motionRun.sourceKind}-${motionRun.sourceRevision}-${this.windowIdAllocator.allocate(motionRun.glyphIds)}`)
    }
    return keys
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
      // Issue #879 复核评论6088787701 问题4：
      // piece 分区（firstIndex/lastIndex/glyphIds）沿用 prepare 时的结果，只重算起始 clip。
      // 不再用提交瞬间的可见性重新拆分 piece——那会让同一字形换出不同 renderNodeKey，
      // 交棒前刚准备完的物理节点作废（旧 animator 已 cancel，新节点未布局）。
      const pieces: RunStartPiece[] = context === null
        ? run.pieces
        : retargetRunStarts(
            EditorMotionCoordinator.runGeometry(
              run.glyphIds, prepared.state.newSnapshot.text, run.utf16Start, run.utf16End, run.rect,
              prepared.state.newLayout
            ),
            run.pieces,
            context,
            'insert',
            run.rect.x,
            {
              startClipLeft: run.startClipLeft,
              startClipRight: run.startClipRight,
              startPositionX: run.rect.x,
              startPositionY: run.rect.y
            }
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
        ? run.pieces
        : retargetRunStarts(
            EditorMotionCoordinator.runGeometry(
              run.glyphIds, prepared.state.oldSnapshot.text, run.utf16Start, run.utf16End, run.rect,
              prepared.state.oldLayout
            ),
            run.pieces,
            context,
            'deleted',
            collapseX,
            {
              startClipLeft: collapseX,
              startClipRight: collapseX,
              startPositionX: run.rect.x,
              startPositionY: run.rect.y
            }
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
        ? move.pieces
        : retargetRunStarts(
            EditorMotionCoordinator.runGeometry(
              move.glyphIds, prepared.state.oldSnapshot.text, move.oldUtf16Start, move.oldUtf16End,
              move.oldRect, prepared.state.oldLayout
            ),
            move.pieces,
            context,
            'retained',
            move.oldRect.x,
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
        pieces: pieces,
        // Issue #879 复核评论6104619192 问题1：继承 needsCrossfade 标志，
        // 保证 retarget 后 plannedWindowKeys 仍与 buildFrame 输出 key 集合一致。
        needsCrossfade: move.needsCrossfade
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
      latestDisplayedFrame, prepared.state.newSnapshot.revision, planGlyphIds,
      prepared.state.glyphIdentities
    )

    // Issue #879 复核评论6081596024 问题1：识别跨事务延续的运动窗口（insert/reflow）。
    // 上一帧的 insert/reflow 窗口如果尚未完成动画，且其字形不属于本次新 plan 的任何 run，
    // 需要继承到新 plan 继续运动——避免未完成的吐字/保留字移动在新编辑时被丢弃导致跳字。
    const carryOverMotionRuns = this.identifyCarryOverMotionRuns(
      latestDisplayedFrame, prepared.state.newSnapshot.revision, prepared.state.newLayout, planGlyphIds,
      prepared.state.glyphIdentities
    )

    const newPlan: EditorMotionPlan = {
      insertRuns: newInsertRuns,
      deletedRuns: newDeletedRuns,
      retainedMoves: newRetainedMoves,
      caretPath: retargetedCaretPath,
      durationMs: prepared.plan.durationMs,
      motionKind: prepared.plan.motionKind,
      carryOverGhostRuns: carryOverGhostRuns,
      carryOverMotionRuns: carryOverMotionRuns
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
   * 从帧中提取所有运动窗口的 renderNodeKey（窗口逻辑 owner 同步用；frame 为 null 时返回空数组）。
   */
  static extractWindowKeys(frame: EditorVisualFrame | null): string[] {
    const keys: string[] = []
    if (frame !== null && frame.isAnimating) {
      for (let i = 0; i < frame.motionGlyphWindows.length; i++) {
        keys.push(frame.motionGlyphWindows[i].renderNodeKey)
      }
    }
    return keys
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

    // Issue #879 复核评论6084020082 问题4：在 prepareCandidate 中预生成 carry-over runs。
    // 用 displayedVisualFrame（startFrame）作为冻结帧近似——
    // retargetCandidate 中会用真实冻结帧替换近似值。
    // 这样 buildFrame 在 progress=0 时会为 carry-over runs 生成窗口，
    // prepared.startFrame 就包含了 carry-over 窗口。
    if (durationMs > 0 && startFrame.isAnimating) {
      const planGlyphIds: Set<string> = new Set()
      for (const run of plan.insertRuns) {
        for (const gid of run.glyphIds) {
          planGlyphIds.add(gid)
        }
      }
      for (const run of plan.deletedRuns) {
        for (const gid of run.glyphIds) {
          planGlyphIds.add(gid)
        }
      }
      for (const move of plan.retainedMoves) {
        for (const gid of move.glyphIds) {
          planGlyphIds.add(gid)
        }
      }
      plan.carryOverGhostRuns = this.identifyCarryOverGhosts(
        startFrame, newSnapshot.revision, planGlyphIds, identities.newTable
      )
      plan.carryOverMotionRuns = this.identifyCarryOverMotionRuns(
        startFrame, newSnapshot.revision, newLayout, planGlyphIds, identities.newTable
      )
    }

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
      carryOverGhostRuns: [],
      carryOverMotionRuns: []
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
  /**
   * Issue #879 本轮复核评论（高优先级）：把固定 piece 的起始状态规范成「按岛」的区间列表。
   *
   * retargetRunStarts 已为每个 piece 计算好所有不连续可见岛（RunStartInterval[]），
   * 这里直接复用；legacy 数据（无 intervals）回落到整 piece 单个区间。
   * 返回的区间列表至少有一个元素，buildFrame 据此为同一个物理 Text 生成多个裁切矩形（并集）。
   */
  private static pieceIntervals(piece: RunStartPiece): RunStartInterval[] {
    if (piece.intervals !== undefined && piece.intervals.length > 0) {
      return piece.intervals
    }
    return [{
      firstIndex: piece.firstIndex,
      lastIndex: piece.lastIndex,
      glyphIds: piece.glyphIds,
      startClipLeft: piece.startClipLeft,
      startClipRight: piece.startClipRight,
      startPositionX: piece.startPositionX,
      startPositionY: piece.startPositionY
    }]
  }

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
        plan.carryOverMotionRuns.length === 0 &&
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
        const intervals = EditorMotionCoordinator.pieceIntervals(piece)
        // 整个 piece 的 utf16 范围（window 的 utf16Start/End 与静态扣除都按整 piece）
        let pieceUtf16Start = run.utf16Start
        let pieceUtf16End = run.utf16End
        if (runBoundaries.length === run.glyphIds.length + 1 &&
            piece.firstIndex >= 0 && piece.lastIndex + 1 < runBoundaries.length) {
          pieceUtf16Start = runBoundaries[piece.firstIndex]
          pieceUtf16End = runBoundaries[piece.lastIndex + 1]
        }
        // Issue #879 复核评论6083352210 问题1：用 glyphRectForRange 取得 piece 自身的几何，
        // 而不是整 run 的 rect——否则拆成多 piece 时 glyphWidth 仍是整 run 宽度，
        // identifyCarryOverMotionRuns 判断 insert 完成时永远不满足。
        const pieceRect = glyphRectForRange(newLayout, pieceUtf16Start, pieceUtf16End)
        const pieceGlyphX = pieceRect !== null ? pieceRect.x : run.rect.x
        const pieceGlyphY = pieceRect !== null ? pieceRect.y : run.rect.y
        const pieceGlyphWidth = pieceRect !== null ? pieceRect.width : run.rect.width
        const pieceGlyphHeight = pieceRect !== null ? pieceRect.height : run.rect.height
        // Issue #879 本轮复核评论（高优先级）：一个固定 piece 物理 Text 可能覆盖多个不连续可见岛，
        // 每个岛各自从自己的起始裁切插值到自己的目标裁切，合成「多岛并集」clipRects。
        const clipRects: ClipRect[] = []
        let bboxLeft = 0
        let bboxRight = 0
        let bboxTop = pieceGlyphY
        let bboxBottom = pieceGlyphY + pieceGlyphHeight
        let firstRect = true
        for (const interval of intervals) {
          let iStart = pieceUtf16Start
          let iEnd = pieceUtf16End
          let targetLeft = run.rect.x
          let targetRight = run.rect.x + run.rect.width
          if (runBoundaries.length === run.glyphIds.length + 1 &&
              interval.firstIndex >= 0 && interval.lastIndex + 1 < runBoundaries.length) {
            iStart = runBoundaries[interval.firstIndex]
            iEnd = runBoundaries[interval.lastIndex + 1]
            // Issue #879 复核评论6082616112 问题4：start 用 Downstream、end 用 Upstream
            targetLeft = EditorMotionCoordinator.xAtOffsetLocal(newLayout, iStart, run.rect.x)
            targetRight = EditorMotionCoordinator.xAtOffsetLocal(newLayout, iEnd, run.rect.x + run.rect.width,
              CaretAffinity.Upstream)
          } else {
            targetLeft = interval.startClipLeft
            targetRight = interval.startClipRight
          }
          const currentLeft = interval.startClipLeft + (targetLeft - interval.startClipLeft) * p
          const currentRight = interval.startClipRight + (targetRight - interval.startClipRight) * p
          // 该岛自身的几何（top/bottom 可能跨行）
          const iRect = glyphRectForRange(newLayout, iStart, iEnd)
          const top = iRect !== null ? iRect.y : pieceGlyphY
          const height = iRect !== null ? iRect.height : pieceGlyphHeight
          clipRects.push({ x: currentLeft, y: top, width: Math.max(0, currentRight - currentLeft), height: height })
          if (firstRect) {
            bboxLeft = currentLeft
            bboxRight = currentRight
            bboxTop = top
            bboxBottom = top + height
            firstRect = false
          } else {
            bboxLeft = Math.min(bboxLeft, currentLeft)
            bboxRight = Math.max(bboxRight, currentRight)
            bboxTop = Math.min(bboxTop, top)
            bboxBottom = Math.max(bboxBottom, top + height)
          }
        }
        const windowInstanceId = allocator.allocate(piece.glyphIds)
        motionGlyphWindows.push({
          sourceKind: 'new',
          utf16Start: pieceUtf16Start,
          utf16End: pieceUtf16End,
          clipLeft: bboxLeft,
          clipRight: bboxRight,
          clipTop: bboxTop,
          clipBottom: bboxBottom,
          clipRects: clipRects,
          glyphX: pieceGlyphX,
          glyphY: pieceGlyphY,
          glyphWidth: pieceGlyphWidth,
          glyphHeight: pieceGlyphHeight,
          offsetX: 0,
          offsetY: 0,
          runId: `new-${newRevision}`,
          sourceRevision: newRevision,
          glyphIds: piece.glyphIds,
          windowId: run.windowId,
          windowInstanceId: windowInstanceId,
          renderNodeKey: `new-${newRevision}-${windowInstanceId}`,
          currentClipLeft: bboxLeft,
          currentClipRight: bboxRight,
          currentPositionX: pieceGlyphX,
          currentPositionY: pieceGlyphY,
          motionOwnerKind: 'insert',
          // Issue #879 评论6128658045 问题3：insert 通道的完整窗口数据就是当前 piece 的数据，
          // fullWindowGlyphUtf16Ranges 必须计算实际值，不再写 undefined。
          fullWindowGlyphIds: piece.glyphIds,
          fullWindowUtf16Start: pieceUtf16Start,
          fullWindowUtf16End: pieceUtf16End,
          fullWindowGlyphUtf16Ranges: EditorMotionCoordinator.buildGlyphUtf16Ranges(
            piece.glyphIds, pieceUtf16Start, pieceUtf16End, newText
          ),
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
        const intervals = EditorMotionCoordinator.pieceIntervals(piece)
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
        // Issue #879 本轮复核评论（高优先级）：每个可见岛各自从自己的起始裁切缩到 collapseX，
        // 合成「多岛并集」clipRects——同一固定 piece 物理 Text 不丢任何可见字形。
        const clipRects: ClipRect[] = []
        let bboxLeft = 0
        let bboxRight = 0
        let firstRect = true
        for (const interval of intervals) {
          const currentLeft = interval.startClipLeft + (targetLeft - interval.startClipLeft) * p
          const currentRight = interval.startClipRight + (targetRight - interval.startClipRight) * p
          clipRects.push({ x: currentLeft, y: run.rect.y, width: Math.max(0, currentRight - currentLeft), height: run.rect.height })
          if (firstRect) {
            bboxLeft = currentLeft
            bboxRight = currentRight
            firstRect = false
          } else {
            bboxLeft = Math.min(bboxLeft, currentLeft)
            bboxRight = Math.max(bboxRight, currentRight)
          }
        }
        const windowInstanceId = allocator.allocate(piece.glyphIds)
        motionGlyphWindows.push({
          sourceKind: 'old',
          utf16Start: pieceUtf16Start,
          utf16End: pieceUtf16End,
          clipLeft: bboxLeft,
          clipRight: bboxRight,
          clipTop: run.rect.y,
          clipBottom: run.rect.y + run.rect.height,
          clipRects: clipRects,
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
          currentClipLeft: bboxLeft,
          currentClipRight: bboxRight,
          currentPositionX: run.rect.x,
          currentPositionY: run.rect.y,
          motionOwnerKind: 'deleted',
          // Issue #879 评论6128658045 问题3：deleted 通道的完整窗口数据就是当前 piece 的数据，
          // fullWindowGlyphUtf16Ranges 必须计算实际值，不再写 undefined。
          fullWindowGlyphIds: piece.glyphIds,
          fullWindowUtf16Start: pieceUtf16Start,
          fullWindowUtf16End: pieceUtf16End,
          fullWindowGlyphUtf16Ranges: EditorMotionCoordinator.buildGlyphUtf16Ranges(
            piece.glyphIds, pieceUtf16Start, pieceUtf16End, oldText
          ),
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
    // Issue #879 缺口3修复：为每个岛创建独立的 MotionGlyphWindow。
    //    原来一个 piece（多岛）只创建一个 MotionGlyphWindow，clipRect 逐岛插值但
    //    offsetX/offsetY 是 piece 级统一值——裁切框位置变了，Text 内字形却没有随之独立移动。
    //    现在每个岛各自创建独立的 MotionGlyphWindow，拥有自己的 offset/clipRect/glyphIds，
    //    clipRect 和 Text 内字形位置一致。单岛 piece 行为与原来相同。
    // 问题2：冻结多字窗口重复 key 丢字——同一 renderNodeKey 的 frozen 成员
    // 跨多个 interval 只输出一个聚合 MotionGlyphWindow，合并 clipRects 和 UTF-16 范围。
    const frozenWindowAggregator = new Map<string, MotionGlyphWindow>()
    // Issue #879 评论6128658045 问题2：删除 conflictCounter/conflictKeyMap。
    // buildSourceMembers 已从源头保证 renderNodeKey 与 sourceRevision 一致
    // （从 sourceKind + sourceRevision + windowInstanceId 重建），
    // 不再需要在 buildFrame 中检测身份冲突或分配 -conflict-N 后缀。
    // Issue #879 复核评论6120195493：用 FrozenWindowGroupPlan 替代 frozenGroupDelta，
    // 窗口级属性从原始 FrozenWindowSpan/MotionGlyphWindow 选取，不由首成员抢占。
    const frozenGroupPlans = new Map<string, FrozenWindowGroupPlan>()

    // Issue #879 第32轮复核问题1：把 frozenMembersByKey 分组扫描提前到
    // retainedMoves 全部 pieces/intervals 之外。
    // 旧代码在 for(piece) 循环内新建 frozenMembersByKey，导致每个 piece 只看到自己的
    // frozen 成员。第一个 piece 可能只有隐藏成员（clipRects=[]、opacity=0），
    // 而第二个 piece 有可见成员，但 frozenGroupPlans.has(renderNodeKey) continue
    // 使得第二个 piece 的可见成员被跳过，group plan 用了隐藏成员的值。
    // 修复：在所有 retainedMoves 之前做一遍全局预扫描，收集所有 frozen 成员，
    // 为每个 renderNodeKey 选择第一个真正可见的成员（clipRects 有正面积且 opacity>0）。
    {
      const frozenMembersByKeyGlobal = new Map<string, FrozenMemberEntry[]>()

      for (const move of plan.retainedMoves) {
        const moveBoundariesScan = clusterBoundaries(oldText, move.oldUtf16Start, move.oldUtf16End)
        const newMoveBoundariesScan = clusterBoundaries(newText, move.newUtf16Start, move.newUtf16End)
        for (const piece of move.pieces) {
          // 计算 pieceNewX/Y（与循环内逻辑一致）
          let pieceNewXScan = move.newRect.x
          let pieceNewYScan = move.newRect.y
          if (newMoveBoundariesScan.length === move.glyphIds.length + 1 &&
              piece.firstIndex >= 0 && piece.lastIndex + 1 < newMoveBoundariesScan.length) {
            const pieceNewUtf16StartScan = newMoveBoundariesScan[piece.firstIndex]
            pieceNewXScan = EditorMotionCoordinator.xAtOffsetLocal(
              newLayout, pieceNewUtf16StartScan, move.newRect.x)
            pieceNewYScan = EditorMotionCoordinator.yAtOffsetLocal(
              newLayout, pieceNewUtf16StartScan, move.newRect.y)
          }
          const intervalsScan = EditorMotionCoordinator.pieceIntervals(piece)
          for (const interval of intervalsScan) {
            if (interval.sourceMembers === undefined) continue

            // 计算 interval 级别的目标位置（与循环内逻辑一致）
            let intervalNewGlyphRectScan: RectLike | null = null
            if (newMoveBoundariesScan.length === move.glyphIds.length + 1 &&
                interval.firstIndex >= 0 && interval.lastIndex + 1 < newMoveBoundariesScan.length) {
              const newIslandUtf16StartScan = newMoveBoundariesScan[interval.firstIndex]
              const newIslandUtf16EndScan = newMoveBoundariesScan[interval.lastIndex + 1]
              const newRectScan = glyphRectForRange(newLayout, newIslandUtf16StartScan, newIslandUtf16EndScan)
              if (newRectScan !== null) {
                intervalNewGlyphRectScan = newRectScan
              }
            }
            let intervalIslandNewXScan = pieceNewXScan
            let intervalIslandNewYScan = pieceNewYScan
            if (newMoveBoundariesScan.length === move.glyphIds.length + 1 &&
                interval.firstIndex >= 0 && interval.lastIndex + 1 < newMoveBoundariesScan.length) {
              const islandNewUtf16StartScan = newMoveBoundariesScan[interval.firstIndex]
              intervalIslandNewXScan = EditorMotionCoordinator.xAtOffsetLocal(
                newLayout, islandNewUtf16StartScan, move.newRect.x)
              intervalIslandNewYScan = EditorMotionCoordinator.yAtOffsetLocal(
                newLayout, islandNewUtf16StartScan, move.newRect.y)
            }
            const ownTargetXScan = intervalNewGlyphRectScan !== null
              ? intervalNewGlyphRectScan.x : intervalIslandNewXScan
            const ownTargetYScan = intervalNewGlyphRectScan !== null
              ? intervalNewGlyphRectScan.y : intervalIslandNewYScan

            for (const member of interval.sourceMembers) {
              const memberRoleScan = member.role !== undefined
                ? member.role
                : (member.sourceRevision === newRevision ? 'target' : 'frozen')
              if (memberRoleScan !== 'frozen') continue

              // Issue #879 评论6128658045 问题2：不再检测身份冲突或分配 -conflict-N 后缀。
              // buildSourceMembers 已从源头保证 renderNodeKey 与 sourceRevision 一致。
              // 直接使用 member.renderNodeKey 作为分组 key。
              const keyScan = member.renderNodeKey
              if (!frozenMembersByKeyGlobal.has(keyScan)) {
                frozenMembersByKeyGlobal.set(keyScan, [])
              }
              const entryScan: FrozenMemberEntry = {
                member: member,
                ownTargetX: ownTargetXScan,
                ownTargetY: ownTargetYScan,
                intervalGlyphIds: interval.glyphIds
              }
              frozenMembersByKeyGlobal.get(keyScan)!.push(entryScan)
            }
          }
        }
      }

      // 为每个 renderNodeKey 创建统一的 FrozenWindowGroupPlan
      // Issue #879 第32轮复核问题1：选择第一个真正可见的成员（clipRects 有正面积且 opacity>0）
      // 作为 baseMember。如果所有成员都隐藏，才用第一个成员的值。
      // Issue #879 评论6128658045 问题1+3：fullWindowGlyphIds/utf16Start/End/glyphUtf16Ranges
      // 从 baseMember 的 fullWindow* 字段取值，而不是从 intervalGlyphIds/sourceUtf16Start/End 取值。
      const frozenKeysGlobal = Array.from(frozenMembersByKeyGlobal.keys())
      for (const renderNodeKey of frozenKeysGlobal) {
        const entries = frozenMembersByKeyGlobal.get(renderNodeKey)!

        // 选择第一个真正可见的成员（clipRects 有正面积且 opacity>0）作为 baseMember
        let baseEntry = entries[0]
        for (const entry of entries) {
          const memberOpacity = entry.member.opacity !== undefined ? entry.member.opacity : 1
          const hasVisibleClip = entry.member.clipRects.some(
            (cr: RectLike): boolean => cr.width > 0 && cr.height > 0
          )
          if (hasVisibleClip && memberOpacity > 0) {
            baseEntry = entry
            break
          }
        }
        const baseMember = baseEntry.member

        const gpStartOffsetX = baseMember.offsetX
        const gpStartOffsetY = baseMember.offsetY
        const gpStartAlpha = baseMember.opacity !== undefined ? baseMember.opacity : 1
        const gpEndAlpha = baseMember.targetOpacity >= 0 ? baseMember.targetOpacity : 0
        const gpTargetOffsetX = baseEntry.ownTargetX - baseMember.sourceGlyphRect.x
        const gpTargetOffsetY = baseEntry.ownTargetY - baseMember.sourceGlyphRect.y
        const gpCurrentX = baseMember.sourceGlyphRect.x + gpStartOffsetX +
          (gpTargetOffsetX - gpStartOffsetX) * p
        const gpCurrentY = baseMember.sourceGlyphRect.y + gpStartOffsetY +
          (gpTargetOffsetY - gpStartOffsetY) * p
        const gpCurrentAlpha = gpStartAlpha + (gpEndAlpha - gpStartAlpha) * p

        // Issue #879 第32轮复核问题2：从 baseMember 的 fullWindow* 字段取值，
        // 而不是从 intervalGlyphIds/sourceUtf16Start/End/glyphUtf16Ranges 取值。
        // fullWindow* 字段保存了物理源窗口首次创建时的完整数据，
        // 确保 MotionGlyphWindow 携带完整字形列表和逐字映射。
        const fullWindowGlyphIds = baseMember.fullWindowGlyphIds !== undefined
          ? baseMember.fullWindowGlyphIds : baseEntry.intervalGlyphIds
        const fullWindowUtf16Start = baseMember.fullWindowUtf16Start !== undefined
          ? baseMember.fullWindowUtf16Start : baseMember.sourceUtf16Start
        const fullWindowUtf16End = baseMember.fullWindowUtf16End !== undefined
          ? baseMember.fullWindowUtf16End : baseMember.sourceUtf16End
        const fullWindowGlyphUtf16Ranges = baseMember.fullWindowGlyphUtf16Ranges !== undefined
          ? baseMember.fullWindowGlyphUtf16Ranges
          : (baseMember.glyphUtf16Ranges !== undefined ? baseMember.glyphUtf16Ranges : null)

        frozenGroupPlans.set(renderNodeKey, {
          startOffsetX: gpStartOffsetX,
          startOffsetY: gpStartOffsetY,
          startAlpha: gpStartAlpha,
          endAlpha: gpEndAlpha,
          targetOffsetX: gpTargetOffsetX,
          targetOffsetY: gpTargetOffsetY,
          currentX: gpCurrentX,
          currentY: gpCurrentY,
          currentAlpha: gpCurrentAlpha,
          renderNodeKey: renderNodeKey,
          runId: baseMember.runId,
          sourceRevision: baseMember.sourceRevision,
          sourceKind: baseMember.sourceKind,
          windowInstanceId: baseMember.windowInstanceId !== undefined
            ? baseMember.windowInstanceId : '',
          // Issue #879 评论6128658045 问题1+3：合并 original* 和 fullWindow* 为唯一字段，
          // 统一使用 fullWindow* 字段，不再保留冗余的 original* 字段。
          fullWindowGlyphIds: fullWindowGlyphIds,
          fullWindowUtf16Start: fullWindowUtf16Start,
          fullWindowUtf16End: fullWindowUtf16End,
          fullWindowGlyphUtf16Ranges: fullWindowGlyphUtf16Ranges,
        })
      }
    }

    for (const move of plan.retainedMoves) {
      const moveBoundaries = clusterBoundaries(oldText, move.oldUtf16Start, move.oldUtf16End)
      // 同时计算新正文的 clusterBoundaries——用于查出每个 piece 在新布局中的位置
      const newMoveBoundaries = clusterBoundaries(newText, move.newUtf16Start, move.newUtf16End)
      for (const piece of move.pieces) {
        // 计算 piece 的 utf16 范围（整 piece，用于参考和 fallback）
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
          // Issue #879 复核评论6082616112 问题4：start 用 Downstream、end 用 Upstream
          pieceOldLeft = EditorMotionCoordinator.xAtOffsetLocal(
            oldLayout, pieceUtf16Start, move.oldRect.x)
          pieceOldRight = EditorMotionCoordinator.xAtOffsetLocal(
            oldLayout, pieceUtf16End, move.oldRect.x + move.oldRect.width, CaretAffinity.Upstream)
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
        const pieceOldLineY = EditorMotionCoordinator.yAtOffsetLocal(
          oldLayout, pieceUtf16Start, move.oldRect.y)
        const pieceOldLineHeight = move.oldRect.height  // 行高通常整 move 一致

        // Issue #879 缺口3：为每个岛创建独立的 MotionGlyphWindow，
        // 每个窗口有自己的 offset/clipRect/glyphIds，clipRect 和字形位置一致。
        const intervals = EditorMotionCoordinator.pieceIntervals(piece)

        // Issue #879 第32轮复核问题1：frozenMembersByKey 分组扫描已提前到
        // retainedMoves 全部 pieces/intervals 之外，此处不再重新创建 group plan。
        // frozenGroupPlans 已在 for (const move) 循环之前预计算完成。

        for (const interval of intervals) {
          // 该岛在旧布局中的 utf16 范围
          let iStart = pieceUtf16Start
          let iEnd = pieceUtf16End
          if (moveBoundaries.length === move.glyphIds.length + 1 &&
              interval.firstIndex >= 0 && interval.lastIndex + 1 < moveBoundaries.length) {
            iStart = moveBoundaries[interval.firstIndex]
            iEnd = moveBoundaries[interval.lastIndex + 1]
          }
          // 该岛在旧布局中的几何（刚体平移前）
          const iRect = glyphRectForRange(oldLayout, iStart, iEnd)
          const islandOldLeft = iRect !== null ? iRect.x : pieceOldLeft
          const islandOldTop = iRect !== null ? iRect.y : pieceOldLineY
          const islandOldWidth = iRect !== null ? iRect.width : pieceOldWidth
          const islandOldHeight = iRect !== null ? iRect.height : pieceOldLineHeight
          // 该岛在新布局中的目标位置
          let islandNewX = pieceNewX
          let islandNewY = pieceNewY
          if (newMoveBoundaries.length === move.glyphIds.length + 1 &&
              interval.firstIndex >= 0 && interval.lastIndex + 1 < newMoveBoundaries.length) {
            const islandNewUtf16Start = newMoveBoundaries[interval.firstIndex]
            islandNewX = EditorMotionCoordinator.xAtOffsetLocal(
              newLayout, islandNewUtf16Start, move.newRect.x)
            islandNewY = EditorMotionCoordinator.yAtOffsetLocal(
              newLayout, islandNewUtf16Start, move.newRect.y)
          }
          // Issue #879 复核评论6102724233 问题2：startPositionX/Y 语义已变更为屏幕坐标系
          // 绝对位置（sourceGlyphX0 + frozenDrawOffsetX / sourceGlyphY0 + frozenDrawOffsetY），
          // 直接使用，不需要加减 islandOldLeft 或 oldRect.x/y。
          const islandStartX = interval.startPositionX
          const islandStartY = interval.startPositionY
          const islandCurrentX = islandStartX + (islandNewX - islandStartX) * p
          const islandCurrentY = islandStartY + (islandNewY - islandStartY) * p
          // 该岛的独立 offset——基于该岛的旧位置和当前插值位置
          const islandOffsetX = islandCurrentX - islandOldLeft
          const islandOffsetY = islandCurrentY - islandOldTop
          // Issue #879 复核评论6099070438 问题2：clip 宽度从 interval 的 startClipLeft/startClipRight
          // 插值——保留部分可见裁切宽度，不简单恢复 islandOldWidth 整段。
          // p=0 时 clip 宽度 = 当前裁切宽度（startClipRight - startClipLeft），
          // p=1 时 clip 宽度 = 该岛在新布局中的完整宽度（islandOldWidth）。
          const startClipWidth = Math.max(0, interval.startClipRight - interval.startClipLeft)
          const targetClipWidth = Math.max(0, islandOldWidth)
          // Issue #879 复核评论6099070438 问题4：如果 interval 有 clipRects（多段裁切子矩形），
          // 用 clipRects 作为该岛的裁切矩形——每段一个 ClipRect，位置随 offset 插值，
          // 宽度按各自在起始总宽度中的比例从起始宽度插值到目标宽度。
          // 否则回落到单个 islandClipRect（legacy 语义）。
          // Issue #879 复核评论6103606574 缺口1：新旧源交叉淡入淡出。
          // 当新旧字宽/高不同时，retained 通道同时输出旧源(opacity=1-p)和新源(opacity=p)两个窗口，
          // 避免字宽变化时 clip 放宽但旧 Text 实际排版宽高不变导致跳字。
          // Issue #879 复核评论6104619192 问题1：使用 move.needsCrossfade 标志（在 plan 阶段计算），
          // 不再逐 interval 动态判断——保证 plannedWindowKeys 和 buildFrame 输出 key 集合一致。
          // Issue #879 复核评论6105482207 问题3：优先读取 per-interval needsCrossfade，
          // 整段总宽可能不变但单个字形宽高变化，needsCrossfade 必须按 interval 判断。
          let needsSourceCrossfade = interval.needsCrossfade !== undefined
            ? interval.needsCrossfade
            : move.needsCrossfade
          let newSourceUtf16Start = iStart
          let newSourceUtf16End = iEnd
          let newSourceGlyphX = islandNewX
          let newSourceGlyphY = islandNewY
          let newSourceGlyphWidth = islandOldWidth
          let newSourceGlyphHeight = islandOldHeight
          const islandClipRects: ClipRect[] = []
          // Issue #879 评论6120849815 问题3：fallback 分支中区分 frozen/target 角色。
          // frozen（旧源窗口）不补间隙——只保留已可见碎片，不让被裁掉的旧笔画重新长出来。
          // target（新源交叉淡入淡出窗口）可以补间隙——从零面积渐显至完整目标。
          // islandClipRects 存放已可见碎片（frozen 和 target 共用的基础碎片），
          // targetIslandClipRects 存放间隙碎片（只有 target 窗口才使用）。
          const targetIslandClipRects: ClipRect[] = []
          // Issue #879 评论6108900966 问题1：优先检查 sourceMembers，逐成员独立计算。
          // 每个 FrozenGlyphMember 包含自己的 sourceGlyphRect/offsetX/offsetY/clipRects/opacity，
          // buildFrame 逐成员独立插值，不做笛卡尔混合。
          // 缺省时回落到旧字段（startScreenClipRects/frozenDrawOffsetX/Y/frozenSourceGlyphRects）。
          if (interval.sourceMembers !== undefined && interval.sourceMembers.length > 0) {
            // 计算 interval 级别的 newGlyphRect（所有成员共用）
            let intervalNewGlyphRect: RectLike | null = null
            if (newMoveBoundaries.length === move.glyphIds.length + 1 &&
                interval.firstIndex >= 0 && interval.lastIndex + 1 < newMoveBoundaries.length) {
              const newIslandUtf16Start = newMoveBoundaries[interval.firstIndex]
              const newIslandUtf16End = newMoveBoundaries[interval.lastIndex + 1]
              const newRect = glyphRectForRange(newLayout, newIslandUtf16Start, newIslandUtf16End)
              if (newRect !== null) {
                intervalNewGlyphRect = newRect
              }
            }

            // Issue #879 复核评论6111437799 问题2：newSourceUtf16Start/End 和 newSourceGlyphX/Y/Width/Height
            // 的计算不再受 needsSourceCrossfade 条件控制——只要拿到有效的 newMoveBoundaries 就独立计算。
            // needsSourceCrossfade 仅控制是否有必要额外切换文字形态，不能控制新源的字符范围。
            if (newMoveBoundaries.length === move.glyphIds.length + 1 &&
                interval.firstIndex >= 0 && interval.lastIndex + 1 < newMoveBoundaries.length) {
              newSourceUtf16Start = newMoveBoundaries[interval.firstIndex]
              newSourceUtf16End = newMoveBoundaries[interval.lastIndex + 1]
            }
            if (intervalNewGlyphRect !== null) {
              newSourceGlyphX = intervalNewGlyphRect.x
              newSourceGlyphY = intervalNewGlyphRect.y
              newSourceGlyphWidth = intervalNewGlyphRect.width
              newSourceGlyphHeight = intervalNewGlyphRect.height
            }

            // 问题2：构建 effective members——原始 sourceMembers + 可能新增的 newRevision 目标成员
            // 如果 interval 没有 sourceRevision === newRevision 的成员，创建一个新目标成员
            // （p=0 时 opacity=0、clipRects 为空，随动画推进逐渐淡入并显露）
            const effectiveMembers: FrozenGlyphMember[] = [...interval.sourceMembers]
            const hasNewRevisionMember = interval.sourceMembers.some(
              m => m.sourceRevision === newRevision
            )
            // Issue #879 复核评论6111437799 问题3：即使 intervalNewGlyphRect === null 也创建目标成员，
            // 用零面积 clip + opacity=0，保留稳定 key——与 plannedWindowKeys 的无条件输出对齐。
            if (!hasNewRevisionMember) {
              // Issue #879 复核评论6111437799 问题3：当 intervalNewGlyphRect === null 时用 fallback 几何
              const targetGlyphRect: RectLike = intervalNewGlyphRect !== null
                ? intervalNewGlyphRect
                : {
                    x: islandNewX,
                    y: islandNewY,
                    width: 0,
                    height: islandOldHeight,
                  }
              effectiveMembers.push({
                sourceKind: 'new',
                sourceRevision: newRevision,
                // Issue #879 复核评论6111437799 问题1：新建目标成员 role='target'，
                // 空clip时从零宽展开到完整字形。
                role: 'target',
                runId: `new-${newRevision}`,
                renderNodeKey: `new-${newRevision}`,
                sourceGlyphRect: {
                  x: targetGlyphRect.x,
                  y: targetGlyphRect.y,
                  width: targetGlyphRect.width,
                  height: targetGlyphRect.height,
                },
                offsetX: islandCurrentX - targetGlyphRect.x,
                offsetY: islandCurrentY - targetGlyphRect.y,
                clipRects: [],
                opacity: 0,
                sourceUtf16Start: newSourceUtf16Start,
                sourceUtf16End: newSourceUtf16End,
                targetOpacity: 1,
                // Issue #879 评论6116388578 问题2：target 成员的 windowInstanceId
                // 为 undefined——由 buildFrame 分配新的 windowInstanceId。
                windowInstanceId: undefined,
              })
            }

            // 问题5：逐成员计算位置插值和 clipRects——先求成员真实屏幕 Text 矩形，
            // 再以它为坐标基准生成 clip，不再独立映射到 newRevision 的不同宽高。
            // 形态变化由真正 newRevision 的目标 Text 淡入接管，历史源只移动、
            // 按自己实际几何裁切并淡出。
            // Issue #879 评论6128121842 问题3：提前分配 islandWindowInstanceId，
            // 供第一个 for 循环的回退逻辑和第二个 for 循环的窗口输出共同使用。
            const islandWindowInstanceId = allocator.allocate(interval.glyphIds)
            const memberClipRectsList: ClipRect[][] = []
            const memberCurrentXList: number[] = []
            const memberCurrentYList: number[] = []
            const memberOffsetXList: number[] = []
            const memberOffsetYList: number[] = []
            for (const member of effectiveMembers) {
              // 成员的源字形在屏矩形 = 成员的 sourceGlyphRect + 成员的 offset
              const memberSourceGlyphOnScreen: RectLike = {
                x: member.sourceGlyphRect.x + member.offsetX,
                y: member.sourceGlyphRect.y + member.offsetY,
                width: member.sourceGlyphRect.width,
                height: member.sourceGlyphRect.height
              }

              // 问题5：先计算位置插值——每帧成员的真实屏幕位置
              const memberStartScreenX = member.sourceGlyphRect.x + member.offsetX
              const memberStartScreenY = member.sourceGlyphRect.y + member.offsetY
              const memberRole = member.role !== undefined
                ? member.role
                : (member.sourceRevision === newRevision ? 'target' : 'frozen')
              // 该成员按自身 interval 目标求出的目标位置（target 成员直接使用）
              const ownTargetX = intervalNewGlyphRect !== null ? intervalNewGlyphRect.x : islandNewX
              const ownTargetY = intervalNewGlyphRect !== null ? intervalNewGlyphRect.y : islandNewY
              let memberTargetScreenX: number = ownTargetX
              let memberTargetScreenY: number = ownTargetY
              // Issue #879 评论6128658045 问题2：不再通过 conflictKeyMap 查找冲突 key。
              // buildSourceMembers 已保证 renderNodeKey 与 sourceRevision 一致，
              // frozen 成员直接使用 member.renderNodeKey。
              const memberRenderNodeKey = member.role === 'frozen'
                ? member.renderNodeKey
                : `${member.renderNodeKey}-${islandWindowInstanceId}`
              // Issue #879 复核评论6120195493：frozen 成员共享同一物理 Text 的窗口级属性。
              // 同 renderNodeKey 的所有成员用同一份窗口级 offset/alpha/position，
              // 不由首成员抢占。窗口级属性从原始 FrozenWindowSpan（通过 member 的
              // offsetX/offsetY/opacity 等字段）选取，确保隐藏成员不影响整张窗口位置/alpha。
                if (memberRole === 'frozen') {
                  // Issue #879 第32轮复核问题1：group plan 已在 retainedMoves 循环之前
                  // 全局预计算完成（跨所有 pieces/intervals 选可见 baseMember），此处直接使用。
                  // 仅当预计算遗漏时才回退到按当前 interval 的 effectiveMembers 创建。
                  let groupPlan = frozenGroupPlans.get(memberRenderNodeKey)
                  if (groupPlan === undefined) {
                    // 回退：按当前 interval 的 effectiveMembers 创建 group plan。
                    // 注意：此回退只在当前 interval 内搜索可见成员，可能不如预计算准确。
                    let baseMember = member
                    for (const m of effectiveMembers) {
                      const mKey = m.role === 'frozen'
                        ? m.renderNodeKey
                        : `${m.renderNodeKey}-${islandWindowInstanceId}`
                      if (mKey === memberRenderNodeKey &&
                          m.clipRects.length > 0) {
                        baseMember = m
                        break
                      }
                    }
                    const gpStartOffsetX = baseMember.offsetX
                    const gpStartOffsetY = baseMember.offsetY
                    const gpStartAlpha = baseMember.opacity !== undefined ? baseMember.opacity : 1
                    const gpEndAlpha = baseMember.targetOpacity >= 0 ? baseMember.targetOpacity : 0
                    const gpTargetOffsetX = ownTargetX - baseMember.sourceGlyphRect.x
                    const gpTargetOffsetY = ownTargetY - baseMember.sourceGlyphRect.y
                    const gpCurrentX = baseMember.sourceGlyphRect.x + gpStartOffsetX +
                      (gpTargetOffsetX - gpStartOffsetX) * p
                    const gpCurrentY = baseMember.sourceGlyphRect.y + gpStartOffsetY +
                      (gpTargetOffsetY - gpStartOffsetY) * p
                    const gpCurrentAlpha = gpStartAlpha + (gpEndAlpha - gpStartAlpha) * p

                    // Issue #879 第32轮复核问题2：从 baseMember 的 fullWindow* 字段取值
                    const fullWindowGlyphIdsFallback = baseMember.fullWindowGlyphIds !== undefined
                      ? baseMember.fullWindowGlyphIds : interval.glyphIds
                    const fullWindowUtf16StartFallback = baseMember.fullWindowUtf16Start !== undefined
                      ? baseMember.fullWindowUtf16Start : baseMember.sourceUtf16Start
                    const fullWindowUtf16EndFallback = baseMember.fullWindowUtf16End !== undefined
                      ? baseMember.fullWindowUtf16End : baseMember.sourceUtf16End
                    const fullWindowGlyphUtf16RangesFallback = baseMember.fullWindowGlyphUtf16Ranges !== undefined
                      ? baseMember.fullWindowGlyphUtf16Ranges
                      : (baseMember.glyphUtf16Ranges !== undefined ? baseMember.glyphUtf16Ranges : null)

                    groupPlan = {
                      startOffsetX: gpStartOffsetX,
                      startOffsetY: gpStartOffsetY,
                      startAlpha: gpStartAlpha,
                      endAlpha: gpEndAlpha,
                      targetOffsetX: gpTargetOffsetX,
                      targetOffsetY: gpTargetOffsetY,
                      currentX: gpCurrentX,
                      currentY: gpCurrentY,
                      currentAlpha: gpCurrentAlpha,
                      renderNodeKey: memberRenderNodeKey,
                      runId: member.runId,
                      sourceRevision: member.sourceRevision,
                      sourceKind: member.sourceKind,
                      windowInstanceId: member.windowInstanceId !== undefined
                        ? member.windowInstanceId : '',
                      // Issue #879 评论6128658045 问题1+3：合并 original* 和 fullWindow* 为唯一字段，
                      // 统一使用 fullWindow* 字段，不再保留冗余的 original* 字段。
                      fullWindowGlyphIds: fullWindowGlyphIdsFallback,
                      fullWindowUtf16Start: fullWindowUtf16StartFallback,
                      fullWindowUtf16End: fullWindowUtf16EndFallback,
                      fullWindowGlyphUtf16Ranges: fullWindowGlyphUtf16RangesFallback,
                    }
                    frozenGroupPlans.set(memberRenderNodeKey, groupPlan)
                  }
                // frozen 成员的目标位置 = sourceGlyphRect + groupPlan.targetOffset
                memberTargetScreenX = member.sourceGlyphRect.x + groupPlan.targetOffsetX
                memberTargetScreenY = member.sourceGlyphRect.y + groupPlan.targetOffsetY
              }
              let memberCurrentX: number
              let memberCurrentY: number
              if (memberRole === 'frozen' && member.clipRects.length === 0) {
                // Issue #879 复核评论6120195493：隐藏 frozen 成员保持起始屏幕位置，
                // 但 opacity 从 groupPlan.currentAlpha 取值，不由成员自己计算。
                memberCurrentX = memberStartScreenX
                memberCurrentY = memberStartScreenY
              } else {
                memberCurrentX = memberStartScreenX + (memberTargetScreenX - memberStartScreenX) * p
                memberCurrentY = memberStartScreenY + (memberTargetScreenY - memberStartScreenY) * p
              }
              const memberOffsetX = memberCurrentX - member.sourceGlyphRect.x
              const memberOffsetY = memberCurrentY - member.sourceGlyphRect.y
              memberCurrentXList.push(memberCurrentX)
              memberCurrentYList.push(memberCurrentY)
              memberOffsetXList.push(memberOffsetX)
              memberOffsetYList.push(memberOffsetY)

              // 问题5：p=1 时的真实屏幕 Text 矩形 = sourceGlyphRect + targetOffset
              // clip 以此为坐标基准（目标值），不再独立映射到 newRevision 的不同宽高。
              // 使用 p=1 时的值（而非当前帧 memberCurrentX/Y）确保 clip 插值是线性的，
              // 与 Text 位置插值同步——若用当前帧值，clip 插值变成 p 的二次函数，会滞后于 Text。
              // 形态变化由真正 newRevision 的目标 Text 淡入接管，
              // 历史源只移动、按自己实际几何裁切并淡出。
              const memberFinalScreenRect: RectLike = {
                x: memberTargetScreenX,
                y: memberTargetScreenY,
                width: member.sourceGlyphRect.width,
                height: member.sourceGlyphRect.height
              }

              // 用成员自己的 clipRects 求间隙
              const remaining = rectDifference(memberSourceGlyphOnScreen, member.clipRects)

              const startRects: ClipRect[] = []
              const targetRects: ClipRect[] = []

              if (member.clipRects.length === 0) {
                // Issue #879 复核评论6111437799 问题1：空 clip 有两种本质不同的语义：
                // ① target 角色（本次 newRevision 目标新源）——从零宽度逐渐扩展到完整字形
                // ② frozen 角色（历史源已被上一笔吞没/裁出屏幕）——保持不可见并淡出
                if (memberRole === 'target') {
                  // target：起始零面积，目标 = p=1 时真实屏幕 Text 矩形
                  startRects.push({
                    x: memberSourceGlyphOnScreen.x,
                    y: memberSourceGlyphOnScreen.y,
                    width: 0,
                    height: memberSourceGlyphOnScreen.height,
                  })
                  targetRects.push({
                    x: memberFinalScreenRect.x,
                    y: memberFinalScreenRect.y,
                    width: memberFinalScreenRect.width,
                    height: memberFinalScreenRect.height,
                  })
                } else {
                  // frozen：保持空 clip（startRects 和 targetRects 都是零面积），
                  // 透明度从当前值降到0——由 memberOpacity 插值控制，这里只保证几何不可见
                  startRects.push({
                    x: memberSourceGlyphOnScreen.x,
                    y: memberSourceGlyphOnScreen.y,
                    width: 0,
                    height: 0,
                  })
                  targetRects.push({
                    x: memberSourceGlyphOnScreen.x,
                    y: memberSourceGlyphOnScreen.y,
                    width: 0,
                    height: 0,
                  })
                }
              } else {
                // 已可见碎片：起始 = 成员的 clipRects（屏幕坐标）
                // 问题5：目标 = mapRectByRatio 映射到 p=1 时真实屏幕 Text 矩形（而非 newRevision 目标几何）
                for (const scr of member.clipRects) {
                  startRects.push({
                    x: scr.x, y: scr.y,
                    width: Math.max(0, scr.width), height: Math.max(0, scr.height)
                  })
                  const mapped = mapRectByRatio(scr, memberSourceGlyphOnScreen, memberFinalScreenRect)
                  targetRects.push({
                    x: mapped.x, y: mapped.y,
                    width: mapped.width, height: mapped.height
                  })
                }

                // 间隙碎片：零面积起点 + 按比例映射到 p=1 时真实屏幕 Text 矩形的间隙目标。
                // Issue #879 复核评论6119020043 问题3：只有 target 新源才允许把上一帧尚未
                // 显示的间隙「吐回来」。frozen 历史源上一笔已经吞掉/裁掉的部分必须继续隐藏，
                // 不能因为下一笔编辑又长出已经消失的笔画（否则旧字会短暂冒泡回屏）。
                if (memberRole === 'target') {
                  for (const gap of remaining) {
                    startRects.push({
                      x: gap.x, y: gap.y,
                      width: 0, height: Math.max(0, gap.height)
                    })
                    const mappedGap = mapRectByRatio(gap, memberSourceGlyphOnScreen, memberFinalScreenRect)
                    targetRects.push({
                      x: mappedGap.x, y: mappedGap.y,
                      width: mappedGap.width, height: mappedGap.height
                    })
                  }
                }
              }

              // 逐帧插值：每个碎片从起始位置/宽度插值到目标位置/宽度
              const memberClipRects: ClipRect[] = []
              for (let si = 0; si < startRects.length; si++) {
                const frameX = startRects[si].x + (targetRects[si].x - startRects[si].x) * p
                const frameWidth = startRects[si].width + (targetRects[si].width - startRects[si].width) * p
                const frameY = startRects[si].y + (targetRects[si].y - startRects[si].y) * p
                const frameHeight = startRects[si].height + (targetRects[si].height - startRects[si].height) * p
                memberClipRects.push({
                  x: frameX,
                  y: frameY,
                  width: Math.max(0, frameWidth),
                  height: Math.max(0, frameHeight),
                })
              }
              memberClipRectsList.push(memberClipRects)
            }

            // 包围盒 = 所有成员 clipRects 的并集
            let islandBboxLeft = Infinity
            let islandBboxRight = -Infinity
            let islandBboxTop = Infinity
            let islandBboxBottom = -Infinity
            for (const memberClipRects of memberClipRectsList) {
              for (const cr of memberClipRects) {
                islandBboxLeft = Math.min(islandBboxLeft, cr.x)
                islandBboxRight = Math.max(islandBboxRight, cr.x + cr.width)
                islandBboxTop = Math.min(islandBboxTop, cr.y)
                islandBboxBottom = Math.max(islandBboxBottom, cr.y + cr.height)
              }
            }
            if (islandBboxLeft === Infinity) {
              islandBboxLeft = islandCurrentX
              islandBboxRight = islandCurrentX
              islandBboxTop = islandCurrentY
              islandBboxBottom = islandCurrentY
            }

            // 逐成员输出独立的 MotionGlyphWindow
            // Issue #879 评论6128121842 问题3：islandWindowInstanceId 已在第一个 for 循环之前提前分配。
            for (let mi = 0; mi < effectiveMembers.length; mi++) {
              const member = effectiveMembers[mi]
              const memberClipRects = memberClipRectsList[mi]
              // Issue #879 评论6128658045 问题2：不再通过 conflictKeyMap 查找冲突 key。
              // frozen 成员直接使用 member.renderNodeKey，target 成员追加 islandWindowInstanceId。
              const memberRenderNodeKey = member.role === 'frozen'
                ? member.renderNodeKey
                : `${member.renderNodeKey}-${islandWindowInstanceId}`

              // 问题1：直接使用成员自己的冻结数据，不用 isOldSource 推导
              const memberGlyphX = member.sourceGlyphRect.x
              const memberGlyphY = member.sourceGlyphRect.y
              const memberGlyphWidth = member.sourceGlyphRect.width
              const memberGlyphHeight = member.sourceGlyphRect.height
              const memberUtf16Start = member.sourceUtf16Start
              const memberUtf16End = member.sourceUtf16End

              // 问题5：位置插值已在 Loop 1 中预计算，直接复用
              const memberCurrentX = memberCurrentXList[mi]
              const memberCurrentY = memberCurrentYList[mi]
              const memberOffsetX = memberOffsetXList[mi]
              const memberOffsetY = memberOffsetYList[mi]
              const memberRole = member.role !== undefined
                ? member.role
                : (member.sourceRevision === newRevision ? 'target' : 'frozen')

              // 问题3：透明度用 startOpacity→targetOpacity 插值。
              // Issue #879 复核评论6120195493：frozen 成员的 opacity 从 groupPlan.currentAlpha 取值，
              // 不由成员自己计算——同一物理 Text 只有一份窗口级 alpha，隐藏成员不得用自身
              // opacity=0 抢占整张 Text 的 alpha。target 成员仍各自计算。
              let memberOpacity: number
              if (memberRole === 'frozen') {
                // Issue #879 评论6128121842 问题3：使用 memberRenderNodeKey（可能包含冲突后缀）
                const groupPlan = frozenGroupPlans.get(memberRenderNodeKey)
                if (groupPlan !== undefined) {
                  memberOpacity = groupPlan.currentAlpha
                } else {
                  const memberEndOpacity = member.targetOpacity >= 0
                    ? member.targetOpacity
                    : 0
                  const memberStartOpacity = member.opacity !== undefined
                    ? member.opacity
                    : 1
                  memberOpacity = memberStartOpacity + (memberEndOpacity - memberStartOpacity) * p
                }
              } else {
                const memberEndOpacity = member.targetOpacity >= 0
                  ? member.targetOpacity
                  : 1
                const memberStartOpacity = member.opacity !== undefined
                  ? member.opacity
                  : 0
                memberOpacity = memberStartOpacity + (memberEndOpacity - memberStartOpacity) * p
              }

              // Issue #879 复核评论6120195493：frozen 成员的窗口级属性从 groupPlan 取值，
              // 不从成员自己计算。同一物理 Text 只有一份 offset/position/alpha。
              // target 成员仍使用各自的值。
              let windowOffsetX = memberOffsetX
              let windowOffsetY = memberOffsetY
              let windowCurrentX = memberCurrentX
              let windowCurrentY = memberCurrentY
              let windowUtf16Start = memberUtf16Start
              let windowUtf16End = memberUtf16End
              let windowGlyphIds = interval.glyphIds
              let windowGlyphUtf16Ranges: Map<string, [number, number]> | null | undefined = undefined
              // Issue #879 第32轮复核问题2：fullWindow* 字段——物理源窗口的完整数据
              let windowFullWindowGlyphIds: string[] | undefined = undefined
              let windowFullWindowUtf16Start: number | undefined = undefined
              let windowFullWindowUtf16End: number | undefined = undefined
              let windowFullWindowGlyphUtf16Ranges: Map<string, [number, number]> | null | undefined = undefined
              if (memberRole === 'frozen') {
                // Issue #879 评论6128121842 问题3：使用 memberRenderNodeKey（可能包含冲突后缀）
                const groupPlan = frozenGroupPlans.get(memberRenderNodeKey)
                if (groupPlan !== undefined) {
                  // 窗口级 offset = groupPlan 的插值 offset
                  windowOffsetX = groupPlan.startOffsetX +
                    (groupPlan.targetOffsetX - groupPlan.startOffsetX) * p
                  windowOffsetY = groupPlan.startOffsetY +
                    (groupPlan.targetOffsetY - groupPlan.startOffsetY) * p
                  // 窗口级当前位置 = groupPlan 的插值位置
                  windowCurrentX = groupPlan.currentX
                  windowCurrentY = groupPlan.currentY
                  // Issue #879 评论6128658045 问题1：保留原始完整 UTF-16 范围和 glyphIds，
                  // 使用 fullWindow* 字段（合并后的唯一可信来源），不用 min/max 和去重追加
                  windowUtf16Start = groupPlan.fullWindowUtf16Start
                  windowUtf16End = groupPlan.fullWindowUtf16End
                  windowGlyphIds = groupPlan.fullWindowGlyphIds
                  windowGlyphUtf16Ranges = groupPlan.fullWindowGlyphUtf16Ranges
                  // Issue #879 第32轮复核问题2：从 groupPlan 的 fullWindow* 字段取值
                  windowFullWindowGlyphIds = groupPlan.fullWindowGlyphIds
                  windowFullWindowUtf16Start = groupPlan.fullWindowUtf16Start
                  windowFullWindowUtf16End = groupPlan.fullWindowUtf16End
                  windowFullWindowGlyphUtf16Ranges = groupPlan.fullWindowGlyphUtf16Ranges
                }
              } else {
                // Issue #879 评论6128658045 问题1+3：target 成员的完整窗口数据就是当前 interval 的数据
                // target 成员本来就是新源窗口，interval 级别的数据就是完整的，
                // fullWindowGlyphUtf16Ranges 必须计算实际值，不再写 undefined。
                // 同时将 windowUtf16Start/End/glyphIds/glyphUtf16Ranges 也设置为完整窗口值，
                // 确保 MotionGlyphWindow.utf16Start/End 与 fullWindowUtf16Start/End 一致，
                // 下游 buildFrozenWindowPieces 不再因局部范围与完整 glyphIds 不匹配而返回 null。
                windowFullWindowGlyphIds = interval.glyphIds
                windowFullWindowUtf16Start = newSourceUtf16Start
                windowFullWindowUtf16End = newSourceUtf16End
                windowFullWindowGlyphUtf16Ranges = EditorMotionCoordinator.buildGlyphUtf16Ranges(
                  interval.glyphIds, newSourceUtf16Start, newSourceUtf16End, newText
                )
                // Issue #879 评论6128658045 问题1：将 fullWindow* 的值同步到 window* 变量，
                // 确保 MotionGlyphWindow 的 utf16Start/End/glyphIds/glyphUtf16Ranges
                // 与 fullWindow* 字段一致，下游不再需要 fallback 或重算。
                windowUtf16Start = newSourceUtf16Start
                windowUtf16End = newSourceUtf16End
                windowGlyphIds = interval.glyphIds
                windowGlyphUtf16Ranges = windowFullWindowGlyphUtf16Ranges
              }

              const memberWindow: MotionGlyphWindow = {
                sourceKind: member.sourceKind,
                utf16Start: windowUtf16Start,
                utf16End: windowUtf16End,
                clipLeft: islandBboxLeft,
                clipRight: islandBboxRight,
                clipTop: islandBboxTop,
                clipBottom: islandBboxBottom,
                clipRects: memberClipRects,
                glyphX: memberGlyphX,
                glyphY: memberGlyphY,
                glyphWidth: memberGlyphWidth,
                glyphHeight: memberGlyphHeight,
                offsetX: windowOffsetX,
                offsetY: windowOffsetY,
                runId: member.runId,
                sourceRevision: member.sourceRevision,
                glyphIds: windowGlyphIds,
                windowId: move.windowId,
                windowInstanceId: member.role === 'frozen' && member.windowInstanceId !== undefined
                  ? member.windowInstanceId
                  : islandWindowInstanceId,
                renderNodeKey: memberRenderNodeKey,
                currentClipLeft: islandBboxLeft,
                currentClipRight: islandBboxRight,
                currentPositionX: windowCurrentX,
                currentPositionY: windowCurrentY,
                motionOwnerKind: 'reflow',
                opacity: memberOpacity,
                glyphUtf16Ranges: windowGlyphUtf16Ranges,
                // Issue #879 第32轮复核问题2：物理源窗口的完整数据
                fullWindowGlyphIds: windowFullWindowGlyphIds,
                fullWindowUtf16Start: windowFullWindowUtf16Start,
                fullWindowUtf16End: windowFullWindowUtf16End,
                fullWindowGlyphUtf16Ranges: windowFullWindowGlyphUtf16Ranges,
              }
              // 问题2：frozen 成员按 renderNodeKey 聚合——同一物理 Text 节点覆盖多个字时，
              // 合并各字的 clipRects 到一个 MotionGlyphWindow，避免重复 key 丢字。
              // target 成员仍按 interval 独立分配 windowInstanceId，直接 push。
              if (memberRole === 'frozen') {
                const existing = frozenWindowAggregator.get(memberRenderNodeKey)
                if (existing !== undefined) {
                  // Issue #879 评论6128658045 问题2：buildSourceMembers 已从源头保证
                  // renderNodeKey 与 sourceRevision 一致，不再需要 -conflict-N 后缀或
                  // defensiveConflictKey。如果仍检测到同 key 但不同 runId/sourceRevision
                  // 的成员（不应发生），跳过该成员——不分配新 key，避免破坏 sameKeySet。
                  if (existing.runId !== memberWindow.runId ||
                      existing.sourceRevision !== memberWindow.sourceRevision) {
                    // 防御性检查：同 key 不同身份不应发生（上游 buildSourceMembers 已保证一致）。
                    // 跳过此成员，不分配新 key。
                    continue
                  } else {
                    // Issue #879 复核评论6120195493：只合并裁切和显示片段。
                    // offsetX/offsetY/opacity/currentPositionX/Y 已从 groupPlan 取值，不需要合并。
                    existing.clipRects = [...existing.clipRects, ...memberWindow.clipRects]
                    existing.clipLeft = Math.min(existing.clipLeft, memberWindow.clipLeft)
                    existing.clipRight = Math.max(existing.clipRight, memberWindow.clipRight)
                    existing.clipTop = Math.min(existing.clipTop, memberWindow.clipTop)
                    existing.clipBottom = Math.max(existing.clipBottom, memberWindow.clipBottom)
                    existing.currentClipLeft = Math.min(existing.currentClipLeft, memberWindow.currentClipLeft)
                    existing.currentClipRight = Math.max(existing.currentClipRight, memberWindow.currentClipRight)
                    // Issue #879 评论6128658045 问题1+3：从 group plan 更新 glyphUtf16Ranges、
                    // utf16Start/End 和 glyphIds，确保聚合后的窗口携带完整的逐字映射。
                    // 使用 fullWindow* 字段（合并后的唯一可信来源）。
                    const gp = frozenGroupPlans.get(memberRenderNodeKey)
                    if (gp !== undefined) {
                      existing.glyphUtf16Ranges = gp.fullWindowGlyphUtf16Ranges
                      existing.utf16Start = gp.fullWindowUtf16Start
                      existing.utf16End = gp.fullWindowUtf16End
                      existing.glyphIds = gp.fullWindowGlyphIds
                      // Issue #879 评论6128658045 问题1+3：聚合时也更新 fullWindow* 字段，
                      // 确保聚合窗口携带完整窗口数据供冻结索引使用。
                      existing.fullWindowGlyphIds = gp.fullWindowGlyphIds
                      existing.fullWindowUtf16Start = gp.fullWindowUtf16Start
                      existing.fullWindowUtf16End = gp.fullWindowUtf16End
                      existing.fullWindowGlyphUtf16Ranges = gp.fullWindowGlyphUtf16Ranges
                    }
                  }
                } else {
                  frozenWindowAggregator.set(memberRenderNodeKey, memberWindow)
                }
              } else {
                motionGlyphWindows.push(memberWindow)
              }
            }
            // sourceMembers 分支已处理 bbox 和窗口输出，跳过旧逻辑
            continue
          }
          // Issue #879 复核评论6102724233 问题2+3：优先使用 startScreenClipRects（屏幕坐标的
          // 真实裁切交集矩形），它包含真实的纵向裁切信息（y 和 height 反映裁切后的可见部分）。
          // 旧实现用 clipRects + frozenDrawOffset 现场重算起始 x/y，但 clipRects 的 y/height
          // 都是整字值（run.ownRect.y 和 run.ownRect.height），映射后的目标 y/height 也都是整字值，
          // 到 p=1 时所有碎片都变成整字高度，多张 mask 重叠。
          if (interval.startScreenClipRects !== undefined && interval.startScreenClipRects.length > 0 &&
              interval.sourceGlyphRects !== undefined && interval.sourceGlyphRects.length > 0) {
            // Issue #879 复核评论6102724233 问题2+3：startScreenClipRects 是屏幕坐标的
            // 真实裁切交集矩形，直接作为 p=0 的起始 clip。p>0 时用 mapRectByRatio 从
            // 源字形在屏矩形映射到目标字形矩形，每个碎片保留真实纵向子矩形。
            // Issue #879 复核评论6104161755 问题3：优先使用 frozenSourceGlyphRects（来自源布局坐标系），
            // 它与 frozenDrawOffsetX/Y（来自 FrozenWindowSpan.offsetX/Y）在同一坐标系。
            // sourceGlyphRects 来自 run.ownLayout，跨事务继承旧修订文字时坐标系不匹配。
            // Issue #879 复核评论6105482207 问题1：遍历 frozenSourceGlyphRects 的所有成员，
            // 为每个成员分别计算 sourceGlyphOnScreen 和 clipRects，合并到 islandClipRects。
            // 双源交叉淡入淡出时，同一 glyphId 可能同时被旧源和新源两个窗口覆盖，
            // 每个成员的几何对应一个 RectLike，需要分别计算。
            const frozenDrawOffsetX = interval.frozenDrawOffsetX !== undefined
              ? interval.frozenDrawOffsetX
              : interval.startPositionX - move.oldRect.x
            const frozenDrawOffsetY = interval.frozenDrawOffsetY !== undefined
              ? interval.frozenDrawOffsetY
              : interval.startPositionY - move.oldRect.y

            // 确定要遍历的源字形矩形列表：
            // Issue #879 评论6108234595：多成员场景应由 sourceMembers 分支处理，
            // fallback 分支只处理单成员场景，避免混合不同成员的 offset/clip。
            // 当 frozenSourceGlyphRects 有多个成员时只取第一个，不遍历所有成员。
            const sourceRectsToIterate: RectLike[] =
              interval.frozenSourceGlyphRects !== undefined && interval.frozenSourceGlyphRects.length > 0
                ? [interval.frozenSourceGlyphRects[0]]
                : (interval.sourceGlyphRects !== undefined && interval.sourceGlyphRects.length > 0
                   ? [interval.sourceGlyphRects[0]]
                   : [])

            // 先计算 interval 级别的 newGlyphRect（当 newMoveBoundaries 可用时），
            // 供 needsSourceCrossfade 和所有成员共用。当 newMoveBoundaries 不可用时，
            // 各成员使用自己的 sr 计算 fallback 值。
            let intervalNewGlyphRect: RectLike | null = null
            if (newMoveBoundaries.length === move.glyphIds.length + 1 &&
                interval.firstIndex >= 0 && interval.lastIndex + 1 < newMoveBoundaries.length) {
              const newIslandUtf16Start = newMoveBoundaries[interval.firstIndex]
              const newIslandUtf16End = newMoveBoundaries[interval.lastIndex + 1]
              const newRect = glyphRectForRange(newLayout, newIslandUtf16Start, newIslandUtf16End)
              if (newRect !== null) {
                intervalNewGlyphRect = newRect
              }
            }

            // Issue #879 复核评论6111437799 问题2：newSourceUtf16Start/End 和 newSourceGlyphX/Y/Width/Height
            // 的计算不再受 needsSourceCrossfade 条件控制——只要拿到有效的 newMoveBoundaries 就独立计算。
            if (newMoveBoundaries.length === move.glyphIds.length + 1 &&
                interval.firstIndex >= 0 && interval.lastIndex + 1 < newMoveBoundaries.length) {
              newSourceUtf16Start = newMoveBoundaries[interval.firstIndex]
              newSourceUtf16End = newMoveBoundaries[interval.lastIndex + 1]
            }
            if (intervalNewGlyphRect !== null) {
              newSourceGlyphX = intervalNewGlyphRect.x
              newSourceGlyphY = intervalNewGlyphRect.y
              newSourceGlyphWidth = intervalNewGlyphRect.width
              newSourceGlyphHeight = intervalNewGlyphRect.height
            } else {
              // fallback：用第一个成员的 sr 计算（与旧实现行为一致）
              const firstSr = sourceRectsToIterate[0]
              newSourceGlyphX = islandNewX + (firstSr.x - islandOldLeft)
              newSourceGlyphY = islandNewY
              newSourceGlyphWidth = Math.max(0, firstSr.width)
              newSourceGlyphHeight = Math.max(0, firstSr.height)
            }

            // 遍历所有成员，为每个成员分别计算 sourceGlyphOnScreen、newGlyphRect、clipRects
            for (const sr of sourceRectsToIterate) {
              // 源字形在屏矩形 = 源字形布局矩形 + 冻结窗口偏移（屏幕坐标系）
              const sourceGlyphOnScreen: RectLike = {
                x: sr.x + frozenDrawOffsetX,
                y: sr.y + frozenDrawOffsetY,
                width: sr.width,
                height: sr.height
              }

              // Issue #879 复核评论6103000047 问题4：目标字形几何使用新布局而不是旧布局。
              let newGlyphRect: RectLike
              if (intervalNewGlyphRect !== null) {
                newGlyphRect = intervalNewGlyphRect
              } else {
                newGlyphRect = {
                  x: islandNewX + (sr.x - islandOldLeft),
                  y: islandNewY,
                  width: Math.max(0, sr.width),
                  height: Math.max(0, sr.height)
                }
              }

              // Issue #879 复核评论6102724233 问题3：用 rectDifference 在屏幕坐标系中计算
              // 尚未覆盖的区域（间隙）。sourceGlyphOnScreen 和 startScreenClipRects 都在屏幕坐标。
              const remaining = rectDifference(sourceGlyphOnScreen, interval.startScreenClipRects)

              const startRects: ClipRect[] = []
              const targetRects: ClipRect[] = []

              // 已可见碎片：起始 = startScreenClipRects（屏幕坐标真实裁切矩形，含真实纵向裁切），
              // 目标 = mapRectByRatio 从源字形在屏矩形映射到新布局字形矩形
              for (const scr of interval.startScreenClipRects) {
                startRects.push({
                  x: scr.x, y: scr.y,
                  width: Math.max(0, scr.width), height: Math.max(0, scr.height)
                })
                // Issue #879 评论6116985734 问题4：clip 跟随 Text 平移，不按比例映射到不同宽高。
                // frozen 成员的 Text 只平移不变形，clip 应保持相对位置不变。
                // 当新旧布局字形宽高差异超过阈值时，由 needsSourceCrossfade 机制处理交叉淡入淡出。
                const mapped = mapRectByRatio(scr, sourceGlyphOnScreen, {
                  x: newGlyphRect.x, y: newGlyphRect.y,
                  width: sourceGlyphOnScreen.width, height: sourceGlyphOnScreen.height
                })
                targetRects.push({
                  x: mapped.x, y: mapped.y,
                  width: mapped.width, height: mapped.height
                })
              }

              // Issue #879 评论6120849815 问题3：间隙碎片只对 target 角色生成。
              // frozen 成员（旧源窗口）不补间隙——被裁掉的旧笔画不能重新长出来。
              // target 成员（新源交叉淡入淡出窗口）可以从零面积渐显至完整目标。
              const gapStartRects: ClipRect[] = []
              const gapTargetRects: ClipRect[] = []
              for (const gap of remaining) {
                gapStartRects.push({
                  x: gap.x, y: gap.y,
                  width: 0, height: Math.max(0, gap.height)
                })
                // Issue #879 评论6116985734 问题4：clip 跟随 Text 平移，不按比例映射到不同宽高。
                const mappedGap = mapRectByRatio(gap, sourceGlyphOnScreen, {
                  x: newGlyphRect.x, y: newGlyphRect.y,
                  width: sourceGlyphOnScreen.width, height: sourceGlyphOnScreen.height
                })
                gapTargetRects.push({
                  x: mappedGap.x, y: mappedGap.y,
                  width: mappedGap.width, height: mappedGap.height
                })
              }

              // 已可见碎片的逐帧插值 → islandClipRects（frozen 和 target 共用的基础碎片）
              for (let si = 0; si < startRects.length; si++) {
                const frameX = startRects[si].x + (targetRects[si].x - startRects[si].x) * p
                const frameWidth = startRects[si].width + (targetRects[si].width - startRects[si].width) * p
                const frameY = startRects[si].y + (targetRects[si].y - startRects[si].y) * p
                const frameHeight = startRects[si].height + (targetRects[si].height - startRects[si].height) * p
                islandClipRects.push({
                  x: frameX,
                  y: frameY,
                  width: Math.max(0, frameWidth),
                  height: Math.max(0, frameHeight),
                })
              }
              // 间隙碎片的逐帧插值 → targetIslandClipRects（只有 target 窗口才使用）
              for (let si = 0; si < gapStartRects.length; si++) {
                const frameX = gapStartRects[si].x + (gapTargetRects[si].x - gapStartRects[si].x) * p
                const frameWidth = gapStartRects[si].width + (gapTargetRects[si].width - gapStartRects[si].width) * p
                const frameY = gapStartRects[si].y + (gapTargetRects[si].y - gapStartRects[si].y) * p
                const frameHeight = gapStartRects[si].height + (gapTargetRects[si].height - gapStartRects[si].height) * p
                targetIslandClipRects.push({
                  x: frameX,
                  y: frameY,
                  width: Math.max(0, frameWidth),
                  height: Math.max(0, frameHeight),
                })
              }
            }
          } else if (interval.clipRects !== undefined && interval.clipRects.length > 0 &&
              interval.sourceGlyphRects !== undefined &&
              interval.sourceGlyphRects.length === interval.clipRects.length) {
            // Issue #879 复核评论6102724233 问题3：每块可见碎片不应各自独立插值到同一个
            // sourceGlyphRect（fullRect），否则 p=1 时多块重叠。采用 carry insert 同款逻辑：
            // 已可见碎片保持自身位置随平移移动（宽度不变），间隙碎片从零宽扩展到间隙宽度。
            // 所有碎片的位置随 islandStartX→islandNewX 平移。
            //
            // retained 通道的 interval 通常只覆盖一个字形（retainedMoveStartState 为每个
            // glyph 创建单字形 interval），sourceGlyphRects 中所有元素都相同。取第一个
            // sourceGlyphRect 作为整个字形的完整 rect。
            //
            // Issue #879 复核评论6102724233 问题3修复：对同一字形的多碎片，只有一份完整目标
            // 字形矩形（sourceGlyphRects[0]），多个 clipRect 是起始裁切并集。用 rectDifference
            // 计算尚未覆盖的区域（间隙），已可见碎片保持自身位置随平移移动（宽度不变），
            // 间隙碎片从零宽扩展到间隙宽度。不再对每个 clipRect 都用同一个 sourceGlyphRects[0]
            // 作为目标——那样会每个碎片各拥有一份完整目标，等于用两套 mask 分别扩满整字。
            // Issue #879 复核评论6105187411 问题3：第二分支统一使用 sourceGlyphRects[0]
            //（ownLayout 坐标系），不使用 frozenSourceGlyphRects[0]（源布局坐标系）。
            // frozenSourceGlyphRects 只用于第一分支（与 startScreenClipRects + frozenDrawOffsetX/Y
            // 配合使用）。第二分支的 clipRects 也是 ownLayout 坐标系，两者必须在同一坐标系。
            // 旧实现把 sr 换成 frozenSourceGlyphRects[0]，但 clipRects 仍由 buildPiece 投影到
            // run.ownLayout，导致 rectDifference(sr, clipRects) 混用两个坐标系，计算错位。
            const sr = interval.sourceGlyphRects[0]
            // Issue #879 复核评论6102724233 问题2：用 frozenDrawOffsetX/Y 替代
            // startPositionX/Y - oldRect.x/y，确保纵向半字不错位。
            const frozenDrawOffsetX = interval.frozenDrawOffsetX !== undefined
              ? interval.frozenDrawOffsetX
              : interval.startPositionX - move.oldRect.x
            const frozenDrawOffsetY = interval.frozenDrawOffsetY !== undefined
              ? interval.frozenDrawOffsetY
              : interval.startPositionY - move.oldRect.y

            // Issue #879 复核评论6103000047 问题4：目标字形几何使用新布局而不是旧布局。
            // 旧实现用 sr（旧布局坐标系中的源字形矩形）计算目标位置和宽度/高度，
            // 但新布局中同一 glyph 可能因排版宽度、软换行或样式变化而有不同目标尺寸。
            // 修复：用 newMoveBoundaries 和 glyphRectForRange(newLayout,...) 计算新布局中
            // 该岛/字形的完整矩形 newGlyphRect，把旧布局的 clipRects 和间隙按比例映射到新布局。
            let newGlyphRect: RectLike = {
              x: islandNewX + (sr.x - islandOldLeft),
              y: islandNewY,
              width: Math.max(0, sr.width),
              height: Math.max(0, sr.height)
            }
            if (newMoveBoundaries.length === move.glyphIds.length + 1 &&
                interval.firstIndex >= 0 && interval.lastIndex + 1 < newMoveBoundaries.length) {
              const newIslandUtf16Start = newMoveBoundaries[interval.firstIndex]
              const newIslandUtf16End = newMoveBoundaries[interval.lastIndex + 1]
              const newRect = glyphRectForRange(newLayout, newIslandUtf16Start, newIslandUtf16End)
              if (newRect !== null) {
                newGlyphRect = newRect
              }
            }

            // Issue #879 复核评论6111437799 问题2：newSourceUtf16Start/End 和 newSourceGlyphX/Y/Width/Height
            // 的计算不再受 needsSourceCrossfade 条件控制——只要拿到有效的 newMoveBoundaries 就独立计算。
            if (newMoveBoundaries.length === move.glyphIds.length + 1 &&
                interval.firstIndex >= 0 && interval.lastIndex + 1 < newMoveBoundaries.length) {
              newSourceUtf16Start = newMoveBoundaries[interval.firstIndex]
              newSourceUtf16End = newMoveBoundaries[interval.lastIndex + 1]
            }
            newSourceGlyphX = newGlyphRect.x
            newSourceGlyphY = newGlyphRect.y
            newSourceGlyphWidth = newGlyphRect.width
            newSourceGlyphHeight = newGlyphRect.height

            // Issue #879 复核评论6102724233 问题3+4：用 rectDifference 计算尚未覆盖的区域。
            // 与 carry insert 的找间隙逻辑共用同一个几何求差函数。
            // fullRectInRunCoords = sr（sourceGlyphRect，在 run 自己布局坐标系中）
            // clipRects 也在 run 自己布局坐标系中
            const remaining = rectDifference(sr, interval.clipRects)

            const startRects: ClipRect[] = []
            const targetRects: ClipRect[] = []

            // Issue #879 复核评论6103000047 问题4：已可见碎片的目标按比例映射到新布局。
            // 旧布局中相对于字形的位置比例 → 新布局中相对于新字形的位置。
            // 当新旧布局字形尺寸相同时，比例映射结果与旧实现一致（relX*newWidth = cr.x-islandOldLeft）。
            // 当新旧布局字形尺寸不同时（换行/字宽变化），目标几何正确匹配新布局。
            for (const cr of interval.clipRects) {
              startRects.push({
                x: cr.x + frozenDrawOffsetX, y: cr.y + frozenDrawOffsetY,
                width: Math.max(0, cr.width), height: Math.max(0, cr.height)
              })
              // Issue #879 评论6116985734 问题4：clip 跟随 Text 平移，不按比例映射到不同宽高。
              const mapped = mapRectByRatio(cr, sr, {
                x: newGlyphRect.x, y: newGlyphRect.y,
                width: sr.width, height: sr.height
              })
              targetRects.push({
                x: mapped.x, y: mapped.y,
                width: mapped.width, height: mapped.height
              })
            }

            // Issue #879 评论6120849815 问题3：间隙碎片只对 target 角色生成。
            // frozen 成员（旧源窗口）不补间隙——被裁掉的旧笔画不能重新长出来。
            // target 成员（新源交叉淡入淡出窗口）可以从零面积渐显至完整目标。
            const gapStartRects: ClipRect[] = []
            const gapTargetRects: ClipRect[] = []
            for (const gap of remaining) {
              gapStartRects.push({
                x: gap.x + frozenDrawOffsetX, y: gap.y + frozenDrawOffsetY,
                width: 0, height: Math.max(0, gap.height)
              })
              // Issue #879 评论6116985734 问题4：clip 跟随 Text 平移，不按比例映射到不同宽高。
              const mappedGap = mapRectByRatio(gap, sr, {
                x: newGlyphRect.x, y: newGlyphRect.y,
                width: sr.width, height: sr.height
              })
              gapTargetRects.push({
                x: mappedGap.x, y: mappedGap.y,
                width: mappedGap.width, height: mappedGap.height
              })
            }

            // 已可见碎片的逐帧插值 → islandClipRects（frozen 和 target 共用的基础碎片）
            for (let si = 0; si < startRects.length; si++) {
              const frameX = startRects[si].x + (targetRects[si].x - startRects[si].x) * p
              const frameWidth = startRects[si].width + (targetRects[si].width - startRects[si].width) * p
              const frameY = startRects[si].y + (targetRects[si].y - startRects[si].y) * p
              const frameHeight = startRects[si].height + (targetRects[si].height - startRects[si].height) * p
              islandClipRects.push({
                x: frameX,
                y: frameY,
                width: Math.max(0, frameWidth),
                height: Math.max(0, frameHeight),
              })
            }
            // 间隙碎片的逐帧插值 → targetIslandClipRects（只有 target 窗口才使用）
            for (let si = 0; si < gapStartRects.length; si++) {
              const frameX = gapStartRects[si].x + (gapTargetRects[si].x - gapStartRects[si].x) * p
              const frameWidth = gapStartRects[si].width + (gapTargetRects[si].width - gapStartRects[si].width) * p
              const frameY = gapStartRects[si].y + (gapTargetRects[si].y - gapStartRects[si].y) * p
              const frameHeight = gapStartRects[si].height + (gapTargetRects[si].height - gapStartRects[si].height) * p
              targetIslandClipRects.push({
                x: frameX,
                y: frameY,
                width: Math.max(0, frameWidth),
                height: Math.max(0, frameHeight),
              })
            }
          } else if (interval.clipRects !== undefined && interval.clipRects.length > 0) {
            // legacy：按比例分配（sourceGlyphRects 不存在或长度不匹配时回落）
            let totalStartWidth = 0
            for (const cr of interval.clipRects) {
              totalStartWidth += Math.max(0, cr.width)
            }
            const segCount = interval.clipRects.length
            for (const cr of interval.clipRects) {
              const segStartWidth = Math.max(0, cr.width)
              const segRatio = totalStartWidth > 0.01
                ? segStartWidth / totalStartWidth
                : 1 / segCount
              const segTargetWidth = segRatio * targetClipWidth
              const segCurrentWidth = segStartWidth + (segTargetWidth - segStartWidth) * p
              // 该段相对岛左边界的偏移（run 布局坐标系）
              const relX = cr.x - interval.startClipLeft
              // 起始/目标屏幕位置——整体随 islandStartX→islandNewX 平移
              const segStartX = islandStartX + relX
              const segTargetX = islandNewX + relX
              const segCurrentX = segStartX + (segTargetX - segStartX) * p
              islandClipRects.push({
                x: segCurrentX,
                y: islandCurrentY,
                width: Math.max(0, segCurrentWidth),
                height: Math.max(0, islandOldHeight),
              })
            }
          } else {
            const currentClipWidth = startClipWidth + (targetClipWidth - startClipWidth) * p
            islandClipRects.push({
              x: islandCurrentX,
              y: islandCurrentY,
              width: currentClipWidth,
              height: Math.max(0, islandOldHeight),
            })
          }
          // Issue #879 评论6120849815 问题3：区分 frozen/target 包围盒。
          // frozen（旧源窗口）的包围盒只基于已可见碎片（islandClipRects），
          // target（新源交叉淡入淡出窗口）的包围盒基于已可见碎片 + 间隙碎片。
          // 如果没有交叉淡入淡出（!needsSourceCrossfade），则只有旧源窗口，
          // 使用 frozen 包围盒（即 islandClipRects 的包围盒）。
          // 如果有交叉淡入淡出，旧源和新源各自有独立的 clipRects 和包围盒。
          const targetClipRects = targetIslandClipRects.length > 0
            ? [...islandClipRects, ...targetIslandClipRects]
            : islandClipRects

          // frozen 包围盒 = islandClipRects（已可见碎片）的并集
          let islandBboxLeft = Infinity
          let islandBboxRight = -Infinity
          let islandBboxTop = Infinity
          let islandBboxBottom = -Infinity
          for (const cr of islandClipRects) {
            islandBboxLeft = Math.min(islandBboxLeft, cr.x)
            islandBboxRight = Math.max(islandBboxRight, cr.x + cr.width)
            islandBboxTop = Math.min(islandBboxTop, cr.y)
            islandBboxBottom = Math.max(islandBboxBottom, cr.y + cr.height)
          }
          if (islandBboxLeft === Infinity) {
            islandBboxLeft = islandCurrentX
            islandBboxRight = islandCurrentX
            islandBboxTop = islandCurrentY
            islandBboxBottom = islandCurrentY
          }

          // target 包围盒 = targetClipRects（已可见碎片 + 间隙碎片）的并集
          let targetBboxLeft = Infinity
          let targetBboxRight = -Infinity
          let targetBboxTop = Infinity
          let targetBboxBottom = -Infinity
          for (const cr of targetClipRects) {
            targetBboxLeft = Math.min(targetBboxLeft, cr.x)
            targetBboxRight = Math.max(targetBboxRight, cr.x + cr.width)
            targetBboxTop = Math.min(targetBboxTop, cr.y)
            targetBboxBottom = Math.max(targetBboxBottom, cr.y + cr.height)
          }
          if (targetBboxLeft === Infinity) {
            targetBboxLeft = islandBboxLeft
            targetBboxRight = islandBboxRight
            targetBboxTop = islandBboxTop
            targetBboxBottom = islandBboxBottom
          }

          // 为该岛独立分配 windowInstanceId（用岛的 glyphIds 子序列）
          const islandWindowInstanceId = allocator.allocate(interval.glyphIds)
          // Issue #879 评论6120849815 问题3：旧源窗口（frozen 角色）使用 islandClipRects（不含间隙碎片）。
          // frozen 成员不补间隙——被裁掉的旧笔画不能重新长出来。
          motionGlyphWindows.push({
            sourceKind: 'old',
            utf16Start: iStart,
            utf16End: iEnd,
            clipLeft: islandBboxLeft,
            clipRight: islandBboxRight,
            clipTop: islandBboxTop,
            clipBottom: islandBboxBottom,
            clipRects: islandClipRects,
            glyphX: islandOldLeft,
            glyphY: islandOldTop,
            glyphWidth: islandOldWidth,
            glyphHeight: islandOldHeight,
            offsetX: islandOffsetX,
            offsetY: islandOffsetY,
            runId: `old-${oldRevision}`,
            sourceRevision: oldRevision,
            glyphIds: interval.glyphIds,
            windowId: move.windowId,
            windowInstanceId: islandWindowInstanceId,
            renderNodeKey: `old-${oldRevision}-${islandWindowInstanceId}`,
            currentClipLeft: islandBboxLeft,
            currentClipRight: islandBboxRight,
            currentPositionX: islandCurrentX,
            currentPositionY: islandCurrentY,
            motionOwnerKind: 'reflow',
            // Issue #879 复核评论6103606574 缺口1：新旧源交叉淡入淡出时旧源 opacity=1-p
            opacity: needsSourceCrossfade ? 1 - p : undefined,
            // Issue #879 评论6128658045 问题3：fallback 分支 frozen 窗口的完整窗口数据
            // 此分支没有 group plan，fullWindow* 就是当前 interval 的数据，
            // fullWindowGlyphUtf16Ranges 必须计算实际值，不再写 undefined。
            fullWindowGlyphIds: interval.glyphIds,
            fullWindowUtf16Start: iStart,
            fullWindowUtf16End: iEnd,
            fullWindowGlyphUtf16Ranges: EditorMotionCoordinator.buildGlyphUtf16Ranges(
              interval.glyphIds, iStart, iEnd, oldText
            ),
          })
          // Issue #879 复核评论6103606574 缺口1：新旧源交叉淡入淡出。
          // 当新旧字宽/高不同时，额外输出新源窗口(opacity=p)，与旧源互补。
          // p=0 时只有旧源可见(opacity=1)，新源不可见(opacity=0)；
          // p=1 时只有新源可见(opacity=1)，旧源不可见(opacity=0)；
          // 中间交叉淡入淡出，避免字宽变化时 clip 放宽但旧 Text 实际排版宽高不变导致跳字。
          // 新源窗口使用新正文 UTF-16 范围、新布局字形位置、offset=0（新源 Text 已在新布局位置）。
          // renderNodeKey 与旧源不同（sourceKind='new' + newRevision），ForEach 创建独立 Text 节点。
          // windowInstanceId 与旧源相同——WindowInstanceIdAllocator 通过 lease 机制安全管理多引用。
          // Issue #879 复核评论6104619192 问题2：新源窗口也跟随运动位置移动。
          // 旧实现将新源固定在终点（offsetX=0, currentPositionX=newSourceGlyphX），
          // 导致渐变不是"沿路径运动的同一个字由旧形状渐变成新形状"，
          // 而是"旧字跟着 clip 移动，新字一直待在终点被另一个地方的 clip 裁掉"。
          // 修复：新源也使用 islandCurrentX/Y 作为当前绘制位置，
          // offsetX/Y = islandCurrentX/Y - newSourceGlyphX/Y，
          // 使新源与旧源在屏幕上的 glyph 锚点相同，clipRects 可继续共用（屏幕坐标系）。
          if (needsSourceCrossfade) {
            // Issue #879 评论6120849815 问题3：新源窗口（target 角色）使用 targetClipRects（含间隙碎片）。
            // target 成员可以从零面积渐显至完整目标，补全被裁掉的区域。
            motionGlyphWindows.push({
              sourceKind: 'new',
              utf16Start: newSourceUtf16Start,
              utf16End: newSourceUtf16End,
              clipLeft: targetBboxLeft,
              clipRight: targetBboxRight,
              clipTop: targetBboxTop,
              clipBottom: targetBboxBottom,
              clipRects: targetClipRects,
              glyphX: newSourceGlyphX,
              glyphY: newSourceGlyphY,
              glyphWidth: newSourceGlyphWidth,
              glyphHeight: newSourceGlyphHeight,
              offsetX: islandCurrentX - newSourceGlyphX,
              offsetY: islandCurrentY - newSourceGlyphY,
              runId: `new-${newRevision}`,
              sourceRevision: newRevision,
              glyphIds: interval.glyphIds,
              windowId: move.windowId,
              windowInstanceId: islandWindowInstanceId,
              renderNodeKey: `new-${newRevision}-${islandWindowInstanceId}`,
              currentClipLeft: targetBboxLeft,
              currentClipRight: targetBboxRight,
              currentPositionX: islandCurrentX,
              currentPositionY: islandCurrentY,
              motionOwnerKind: 'reflow',
              opacity: p,
              // Issue #879 评论6128658045 问题3：fallback 分支 target 窗口的完整窗口数据，
              // fullWindowGlyphUtf16Ranges 必须计算实际值，不再写 undefined。
              fullWindowGlyphIds: interval.glyphIds,
              fullWindowUtf16Start: newSourceUtf16Start,
              fullWindowUtf16End: newSourceUtf16End,
              fullWindowGlyphUtf16Ranges: EditorMotionCoordinator.buildGlyphUtf16Ranges(
                interval.glyphIds, newSourceUtf16Start, newSourceUtf16End, newText
              ),
            })
          }
        }
      }
    }

    // 问题2：将按 renderNodeKey 聚合的 frozen 成员窗口推送到 motionGlyphWindows。
    // 每个 renderNodeKey 只输出一个 MotionGlyphWindow，合并了所有共享该物理 Text 节点的
    // frozen 成员的 clipRects 和 UTF-16 范围，避免 buildAllMotionWindowSources 去重时丢字。
    for (const frozenWindow of frozenWindowAggregator.values()) {
      motionGlyphWindows.push(frozenWindow)
    }

    // Issue #879 复核评论6078682695 问题2：carryOver ghost 残影采样。
    // 每帧同时采样本轮新 run 与 carryOver ghost，
    // 从 currentClipLeft/Right 插值到 targetClipLeft/Right（零宽度），
    // 保持其旧源 TextController/StyledString 有效，直到残影真正隐藏才释放。
    for (const ghost of plan.carryOverGhostRuns) {
      const currentLeft = ghost.currentClipLeft + (ghost.targetClipLeft - ghost.currentClipLeft) * p
      const currentRight = ghost.currentClipRight + (ghost.targetClipRight - ghost.currentClipRight) * p
      // Issue #879 缺口1：按岛插值——每岛从 clipRects[i] 插值到 islandTargetRects[i]（零宽度）
      const ghostClipRects: ClipRect[] = []
      for (let gi = 0; gi < ghost.clipRects.length; gi++) {
        const cr = ghost.clipRects[gi]
        const tr = ghost.islandTargetRects[gi]
        const frameLeft = cr.x + (tr.x - cr.x) * p
        const frameRight = cr.x + cr.width + (tr.x + tr.width - cr.x - cr.width) * p
        ghostClipRects.push({
          x: frameLeft,
          y: cr.y + (tr.y - cr.y) * p,
          width: Math.max(0, frameRight - frameLeft),
          height: cr.height + (tr.height - cr.height) * p
        })
      }
      const windowInstanceId = allocator.allocate(ghost.glyphIds)
      motionGlyphWindows.push({
        // Issue #879 复核评论6084803105 问题1：使用 ghost.sourceKind 替代硬编码 'old'
        sourceKind: ghost.sourceKind,
        utf16Start: ghost.utf16Start,
        utf16End: ghost.utf16End,
        clipLeft: currentLeft,
        clipRight: currentRight,
        clipTop: ghost.clipTop,
        clipBottom: ghost.clipBottom,
        clipRects: ghostClipRects,
        glyphX: ghost.glyphX,
        glyphY: ghost.glyphY,
        glyphWidth: ghost.glyphWidth,
        glyphHeight: ghost.glyphHeight,
        offsetX: ghost.offsetX,
        offsetY: ghost.offsetY,
        // Issue #879 复核评论6084803105 问题1：使用 ghost.sourceRunId 替代硬编码 `old-${ghost.sourceRevision}`
        runId: ghost.sourceRunId,
        sourceRevision: ghost.sourceRevision,
        glyphIds: ghost.glyphIds,
        windowId: ghost.windowId,
        windowInstanceId: windowInstanceId,
        // Issue #879 复核评论6084803105 问题1：使用 ghost.sourceKind 替代硬编码 'old'
        renderNodeKey: `${ghost.sourceKind}-${ghost.sourceRevision}-${windowInstanceId}`,
        currentClipLeft: currentLeft,
        currentClipRight: currentRight,
        currentPositionX: ghost.glyphX + ghost.offsetX,
        currentPositionY: ghost.glyphY + ghost.offsetY,
        motionOwnerKind: 'carryGhost',
        // Issue #879 评论6128658045 问题3：carryOver ghost 的完整窗口数据就是 ghost 自身的数据，
        // fullWindowGlyphUtf16Ranges 必须计算实际值，不再写 undefined。
        fullWindowGlyphIds: ghost.glyphIds,
        fullWindowUtf16Start: ghost.utf16Start,
        fullWindowUtf16End: ghost.utf16End,
        fullWindowGlyphUtf16Ranges: EditorMotionCoordinator.buildGlyphUtf16Ranges(
          ghost.glyphIds, ghost.utf16Start, ghost.utf16End,
          ghost.sourceKind === 'old' ? oldText : newText
        ),
        })
      }

    // Issue #879 复核评论6081596024 问题1：carryOver motion 延续窗口采样。
    // 上一帧的 insert/reflow 窗口如果尚未完成动画，继续从冻结状态向目标运动。
    // Issue #879 复核评论6082616112 修复：
    // - 问题1：runId 沿用 motionRun.sourceRunId，renderNodeKey 用 sourceKind-sourceRevision-windowInstanceId
    // - 问题2：reflow 每帧插值 currentPositionX/Y，并相应更新 offsetX/Y 和 clipLeft/Right/Top/Bottom
    // - 问题3：utf16Start/End 用 sourceUtf16Start/End（给旧 TextController 绘制的源范围）
    for (const motionRun of plan.carryOverMotionRuns) {
      const windowInstanceId = allocator.allocate(motionRun.glyphIds)
      if (motionRun.motionOwnerKind === 'reflow') {
        // reflow：每帧插值 currentPositionX/Y，并相应更新 offset/clip
        const currentX = motionRun.startX + (motionRun.targetX - motionRun.startX) * p
        const currentY = motionRun.startY + (motionRun.targetY - motionRun.startY) * p
        const currentOffsetX = currentX - motionRun.glyphX
        const currentOffsetY = currentY - motionRun.glyphY
        // reflow 的 clip 始终跟随当前位置
        const clipLeft = currentX
        const clipRight = currentX + motionRun.glyphWidth
        const clipTop = currentY
        const clipBottom = currentY + motionRun.glyphHeight
        // Issue #879 缺口1：按岛插值——每岛从 islandStartRects[i] 插值到 islandTargetRects[i]（新版布局位置）
        const reflowClipRects: ClipRect[] = []
        for (let ri = 0; ri < motionRun.clipRects.length; ri++) {
          const sr = motionRun.islandStartRects[ri]
          const tr = motionRun.islandTargetRects[ri]
          const frameLeft = sr.x + (tr.x - sr.x) * p
          const frameRight = sr.x + sr.width + (tr.x + tr.width - sr.x - sr.width) * p
          const frameTop = sr.y + (tr.y - sr.y) * p
          const frameBottom = sr.y + sr.height + (tr.y + tr.height - sr.y - sr.height) * p
          reflowClipRects.push({
            x: frameLeft,
            y: frameTop,
            width: Math.max(0, frameRight - frameLeft),
            height: Math.max(0, frameBottom - frameTop)
          })
        }
        motionGlyphWindows.push({
          sourceKind: motionRun.sourceKind,
          utf16Start: motionRun.sourceUtf16Start,
          utf16End: motionRun.sourceUtf16End,
          clipLeft: clipLeft,
          clipRight: clipRight,
          clipTop: clipTop,
          clipBottom: clipBottom,
          clipRects: reflowClipRects,
          glyphX: motionRun.glyphX,
          glyphY: motionRun.glyphY,
          glyphWidth: motionRun.glyphWidth,
          glyphHeight: motionRun.glyphHeight,
          offsetX: currentOffsetX,
          offsetY: currentOffsetY,
          runId: motionRun.sourceRunId,
          sourceRevision: motionRun.sourceRevision,
          glyphIds: motionRun.glyphIds,
          windowId: motionRun.windowId,
          windowInstanceId: windowInstanceId,
          renderNodeKey: `${motionRun.sourceKind}-${motionRun.sourceRevision}-${windowInstanceId}`,
          currentClipLeft: clipLeft,
          currentClipRight: clipRight,
          currentPositionX: currentX,
          currentPositionY: currentY,
          motionOwnerKind: motionRun.motionOwnerKind,
          // Issue #879 复核评论6104619192 问题3：继承源窗口的 opacity
          // Issue #879 复核评论6104942471 问题3：从冻结透明度继续插值到 targetOpacity。
          opacity: motionRun.opacity === undefined
            ? undefined
            : motionRun.opacity + ((motionRun.targetOpacity ?? motionRun.opacity) - motionRun.opacity) * p,
          // Issue #879 评论6128658045 问题3：carryOver motion reflow 的完整窗口数据，
          // fullWindowGlyphUtf16Ranges 必须计算实际值，不再写 undefined。
          fullWindowGlyphIds: motionRun.glyphIds,
          fullWindowUtf16Start: motionRun.sourceUtf16Start,
          fullWindowUtf16End: motionRun.sourceUtf16End,
          fullWindowGlyphUtf16Ranges: EditorMotionCoordinator.buildGlyphUtf16Ranges(
            motionRun.glyphIds, motionRun.sourceUtf16Start, motionRun.sourceUtf16End,
            motionRun.sourceKind === 'old' ? oldText : newText
          ),
        })
      } else {
        // insert：保持原来的 clip 插值逻辑（从 currentClipLeft/Right 插值到 targetClipLeft/Right）
        const currentLeft = motionRun.currentClipLeft + (motionRun.targetClipLeft - motionRun.currentClipLeft) * p
        const currentRight = motionRun.currentClipRight + (motionRun.targetClipRight - motionRun.currentClipRight) * p
        // Issue #879 缺口1：按岛插值——每岛从 islandStartRects[i] 插值到 islandTargetRects[i]（完整可见）
        // Issue #879 复核评论6101344441 问题2：循环上界用 islandStartRects.length 而非
        // clipRects.length，因为问题2修复后 islandStartRects 可能比 clipRects 长（增加了间隙碎片）。
        const insertClipRects: ClipRect[] = []
        for (let ii = 0; ii < motionRun.islandStartRects.length; ii++) {
          const sr = motionRun.islandStartRects[ii]
          const tr = motionRun.islandTargetRects[ii]
          const frameLeft = sr.x + (tr.x - sr.x) * p
          const frameRight = sr.x + sr.width + (tr.x + tr.width - sr.x - sr.width) * p
          insertClipRects.push({
            x: frameLeft,
            y: sr.y + (tr.y - sr.y) * p,
            width: Math.max(0, frameRight - frameLeft),
            height: sr.height + (tr.height - sr.height) * p
          })
        }
        motionGlyphWindows.push({
          sourceKind: motionRun.sourceKind,
          utf16Start: motionRun.sourceUtf16Start,
          utf16End: motionRun.sourceUtf16End,
          clipLeft: currentLeft,
          clipRight: currentRight,
          clipTop: motionRun.clipTop,
          clipBottom: motionRun.clipBottom,
          clipRects: insertClipRects,
          glyphX: motionRun.glyphX,
          glyphY: motionRun.glyphY,
          glyphWidth: motionRun.glyphWidth,
          glyphHeight: motionRun.glyphHeight,
          offsetX: motionRun.offsetX,
          offsetY: motionRun.offsetY,
          runId: motionRun.sourceRunId,
          sourceRevision: motionRun.sourceRevision,
          glyphIds: motionRun.glyphIds,
          windowId: motionRun.windowId,
          windowInstanceId: windowInstanceId,
          renderNodeKey: `${motionRun.sourceKind}-${motionRun.sourceRevision}-${windowInstanceId}`,
          currentClipLeft: currentLeft,
          currentClipRight: currentRight,
          currentPositionX: motionRun.currentPositionX,
          currentPositionY: motionRun.currentPositionY,
          motionOwnerKind: motionRun.motionOwnerKind,
          // Issue #879 复核评论6104619192 问题3：继承源窗口的 opacity
          // Issue #879 复核评论6104942471 问题3：从冻结透明度继续插值到 targetOpacity。
          opacity: motionRun.opacity === undefined
            ? undefined
            : motionRun.opacity + ((motionRun.targetOpacity ?? motionRun.opacity) - motionRun.opacity) * p,
          // Issue #879 评论6128658045 问题3：carryOver motion insert 的完整窗口数据，
          // fullWindowGlyphUtf16Ranges 必须计算实际值，不再写 undefined。
          fullWindowGlyphIds: motionRun.glyphIds,
          fullWindowUtf16Start: motionRun.sourceUtf16Start,
          fullWindowUtf16End: motionRun.sourceUtf16End,
          fullWindowGlyphUtf16Ranges: EditorMotionCoordinator.buildGlyphUtf16Ranges(
            motionRun.glyphIds, motionRun.sourceUtf16Start, motionRun.sourceUtf16End,
            motionRun.sourceKind === 'old' ? oldText : newText
          ),
        })
      }
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
   * Issue #879 复核评论6084020082 问题1+2+3：按源字形连续性拆分冻结窗口。
   *
   * 上一轮的"按 glyphId 分区"只拆了 glyphIds 数组，没有真正拆窗口——
   * 源范围、裁切、偏移仍是整窗口。本方法创建真正的按源字形连续性拆片。
   *
   * 输入：一个冻结窗口 w，uncoveredIds（需要 carry-over 的 glyphId 列表），
   * newIdentityTable（新版字形身份表），newLayout（新版行布局），windowIdAllocator。
   *
   * 按 sourceRevision 获取源排版记录（含 sourceText 和 sourceLayout），
   * 用 buildGlyphUtf16Ranges 获取每个 glyphId 的源 UTF-16 范围，
   * 将 uncoveredIds 按源 UTF-16 范围的连续性分组：
   * 相邻的 glyphId（源范围首尾相接）归为同一组。
   *
   * 每个连续组分别计算：
   * - 源 UTF-16：该组 glyphIds 在源正文中的 [minStart, maxEnd]
   * - 源矩形：用 glyphRectForRange(sourceLayout, groupSourceStart, groupSourceEnd)
   * - 冻结 clip：源矩形与原窗口 clip 的交集
   * - 偏移：沿用原窗口的 offsetX/Y
   * - 目标 UTF-16：用 newIdentityTable 查找该组每个 glyphId 在新版正文中的对应范围
   * - 目标矩形：用 glyphRectForRange(newLayout, groupTargetStart, groupTargetEnd)
   * - renderNodeKey：为每个 piece 分配独立的 renderNodeKey
   */
  private splitFrozenWindowByGlyphOwnership(
    w: MotionGlyphWindow,
    uncoveredIds: string[],
    newIdentityTable: GlyphIdentityTable | null,
    newLayout: LineLayout[]
  ): FrozenWindowPiece[] {
    // 用 sourceRevision 查源排版记录
    const sourceRecord = this.getSourceLayout(w.sourceRevision)
    let sourceText: string
    let sourceLayout: LineLayout[]
    if (sourceRecord !== null) {
      sourceText = sourceRecord.text
      sourceLayout = sourceRecord.lineLayouts
    } else {
      // fallback：缓存中没有该 revision，退回 sourceKind 猜测
      sourceText = w.sourceKind === 'old'
        ? (this.oldSnapshot?.text ?? '')
        : (this.newSnapshot?.text ?? '')
      sourceLayout = w.sourceKind === 'old' ? this.oldLayout : this.newLayout
    }

    // Issue #879 评论6128658045 问题1：优先使用窗口已携带的 fullWindowGlyphUtf16Ranges（完整映射），
    // 避免 buildGlyphUtf16Ranges 用局部范围 w.utf16Start/End + w.glyphIds 重算导致 null。
    // 上游 buildFrame 已保证 MotionGlyphWindow.utf16Start/End 与 fullWindowUtf16Start/End 一致，
    // 但仍优先使用完整映射以防遗漏。
    let glyphUtf16Ranges: Map<string, [number, number]> | null = null
    if (w.fullWindowGlyphUtf16Ranges !== undefined && w.fullWindowGlyphUtf16Ranges !== null) {
      glyphUtf16Ranges = new Map(w.fullWindowGlyphUtf16Ranges)
    } else if (w.glyphUtf16Ranges !== undefined && w.glyphUtf16Ranges !== null) {
      glyphUtf16Ranges = new Map(w.glyphUtf16Ranges)
    } else {
      // fallback：用 buildGlyphUtf16Ranges 重新计算
      const fwGlyphIds = w.fullWindowGlyphIds !== undefined ? w.fullWindowGlyphIds : w.glyphIds
      const fwStart = w.fullWindowUtf16Start !== undefined ? w.fullWindowUtf16Start : w.utf16Start
      const fwEnd = w.fullWindowUtf16End !== undefined ? w.fullWindowUtf16End : w.utf16End
      glyphUtf16Ranges = EditorMotionCoordinator.buildGlyphUtf16Ranges(
        fwGlyphIds, fwStart, fwEnd, sourceText
      )
    }

    // 如果 glyphUtf16Ranges 仍为 null（簇数不匹配），用 fullWindowUtf16Start/End 作为 fallback
    // 整个 uncoveredIds 作为一个组
    if (glyphUtf16Ranges === null) {
      const fwStart = w.fullWindowUtf16Start !== undefined ? w.fullWindowUtf16Start : w.utf16Start
      const fwEnd = w.fullWindowUtf16End !== undefined ? w.fullWindowUtf16End : w.utf16End
      return [this.buildFrozenWindowPiece(
        w, uncoveredIds, fwStart, fwEnd,
        sourceLayout, newIdentityTable, newLayout
      )]
    }

    // Issue #879 复核评论6085414957 问题1：按源+目标 UTF-16 范围连续性 + 源/目标视觉行相同分组。
    // 不再只按源/目标 range 相邻分组——还要检查视觉行。
    // 当一组字形跨软换行时，glyphRectForRange 只返回首行 rect，
    // 下一行的字形在动画中消失。因此行号变化就断组。
    // - (b) 类 glyph（新版仍存在）：源 range 相邻 + 目标 range 相邻 + 源视觉行相同 + 目标视觉行相同
    // - (c) 类 glyph（新版已删除）：单独成组，不与 (b) 类混合
    const sourceLineRanges: LineRange[] = sourceLayout.map((l: LineLayout): LineRange => ({
      start: l.startUtf16, end: l.endUtf16, breakKind: l.breakKind,
    }))
    const newLineRanges: LineRange[] = newLayout.map((l: LineLayout): LineRange => ({
      start: l.startUtf16, end: l.endUtf16, breakKind: l.breakKind,
    }))
    const groups: string[][] = []
    let currentGroup: string[] = []
    let currentGroupSourceEnd = -1
    let currentGroupTargetEnd = -1
    let currentGroupHasTarget = false
    let currentGroupSourceLineIdx = -1
    let currentGroupTargetLineIdx = -1
    for (const gid of uncoveredIds) {
      const range = glyphUtf16Ranges.get(gid)
      if (range === undefined) {
        // glyphId 不在映射中——单独成组
        if (currentGroup.length > 0) {
          groups.push(currentGroup)
          currentGroup = []
        }
        groups.push([gid])
        currentGroupSourceEnd = -1
        currentGroupTargetEnd = -1
        currentGroupHasTarget = false
        currentGroupSourceLineIdx = -1
        currentGroupTargetLineIdx = -1
        continue
      }
      const start = range[0]
      const end = range[1]

      // 查目标 range
      let targetEntry: GlyphIdentityEntry | null = null
      if (newIdentityTable !== null) {
        targetEntry = newIdentityTable.entryByGlyphId(gid)
      }

      // Issue #879 复核评论6084803105 问题3：(c) 类 glyph（新版已删除）单独成组，
      // 不与 (b) 类混合。调用方（identifyCarryOverMotionRuns）会跳过 targetResolved=false 的 piece，
      // identifyCarryOverGhosts 会接管它们。
      if (targetEntry === null) {
        // 新版已删除——单独成组
        if (currentGroup.length > 0) {
          groups.push(currentGroup)
          currentGroup = []
        }
        groups.push([gid])
        currentGroupSourceEnd = -1
        currentGroupTargetEnd = -1
        currentGroupHasTarget = false
        currentGroupSourceLineIdx = -1
        currentGroupTargetLineIdx = -1
        continue
      }

      // Issue #879 复核评论6085414957 问题1：计算源/目标视觉行索引。
      // 源视觉行用 range[0]（Downstream），目标视觉行用 targetEntry.utf16Start（Downstream）。
      // 行号变化即断组，避免 glyphRectForRange 跨行只返回首行 rect 导致下一行字形消失。
      const sourceLineIdx = resolveVisualLineIndex(sourceLineRanges,
        { utf16Offset: start, affinity: CaretAffinity.Downstream })
      const targetLineIdx = resolveVisualLineIndex(newLineRanges,
        { utf16Offset: targetEntry.utf16Start, affinity: CaretAffinity.Downstream })

      // (b) 类 glyph（新版仍存在）——检查四元组：
      // 源 range 相邻 + 目标 range 相邻 + 源视觉行相同 + 目标视觉行相同
      if (currentGroup.length === 0) {
        currentGroup = [gid]
        currentGroupSourceEnd = end
        currentGroupTargetEnd = targetEntry.utf16End
        currentGroupHasTarget = true
        currentGroupSourceLineIdx = sourceLineIdx
        currentGroupTargetLineIdx = targetLineIdx
      } else if (start === currentGroupSourceEnd &&
                 targetEntry.utf16Start === currentGroupTargetEnd &&
                 currentGroupHasTarget &&
                 sourceLineIdx === currentGroupSourceLineIdx &&
                 targetLineIdx === currentGroupTargetLineIdx) {
        // 源 range 相邻 + 目标 range 相邻 + 源视觉行相同 + 目标视觉行相同——归入当前组
        currentGroup.push(gid)
        currentGroupSourceEnd = end
        currentGroupTargetEnd = targetEntry.utf16End
      } else {
        // 任一条件不满足——新组
        groups.push(currentGroup)
        currentGroup = [gid]
        currentGroupSourceEnd = end
        currentGroupTargetEnd = targetEntry.utf16End
        currentGroupHasTarget = true
        currentGroupSourceLineIdx = sourceLineIdx
        currentGroupTargetLineIdx = targetLineIdx
      }
    }
    if (currentGroup.length > 0) {
      groups.push(currentGroup)
    }

    // 每个连续组分别计算 piece
    const pieces: FrozenWindowPiece[] = []
    for (const group of groups) {
      // 该组 glyphIds 在源正文中的 [minStart, maxEnd]
      let groupSourceStart = -1
      let groupSourceEnd = -1
      for (const gid of group) {
        const range = glyphUtf16Ranges.get(gid)
        if (range !== undefined) {
          const start = range[0]
          const end = range[1]
          if (groupSourceStart < 0 || start < groupSourceStart) {
            groupSourceStart = start
          }
          if (end > groupSourceEnd) {
            groupSourceEnd = end
          }
        }
      }
      // fallback：如果组中没有任何 glyphId 在映射中，用 w.utf16Start/End
      if (groupSourceStart < 0) {
        groupSourceStart = w.utf16Start
        groupSourceEnd = w.utf16End
      }

      pieces.push(this.buildFrozenWindowPiece(
        w, group, groupSourceStart, groupSourceEnd,
        sourceLayout, newIdentityTable, newLayout
      ))
    }
    return pieces
  }

  /**
   * Issue #879 复核评论6084020082：为每个连续组构建一个 FrozenWindowPiece。
   *
   * 源矩形来自源布局（sourceLayout），不是新版 targetRect。
   * 目标 UTF-16 只包含该连续组的目标范围，不把不相邻字压成一个 min/max。
   *
   * Issue #879 复核评论6085414957 问题1：调用方（splitFrozenWindowByGlyphOwnership）
   * 已保证同一组内的 glyph 属于同一源视觉行和同一目标视觉行，
   * 因此 glyphRectForRange 不会遇到跨行情况，不会只返回首行 rect。
   */
  private buildFrozenWindowPiece(
    w: MotionGlyphWindow,
    groupGlyphIds: string[],
    groupSourceStart: number,
    groupSourceEnd: number,
    sourceLayout: LineLayout[],
    newIdentityTable: GlyphIdentityTable | null,
    newLayout: LineLayout[]
  ): FrozenWindowPiece {
    // 源矩形：用 glyphRectForRange(sourceLayout, groupSourceStart, groupSourceEnd)
    const sourceRect = glyphRectForRange(sourceLayout, groupSourceStart, groupSourceEnd)
    const pieceGlyphX = sourceRect !== null ? sourceRect.x : w.glyphX
    const pieceGlyphY = sourceRect !== null ? sourceRect.y : w.glyphY
    const pieceGlyphWidth = sourceRect !== null ? sourceRect.width : w.glyphWidth
    const pieceGlyphHeight = sourceRect !== null ? sourceRect.height : w.glyphHeight

    // Issue #879 复核评论6084803105 问题2：源矩形加冻结偏移得到屏幕坐标再做交集。
    // sourceRect 是源排版原始坐标，w.currentClipLeft/Right 是屏幕坐标（已移动后的），
    // w.offsetX/Y 是二者间位移。必须先加偏移再与 currentClip 做交集。
    const onScreenX = pieceGlyphX + w.offsetX
    const onScreenY = pieceGlyphY + w.offsetY

    // 冻结 clip：屏幕坐标的源矩形与原窗口 clip 的二维交集
    const pieceClipLeft = Math.max(onScreenX, w.currentClipLeft)
    const pieceClipRight = Math.min(onScreenX + pieceGlyphWidth, w.currentClipRight)
    const pieceClipTop = Math.max(onScreenY, w.clipTop)
    const pieceClipBottom = Math.min(onScreenY + pieceGlyphHeight, w.clipBottom)

    // 偏移：沿用原窗口的 offsetX/Y
    const pieceOffsetX = w.offsetX
    const pieceOffsetY = w.offsetY

    // Issue #879 复核评论6084803105 问题2：子片段 currentPositionX/Y 使用自己的屏幕坐标，
    // 而非母窗口 w.currentPositionX/Y——同一母窗口被拆为几段时各 piece 的起点不同。
    const pieceCurrentPositionX = onScreenX
    const pieceCurrentPositionY = onScreenY

    // 目标 UTF-16：用 newIdentityTable 查找该组每个 glyphId 在新版正文中的对应范围
    let targetUtf16Start = -1
    let targetUtf16End = -1
    let targetResolved = false
    if (newIdentityTable !== null) {
      let foundStart = -1
      let foundEnd = -1
      for (const gid of groupGlyphIds) {
        const entry = newIdentityTable.entryByGlyphId(gid)
        if (entry !== null) {
          if (foundStart < 0 || entry.utf16Start < foundStart) {
            foundStart = entry.utf16Start
          }
          if (entry.utf16End > foundEnd) {
            foundEnd = entry.utf16End
          }
        }
      }
      if (foundStart >= 0 && foundEnd >= 0) {
        targetUtf16Start = foundStart
        targetUtf16End = foundEnd
        targetResolved = true
      }
    }

    // 目标矩形：用 glyphRectForRange(newLayout, groupTargetStart, groupTargetEnd)
    let targetRect: GlyphRect | null = null
    if (targetResolved) {
      targetRect = glyphRectForRange(newLayout, targetUtf16Start, targetUtf16End)
    }

    // 为每个 piece 分配独立的 renderNodeKey
    const windowInstanceId = this.windowIdAllocator.allocate(groupGlyphIds)
    const renderNodeKey = `${w.sourceKind}-${w.sourceRevision}-${windowInstanceId}`

    // Issue #879 评论6096633126 问题1：计算 piece 的多岛 clipRects。
    // 原窗口 w.clipRects 是屏幕坐标的多岛并集，piece 源矩形加 offset 后的屏幕坐标为
    // [onScreenX, onScreenX + pieceGlyphWidth] × [onScreenY, onScreenY + pieceGlyphHeight]。
    // piece 的 clipRects = 原窗口每个 clipRect 与 piece 源矩形（加 offset 后的屏幕坐标）的二维交集。
    // 交集为空（宽度或高度 ≤ 0）的 clipRect 不包含在结果中。
    // Issue #879 复核评论6097927249 问题1：所有交集都为空时保持空数组，
    // 明确表示"无可见交集"——不回退到包围盒，避免隐藏字重新出现。
    // FrozenWindowPiece.clipRects 允许为空数组，下游 buildWindowClipRects
    // 区分 undefined（旧数据兼容）与空数组（显式完全不可见）。
    const pieceClipRects: ClipRect[] = []
    for (const cr of w.clipRects) {
      const interLeft = Math.max(cr.x, onScreenX)
      const interRight = Math.min(cr.x + cr.width, onScreenX + pieceGlyphWidth)
      const interTop = Math.max(cr.y, onScreenY)
      const interBottom = Math.min(cr.y + cr.height, onScreenY + pieceGlyphHeight)
      if (interRight - interLeft > 0 && interBottom - interTop > 0) {
        pieceClipRects.push({
          x: interLeft,
          y: interTop,
          width: interRight - interLeft,
          height: interBottom - interTop
        })
      }
    }

    return {
      glyphIds: groupGlyphIds,
      sourceUtf16Start: groupSourceStart,
      sourceUtf16End: groupSourceEnd,
      sourceRect: sourceRect,
      // Issue #879 复核评论6084803105 问题2：glyphX/Y 保持源排版原始坐标（不加偏移），
      // 因为 buildFrame 中会用 glyphX + offsetX 来计算屏幕位置。
      glyphX: pieceGlyphX,
      glyphY: pieceGlyphY,
      glyphWidth: pieceGlyphWidth,
      glyphHeight: pieceGlyphHeight,
      // Issue #879 复核评论6084803105 问题2：clipLeft/Right/Top/Bottom 是屏幕坐标
      // （加了偏移后的），因为它们要与 currentClipLeft/Right 做交集。
      clipLeft: pieceClipLeft,
      clipRight: pieceClipRight,
      clipTop: pieceClipTop,
      clipBottom: pieceClipBottom,
      offsetX: pieceOffsetX,
      offsetY: pieceOffsetY,
      // Issue #879 复核评论6084803105 问题2：使用子片段自己的屏幕坐标
      currentPositionX: pieceCurrentPositionX,
      currentPositionY: pieceCurrentPositionY,
      targetUtf16Start: targetUtf16Start,
      targetUtf16End: targetUtf16End,
      targetResolved: targetResolved,
      targetRect: targetRect,
      renderNodeKey: renderNodeKey,
      windowInstanceId: windowInstanceId,
      clipRects: pieceClipRects
    }
  }

  /**
   * Issue #879 复核评论6078682695 问题2：识别 carryOver ghost 残影。
   *
   * 上一帧仍有非零 clip、且尚未归还静态层的旧窗口作为残影继承到候选 plan。
   *
   * Issue #879 复核评论6084020082 问题1+2+3：
   * - 用 splitFrozenWindowByGlyphOwnership 真正按源字形连续性拆分窗口，
   *   每个 piece 有独立的 utf16Start/End、glyphX/Y/Width/Height、currentClipLeft/Right
   * - 接受 insert/reflow 转来的 ghost：当 insert/reflow 窗口的字形在新版不存在时
   *   （newIdentityTable 查不到），将其转为 ghost 消退，不再直接丢弃
   * - 增加 newIdentityTable 参数，用于判断 insert/reflow 字形是否在新版正文中存在
   *
   * 残影的目标 clip = 零宽度（继续消退到完全隐藏）。
   *
   * @param latestDisplayedFrame 冻结时刻的 displayedVisualFrame
   * @param newRevision 新正文 revision
   * @param planGlyphIds 本轮新 plan 中所有 run 的 glyphIds 集合（用于排除正在处理的字形）
   * @param newIdentityTable 新版正文的字形身份表——用于判断 insert/reflow 字形是否在新版存在
   */
  private identifyCarryOverGhosts(
    latestDisplayedFrame: EditorVisualFrame,
    newRevision: number,
    planGlyphIds: Set<string>,
    newIdentityTable: GlyphIdentityTable | null
  ): CarryOverGhostRun[] {
    const ghosts: CarryOverGhostRun[] = []
    if (!latestDisplayedFrame.isAnimating) {
      return ghosts
    }
    for (const w of latestDisplayedFrame.motionGlyphWindows) {
      // 仍有非零 clip 宽度
      if (w.currentClipRight - w.currentClipLeft <= 0.01) {
        continue
      }

      // Issue #879 复核评论6084020082 问题2：接受 insert/reflow 转来的 ghost。
      // 当 insert/reflow 窗口的字形在新版不存在时（newIdentityTable 查不到），
      // 将其转为 ghost 消退，不再直接丢弃导致文字突然消失。
      if (w.motionOwnerKind === 'insert' || w.motionOwnerKind === 'reflow') {
        // 检查 uncoveredIds 是否在新版正文中存在
        const uncoveredIds: string[] = []
        for (const gid of w.glyphIds) {
          if (!planGlyphIds.has(gid)) {
            uncoveredIds.push(gid)
          }
        }
        if (uncoveredIds.length === 0) {
          continue
        }
        // 用 newIdentityTable 检查哪些字形在新版正文中不存在
        const ghostIds: string[] = []
        for (const gid of uncoveredIds) {
          if (newIdentityTable === null || newIdentityTable.entryByGlyphId(gid) === null) {
            ghostIds.push(gid)
          }
        }
        if (ghostIds.length === 0) {
          continue  // 所有 uncoveredIds 都在新版正文中存在——不转为 ghost
        }
        // Issue #879 复核评论6084020082 问题1+3：用 splitFrozenWindowByGlyphOwnership 拆分
        const pieces = this.splitFrozenWindowByGlyphOwnership(
          w, ghostIds, newIdentityTable, this.newLayout
        )
        for (const piece of pieces) {
          // Issue #879 复核评论6097927249 问题1：已不可见的 ghost 不得反向生成残影。
          // piece.clipRects 为空数组表示该 piece 在冻结帧完全不可见，跳过不生成 ghost。
          if (piece.clipRects.length === 0) {
            continue
          }
          // 目标 clip = 零宽度（继续消退到完全隐藏）
          const targetClipLeft = piece.clipLeft
          const targetClipRight = piece.clipLeft
          // Issue #879 缺口1：每岛目标裁切矩形——缩到自己的零宽度位置
          const islandTargetRects: ClipRect[] = piece.clipRects.map((cr: ClipRect): ClipRect => ({
            x: cr.x, y: cr.y, width: 0, height: cr.height
          }))
          ghosts.push({
            glyphIds: piece.glyphIds,
            windowId: w.windowId,
            sourceRevision: w.sourceRevision,
            // Issue #879 复核评论6084803105 问题1：从原窗口继承 sourceKind/sourceRunId
            sourceKind: w.sourceKind,
            sourceRunId: w.runId,
            utf16Start: piece.sourceUtf16Start,
            utf16End: piece.sourceUtf16End,
            currentClipLeft: piece.clipLeft,
            currentClipRight: piece.clipRight,
            targetClipLeft: targetClipLeft,
            targetClipRight: targetClipRight,
            clipTop: piece.clipTop,
            clipBottom: piece.clipBottom,
            glyphX: piece.glyphX,
            glyphY: piece.glyphY,
            glyphWidth: piece.glyphWidth,
            glyphHeight: piece.glyphHeight,
            offsetX: piece.offsetX,
            offsetY: piece.offsetY,
            clipRects: piece.clipRects,
            islandTargetRects: islandTargetRects
          })
        }
        continue
      }

      // 只处理 deleted 和 carryGhost——insert/reflow 已在上面处理
      if (w.motionOwnerKind !== 'deleted' && w.motionOwnerKind !== 'carryGhost') {
        continue
      }
      if (w.sourceRevision === newRevision) {
        continue
      }

      // 按 glyphId 分区
      const uncoveredIds: string[] = []
      for (const gid of w.glyphIds) {
        if (!planGlyphIds.has(gid)) {
          uncoveredIds.push(gid)
        }
      }
      // 全部被新 plan 接管——不需要继承残影
      if (uncoveredIds.length === 0) {
        continue
      }

      // Issue #879 复核评论6084020082 问题1+3：用 splitFrozenWindowByGlyphOwnership 拆分
      const pieces = this.splitFrozenWindowByGlyphOwnership(
        w, uncoveredIds, newIdentityTable, this.newLayout
      )
      for (const piece of pieces) {
        // Issue #879 复核评论6097927249 问题1：已不可见的 ghost 不得反向生成残影。
        // piece.clipRects 为空数组表示该 piece 在冻结帧完全不可见，跳过不生成 ghost。
        if (piece.clipRects.length === 0) {
          continue
        }
        // 目标 clip = 零宽度（继续消退到完全隐藏）
        const targetClipLeft = piece.clipLeft
        const targetClipRight = piece.clipLeft
        // Issue #879 缺口1：每岛目标裁切矩形——缩到自己的零宽度位置
        const islandTargetRects: ClipRect[] = piece.clipRects.map((cr: ClipRect): ClipRect => ({
          x: cr.x, y: cr.y, width: 0, height: cr.height
        }))
        ghosts.push({
          glyphIds: piece.glyphIds,
          windowId: w.windowId,
          sourceRevision: w.sourceRevision,
          // Issue #879 复核评论6084803105 问题1：从原窗口继承 sourceKind/sourceRunId
          sourceKind: w.sourceKind,
          sourceRunId: w.runId,
          utf16Start: piece.sourceUtf16Start,
          utf16End: piece.sourceUtf16End,
          currentClipLeft: piece.clipLeft,
          currentClipRight: piece.clipRight,
          targetClipLeft: targetClipLeft,
          targetClipRight: targetClipRight,
          clipTop: piece.clipTop,
          clipBottom: piece.clipBottom,
          glyphX: piece.glyphX,
          glyphY: piece.glyphY,
          glyphWidth: piece.glyphWidth,
          glyphHeight: piece.glyphHeight,
          offsetX: piece.offsetX,
          offsetY: piece.offsetY,
          clipRects: piece.clipRects,
          islandTargetRects: islandTargetRects
        })
      }
    }
    return ghosts
  }

  /**
   * Issue #879 复核评论6081596024 问题1：识别跨事务延续的运动窗口。
   *
   * 上一帧的 insert（吐字）或 reflow（保留字移动）窗口如果尚未完成动画，
   * 且其字形不属于本次新 plan 的任何 run，需要继承到新 plan 继续运动。
   *
   * - insert：继续从当前 clip 扩展到完整可见（targetClipLeft/Right = 完整字形范围）
   * - reflow：继续从当前位置移动到新版布局对应位置
   *
   * 与 identifyCarryOverGhosts 的区别：
   * - ghosts 处理 deleted/carryGhost（字形已不存在于新版正文，消退到零宽度）
   * - motion runs 处理 insert/reflow（字形仍存在于新版正文，继续向目标运动）
   *
   * Issue #879 复核评论6084020082 问题1+2+3：用 splitFrozenWindowByGlyphOwnership
   * 真正按源字形连续性拆分窗口，不再只拆 glyphIds 数组而保留整窗口的源范围/裁切/偏移。
   * 每个 piece 有独立的 sourceUtf16Start/End、glyphX/Y/Width/Height（来自源排版）、
   * targetUtf16Start/End（来自新版身份表），绝不用新版 targetRect 替代旧源字形基准。
   *
   * @param latestDisplayedFrame 冻结时刻的 displayedVisualFrame
   * @param newRevision 新正文 revision
   * @param newLayout 新版行布局
   * @param planGlyphIds 本轮新 plan 中所有 run 的 glyphIds 集合
   * @param newIdentityTable 新版正文的字形身份表——用于查找旧字形在新版正文中的对应 UTF-16 范围
   */
  private identifyCarryOverMotionRuns(
    latestDisplayedFrame: EditorVisualFrame,
    newRevision: number,
    newLayout: LineLayout[],
    planGlyphIds: Set<string>,
    newIdentityTable: GlyphIdentityTable | null
  ): CarryOverMotionRun[] {
    const runs: CarryOverMotionRun[] = []
    if (!latestDisplayedFrame.isAnimating) {
      return runs
    }
    // Issue #879 评论6108900966 问题4：按 windowId 分组计算完成状态。
    // 同一 glyph 的所有成员（sourceMembers 产生的多窗口）必须全部到达目标才认为该 glyph 运动完成。
    // 不能其中一个先结束、另一个还没交棒却让静态层重复绘制。
    const windowIdDoneMap = new Map<string, boolean>()
    for (const w of latestDisplayedFrame.motionGlyphWindows) {
      if (w.motionOwnerKind !== 'insert' && w.motionOwnerKind !== 'reflow') {
        continue
      }
      // 按 glyphId 分区：coveredIds（由新 plan 接手）和 uncoveredIds（需要 carry-over）
      const uncoveredIds: string[] = []
      for (const gid of w.glyphIds) {
        if (!planGlyphIds.has(gid)) {
          uncoveredIds.push(gid)
        }
      }
      // 全部被新 plan 接管——该窗口不需要延续，视为完成
      if (uncoveredIds.length === 0) {
        const prev = windowIdDoneMap.get(w.windowId)
        windowIdDoneMap.set(w.windowId, prev === undefined ? true : prev)
        continue
      }
      // 计算 pieces 并检查是否所有 piece 都已完成
      const pieces = this.splitFrozenWindowByGlyphOwnership(
        w, uncoveredIds, newIdentityTable, newLayout
      )
      let allPiecesDone = true
      for (const piece of pieces) {
        if (!piece.targetResolved) {
          allPiecesDone = false
          break
        }
        if (w.motionOwnerKind === 'insert') {
          const fullRect: RectLike = piece.sourceRect !== null
            ? {
              x: piece.sourceRect.x + piece.offsetX,
              y: piece.sourceRect.y + piece.offsetY,
              width: piece.sourceRect.width,
              height: piece.sourceRect.height
            }
            : {
              x: piece.glyphX + piece.offsetX,
              y: piece.glyphY + piece.offsetY,
              width: piece.glyphWidth,
              height: piece.glyphHeight
            }
          if (!isFullyCovered(fullRect, piece.clipRects)) {
            allPiecesDone = false
            break
          }
        } else {
          // reflow：检查 positionDone && opacityDone && clipDone
          if (piece.targetRect !== null) {
            const targetX = piece.targetRect.x
            const targetY = piece.targetRect.y
            const positionDone = Math.abs(piece.currentPositionX - targetX) < 0.01 &&
                Math.abs(piece.currentPositionY - targetY) < 0.01
            const opacityDone = w.opacity === undefined ||
                (w.sourceKind === 'old' ? w.opacity < 0.01 : w.opacity > 0.99)
            const targetFullRect: RectLike = {
              x: piece.targetRect.x,
              y: piece.targetRect.y,
              width: piece.targetRect.width,
              height: piece.targetRect.height
            }
            const clipDone = isFullyCovered(targetFullRect, piece.clipRects)
            if (!(positionDone && opacityDone && clipDone)) {
              allPiecesDone = false
              break
            }
          } else {
            allPiecesDone = false
            break
          }
        }
      }
      // 按 windowId 聚合：只有所有成员都完成时才认为该 windowId 完成
      const prev = windowIdDoneMap.get(w.windowId)
      windowIdDoneMap.set(w.windowId, prev === undefined ? allPiecesDone : (prev && allPiecesDone))
    }
    for (const w of latestDisplayedFrame.motionGlyphWindows) {
      // 只处理 insert 和 reflow——deleted/carryGhost 由 identifyCarryOverGhosts 处理
      if (w.motionOwnerKind !== 'insert' && w.motionOwnerKind !== 'reflow') {
        continue
      }
      // Issue #879 评论6108900966 问题4：同一 windowId 的所有成员都完成时才跳过
      if (windowIdDoneMap.get(w.windowId) === true) {
        continue
      }
      // 按 glyphId 分区：coveredIds（由新 plan 接手）和 uncoveredIds（需要 carry-over）
      const uncoveredIds: string[] = []
      for (const gid of w.glyphIds) {
        if (!planGlyphIds.has(gid)) {
          uncoveredIds.push(gid)
        }
      }
      // 全部被新 plan 接管——不需要延续
      if (uncoveredIds.length === 0) {
        continue
      }

      // Issue #879 复核评论6084020082 问题1+2+3：用 splitFrozenWindowByGlyphOwnership
      // 真正按源字形连续性拆分窗口。每个 piece 有独立的源/目标范围和几何。
      const pieces = this.splitFrozenWindowByGlyphOwnership(
        w, uncoveredIds, newIdentityTable, newLayout
      )

      // Issue #879 复核评论6082616112 问题1：明确源类型和源 runId。
      // Issue #879 复核评论6104619192 问题3：直接沿用 w.sourceKind，不再根据 motionOwnerKind 推断。
      // reflow 窗口的 sourceKind 可能是 'old' 或 'new'（交叉淡入淡出的新源窗口 motionOwnerKind='reflow'），
      // 强行推断为 'old' 会把新源窗口改名为 old-${newRevision}，指向不存在的旧版源。
      const sourceKind: 'new' | 'old' = w.sourceKind
      const sourceRunId = `${sourceKind}-${w.sourceRevision}`

      for (const piece of pieces) {
        // 问题2：目标身份消失的字形不生成 carry-over motion run——
        // 交给 identifyCarryOverGhosts 处理消退。
        if (!piece.targetResolved) {
          continue
        }

        // Issue #879 复核评论6102724233 问题1：区分 insert 和 reflow 的完成判断。
        if (w.motionOwnerKind === 'insert') {
          // Issue #879 复核评论6102724233 问题1：完成判定改为二维覆盖面积。
          // 旧实现用 clipRight - clipLeft >= fullWidth - 0.01 判断，
          // 但 clipLeft/Right 是整组窗口的左右界，不代表 clipRects[] 的真实并集。
          // 例：字形范围 [0,10]，上一帧只画 [0,3] 和 [7,10]，左右包围宽度恰好等于 10，
          // 中间 [3,7] 还没吐出来，却被 continue 直接丢掉。
          // 修复：先计算 pieceGlyphScreenRect = sourceRect + piece.offset（即 fullRect），
          // 再用 isFullyCovered 判断真实二维 clipRects 是否完全覆盖 fullRect。
          const fullRect: RectLike = piece.sourceRect !== null
            ? {
              x: piece.sourceRect.x + piece.offsetX,
              y: piece.sourceRect.y + piece.offsetY,
              width: piece.sourceRect.width,
              height: piece.sourceRect.height
            }
            : {
              x: piece.glyphX + piece.offsetX,
              y: piece.glyphY + piece.offsetY,
              width: piece.glyphWidth,
              height: piece.glyphHeight
            }
          if (isFullyCovered(fullRect, piece.clipRects)) {
            // Issue #879 评论6108900966 问题4：即使该 piece 已完成，
            // 也需要检查同一 windowId 的所有成员是否都完成。
            // 如果其他成员还未到达目标，不能提前结束该成员的运动。
            if (windowIdDoneMap.get(w.windowId) === true) {
              continue  // insert 已完全可见且所有成员都完成，不需要延续
            }
          }
        } else {
          // reflow：判断当前位置是否已接近目标位置
          if (piece.targetRect !== null) {
            const targetX = piece.targetRect.x
            const targetY = piece.targetRect.y
            const positionDone = Math.abs(piece.currentPositionX - targetX) < 0.01 &&
                Math.abs(piece.currentPositionY - targetY) < 0.01
            const opacityDone = w.opacity === undefined ||
                (w.sourceKind === 'old' ? w.opacity < 0.01 : w.opacity > 0.99)
            // Issue #879 复核评论6106658725 问题3：clip 完成检查——
            // reflow 的 clip 应完整覆盖目标字形矩形，否则字形部分不可见时不应提前结束。
            const targetFullRect: RectLike = {
              x: piece.targetRect.x,
              y: piece.targetRect.y,
              width: piece.targetRect.width,
              height: piece.targetRect.height
            }
            const clipDone = isFullyCovered(targetFullRect, piece.clipRects)
            if (positionDone && opacityDone && clipDone) {
              // Issue #879 评论6108900966 问题4：即使该 piece 已完成，
              // 也需要检查同一 windowId 的所有成员是否都完成。
              // 如果其他成员还未到达目标，不能提前结束该成员的运动。
              if (windowIdDoneMap.get(w.windowId) === true) {
                continue  // reflow 已到达目标位置、透明度已完成、clip 已完整覆盖且所有成员都完成
              }
            }
          }
        }

        // Issue #879 复核评论6082616112 问题2：计算 reflow 的 startX/startY/targetX/targetY。
        let startX = piece.currentPositionX
        let startY = piece.currentPositionY
        let targetX = piece.glyphX + piece.offsetX
        let targetY = piece.glyphY + piece.offsetY
        // 计算 targetClipLeft/Right：
        // - insert：目标是完整可见（targetClipLeft = glyphX, targetClipRight = glyphX + glyphWidth）
        // - reflow：目标是新版布局中的位置——用 piece.targetRect
        let targetClipLeft = piece.glyphX
        let targetClipRight = piece.glyphX + piece.glyphWidth
        if (w.motionOwnerKind === 'reflow') {
          // reflow 的目标位置在新版布局中——用 piece.targetRect
          if (piece.targetRect !== null) {
            targetX = piece.targetRect.x
            targetY = piece.targetRect.y
            targetClipLeft = targetX
            targetClipRight = targetX + piece.targetRect.width
          }
        }
        // Issue #879 缺口1：计算每岛的起始裁切矩形和目标裁切矩形
        // islandStartRects = piece.clipRects（冻结帧位置）
        let islandStartRects: ClipRect[] = piece.clipRects
        // islandTargetRects：根据通道类型计算每岛的目标裁切矩形
        let islandTargetRects: ClipRect[]
        // Issue #879 复核评论6099791364 问题2：clipRects 为空时 buildFrame 用
        // motionRun.clipRects.length 作为循环上界会跳过该 run，需要保证非空。
        let runClipRects: ClipRect[] = piece.clipRects
        if (w.motionOwnerKind === 'reflow') {
          // reflow：每岛目标是各自的新版布局位置
          // 每岛从冻结位置移动到新版布局位置——偏移量 = targetRect.x - currentPositionX
          if (piece.targetRect !== null) {
            const dx = piece.targetRect.x - piece.currentPositionX
            const dy = piece.targetRect.y - piece.currentPositionY
            islandTargetRects = piece.clipRects.map((cr: ClipRect): ClipRect => ({
              x: cr.x + dx,
              y: cr.y + dy,
              width: cr.width,
              height: cr.height
            }))
          } else {
            // fallback：无目标矩形时，目标 = 起始（不动）
            islandTargetRects = piece.clipRects.map((cr: ClipRect): ClipRect => ({
              x: cr.x, y: cr.y, width: cr.width, height: cr.height
            }))
          }
        } else {
          // insert：每岛目标是各自所属字形的完整可见矩形。
          // Issue #879 复核评论6099791364 问题2：旧实现直接拷贝 piece.clipRects 作为
          // islandTargetRects，与 islandStartRects 字段完全相同，导致 buildFrame 每帧
          // clipRects 不变，吐字动画停滞（半吐字停在半截直到结束才突然出现整字）。
          // 修复：目标矩形根据 piece 的源字形几何计算完整可见矩形，起始矩形保持冻结帧
          // 裁切，这样 buildFrame 逐帧从当前裁切插值到完整字形，吐字动画正常推进。
          // piece 按源字形连续性拆分，每个 piece 通常只有一个字形，目标矩形即完整字形
          // 可见矩形；若 sourceRect 不为 null 则优先用 sourceRect 的位置和大小。
          // Issue #879 复核评论6101344441 问题3：piece.sourceRect.x/y 和 piece.glyphX/Y
          // 是源排版坐标，但 piece.clipRects 是冻结帧屏幕坐标。fullRect 必须加
          // piece.offsetX/Y 才能转换到屏幕坐标系，否则间隙碎片和目标矩形位置错位。
          const fullRect: ClipRect = piece.sourceRect !== null
            ? {
              x: piece.sourceRect.x + piece.offsetX,
              y: piece.sourceRect.y + piece.offsetY,
              width: piece.sourceRect.width,
              height: piece.sourceRect.height
            }
            : {
              x: piece.glyphX + piece.offsetX,
              y: piece.glyphY + piece.offsetY,
              width: piece.glyphWidth,
              height: piece.glyphHeight
            }
          if (piece.clipRects.length > 0) {
            // Issue #879 复核评论6102724233 问题4：找间隙改为二维几何差集。
            // 旧实现按 piece.clipRects[0] 检查左边界，用相邻数组元素的 gapLeft=cr.x+cr.width、
            // gapRight=nextCr.x 寻找空洞，最后按 lastCr 补尾部。这隐含了两个未经保证的前提：
            // clipRects[] 已按 x 从左到右排好、每块矩形 y/height 相同且覆盖整行。
            // 它来源是冻结帧真实的二维裁切并集，不能保证按 x 排序，也可能在同一字形上下各露出一块。
            // 修复：用 rectDifference(fullRect, piece.clipRects) 计算剩余区域，
            // 剩余区域拆成互不重叠矩形，作为零面积起点 + 完整目标区间。
            // 已可见碎片保持自身位置不变（目标=起始），间隙碎片从零宽扩展到间隙宽度。
            const startRects: ClipRect[] = []
            const targetRects: ClipRect[] = []

            // Issue #879 复核评论6102724233 问题4：用 rectDifference 计算尚未覆盖的区域。
            const remaining = rectDifference(fullRect, piece.clipRects)

            // 已可见碎片：目标 = 起始（保持不动）
            for (const cr of piece.clipRects) {
              startRects.push({ x: cr.x, y: cr.y, width: cr.width, height: cr.height })
              targetRects.push({ x: cr.x, y: cr.y, width: cr.width, height: cr.height })
            }

            // 间隙碎片（rectDifference 返回的剩余区域）：零面积起点 + 完整目标区间
            for (const gap of remaining) {
              startRects.push({ x: gap.x, y: gap.y, width: 0, height: gap.height })
              targetRects.push({ x: gap.x, y: gap.y, width: gap.width, height: gap.height })
            }

            islandStartRects = startRects
            islandTargetRects = targetRects
            runClipRects = startRects
          } else {
            // 尚未吐出但在新正文仍存在：建立零宽度起点和非零目标
            // Issue #879 复核评论6102724233 问题4：piece.glyphX/glyphY 是源排版坐标，
            // 但 piece.clipRects 是冻结帧屏幕坐标。零面积起点应该使用
            // piece.glyphX + piece.offsetX 和 piece.glyphY + piece.offsetY（冻结屏幕字形原点），
            // 与已有部分可见分支处于同一坐标系。
            islandStartRects = [{
              x: piece.glyphX + piece.offsetX,
              y: piece.glyphY + piece.offsetY,
              width: 0,
              height: piece.glyphHeight
            }]
            islandTargetRects = [{
              x: fullRect.x,
              y: fullRect.y,
              width: fullRect.width,
              height: fullRect.height
            }]
            // 确保 clipRects 字段非空（buildFrame 用其 length 作为循环上界）
            runClipRects = islandStartRects
          }
        }
        runs.push({
          motionOwnerKind: w.motionOwnerKind,
          glyphIds: piece.glyphIds,
          windowId: w.windowId,
          sourceRevision: w.sourceRevision,
          sourceKind: sourceKind,
          sourceRunId: sourceRunId,
          sourceUtf16Start: piece.sourceUtf16Start,
          sourceUtf16End: piece.sourceUtf16End,
          targetUtf16Start: piece.targetUtf16Start,
          targetUtf16End: piece.targetUtf16End,
          currentClipLeft: piece.clipLeft,
          currentClipRight: piece.clipRight,
          targetClipLeft: targetClipLeft,
          targetClipRight: targetClipRight,
          clipTop: piece.clipTop,
          clipBottom: piece.clipBottom,
          glyphX: piece.glyphX,
          glyphY: piece.glyphY,
          glyphWidth: piece.glyphWidth,
          glyphHeight: piece.glyphHeight,
          offsetX: piece.offsetX,
          offsetY: piece.offsetY,
          currentPositionX: piece.currentPositionX,
          currentPositionY: piece.currentPositionY,
          startX: startX,
          startY: startY,
          targetX: targetX,
          targetY: targetY,
          clipRects: runClipRects,
          islandStartRects: islandStartRects,
          islandTargetRects: islandTargetRects,
          // Issue #879 复核评论6104619192 问题3：继承源窗口的 opacity，
          // 交叉淡入淡出的新源窗口被 carry-over 时保持透明度关系。
          opacity: w.opacity,
          // Issue #879 复核评论6104942471 问题3：从冻结透明度继续插值到目标——
          // 旧源成员继续淡出到 0，新源成员继续淡入到 1，保持互补关系（两者之和恒为 1）。
          // 否则两源会停在冻结时的半透明，动画结束时静态正文接管会闪烁/双字。
          targetOpacity: w.opacity === undefined
            ? undefined
            : (sourceKind === 'old' ? 0 : 1)
        })
      }
    }
    return runs
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

      // Issue #879 评论6128658045 问题1+3：合并 original* 和 fullWindow* 为唯一字段。
      // 上游已保证 fullWindowGlyphUtf16Ranges 不为 undefined（所有创建 MotionGlyphWindow 的地方
      // 都计算了实际值），不再 fallback 到 localGlyphUtf16Ranges。
      const fullWindowGlyphIds = w.fullWindowGlyphIds !== undefined
        ? w.fullWindowGlyphIds : w.glyphIds
      const fullWindowUtf16Start = w.fullWindowUtf16Start !== undefined
        ? w.fullWindowUtf16Start : w.utf16Start
      const fullWindowUtf16End = w.fullWindowUtf16End !== undefined
        ? w.fullWindowUtf16End : w.utf16End
      // Issue #879 评论6128658045 问题3：fullWindowGlyphUtf16Ranges 直接使用完整窗口映射，
      // 不再 fallback 到 localGlyphUtf16Ranges（上游已保证不为 undefined）。
      let fullWindowGlyphUtf16Ranges: Map<string, [number, number]> | null = null
      if (w.fullWindowGlyphUtf16Ranges !== undefined && w.fullWindowGlyphUtf16Ranges !== null) {
        fullWindowGlyphUtf16Ranges = new Map(w.fullWindowGlyphUtf16Ranges)
      } else {
        // 防御性 fallback：上游应保证不为 undefined，但此处仍保留计算以防遗漏
        fullWindowGlyphUtf16Ranges = EditorMotionCoordinator.buildGlyphUtf16Ranges(
          fullWindowGlyphIds, fullWindowUtf16Start, fullWindowUtf16End, sourceText
        )
      }

      // Issue #879 评论6128658045 问题1+3：glyphUtf16Ranges 直接使用完整窗口映射，
      // 消除 glyphUtf16Ranges 和 fullWindowGlyphUtf16Ranges 两套来源的矛盾。
      const glyphUtf16Ranges = fullWindowGlyphUtf16Ranges

      frozenWindows.push({
        // Issue #879 评论6128121842：glyphIds 也优先使用完整窗口数据，保持语义一致
        glyphIds: fullWindowGlyphIds,
        clipLeft: w.currentClipLeft,
        clipRight: w.currentClipRight,
        offsetX: w.offsetX,
        offsetY: w.offsetY,
        // Issue #879 评论6096421590 问题2：保留多岛 clipRects，供 clusterVisible/buildPiece 逐岛求交
        clipRects: w.clipRects.map((r: ClipRect): RectLike => ({ x: r.x, y: r.y, width: r.width, height: r.height })),
        sourceRevision: w.sourceRevision,
        glyphUtf16Ranges: glyphUtf16Ranges,
        sourceLayout: sourceLayout,
        // Issue #879 复核评论6105482207 问题1：冻结新字段，供双源交叉淡入淡出使用
        sourceKind: w.sourceKind,
        runId: w.runId,
        renderNodeKey: w.renderNodeKey,
        opacity: w.opacity,
        windowInstanceId: w.windowInstanceId,
        // Issue #879 第32轮复核问题2：从 MotionGlyphWindow 的 fullWindow* 字段读取完整窗口数据
        fullWindowGlyphIds: fullWindowGlyphIds,
        fullWindowUtf16Start: fullWindowUtf16Start,
        fullWindowUtf16End: fullWindowUtf16End,
        fullWindowGlyphUtf16Ranges: fullWindowGlyphUtf16Ranges,
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
      carryOverGhostRuns: [],
      carryOverMotionRuns: []
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
   * Issue #879 复核评论6082616112 问题4：添加 affinity 参数。
   * 默认 Downstream（软换行边界属于下一行），但 end offset 应传 Upstream
   * （结束位置属于当前行末，而非下一行行首）。
   *
   * buildFrame 按 piece 采样时需要计算 piece 的目标 clip（该 piece 覆盖的字形在目标布局中的完整边界）。
   */
  private static xAtOffsetLocal(layout: LineLayout[], offset: number, fallback: number,
    affinity: CaretAffinity = CaretAffinity.Downstream): number {
    if (layout.length === 0) { return fallback }
    const lineRanges: LineRange[] = layout.map((l: LineLayout): LineRange => ({
      start: l.startUtf16, end: l.endUtf16, breakKind: l.breakKind,
    }))
    const idx = resolveVisualLineIndex(lineRanges, { utf16Offset: offset, affinity })
    const line = layout[idx]
    if (line === undefined) { return fallback }
    let x = line.left
    for (const stop of line.caretStops) {
      if (stop.utf16Offset <= offset) {
        x = stop.x
      }
    }
    return x
  }

  /**
   * Issue #879 复核评论6082616112 问题4：添加 affinity 参数。
   * 默认 Downstream，但 end offset 应传 Upstream。
   *
   * RetainedMove 通道按 piece 采样时需要计算每个 piece 在新布局中的 y 坐标，
   * 用于让每个 piece 运动到各自的新版目标行位置（而非整 move 的 newRect.y）。
   */
  private static yAtOffsetLocal(layout: LineLayout[], offset: number, fallback: number,
    affinity: CaretAffinity = CaretAffinity.Downstream): number {
    if (layout.length === 0) { return fallback }
    const lineRanges: LineRange[] = layout.map((l: LineLayout): LineRange => ({
      start: l.startUtf16, end: l.endUtf16, breakKind: l.breakKind,
    }))
    const idx = resolveVisualLineIndex(lineRanges, { utf16Offset: offset, affinity })
    const line = layout[idx]
    if (line === undefined) { return fallback }
    return line.y
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
      // Issue #879 评论6096421590 问题3：在 insert 仍在吐字的过程，
      // 所有新插入的字都归运动层负责，静态层必须排除整个 insert run。
      // 不能只扣「可见岛」而把岛之间的字留在静态层——那些字也属于尚未吐出的新字，
      // 静态层提前显示会导致闪字/双字。完整文字一笔结束统一回到静态层。
      excluded.push({ utf16Start: run.utf16Start, utf16End: run.utf16End })
    }
    for (const move of plan.retainedMoves) {
      excluded.push({ utf16Start: move.newUtf16Start, utf16End: move.newUtf16End })
    }
    // Issue #879 复核评论6081596024 问题1：carryOverMotionRuns 的字形仍由运动层绘制——
    // 从静态层扣除，避免同一字形在静态层和运动层重复绘制（重影）。
    // Issue #879 复核评论6082616112 问题3：只扣 targetUtf16Start/End（新正文范围），
    // 不扣 sourceUtf16Start/End（旧源范围）——旧源范围在新正文中无意义，
    // 拿旧源 offset 在新正文上做减法会误删新正文的字。
    for (const motionRun of plan.carryOverMotionRuns) {
      excluded.push({ utf16Start: motionRun.targetUtf16Start, utf16End: motionRun.targetUtf16End })
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
