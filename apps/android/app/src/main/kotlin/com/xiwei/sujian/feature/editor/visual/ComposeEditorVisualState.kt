package com.xiwei.sujian.feature.editor.visual

import android.util.Log
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.input.EditorInputSnapshot
import com.xiwei.sujian.feature.editor.input.InputSnapshotOutcome
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.layout.cursorRect
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlin.collections.ArrayDeque

/**
 * #641 评论1 第4/5节：Compose 显示层视觉状态。
 *
 * #689 评论 5674631257 步骤7：把视觉动画从"事务重启"改成"持续时间线"。
 *
 * 删除：
 * - _activeTransaction / activeTransaction
 * - _activeIntent / activeIntent
 * - _masterProgress / masterProgress
 * - reportProgress()
 * - finishTransaction()
 * - applyFrameUpdate() 里 _masterProgress = 0f
 *
 * 改成持有：
 * - [frameCoordinator]（只返回 [ComposeVisualPatch]）
 * - [visualTimeline]（长期持续视觉状态）
 * - [_visualScene]（每次 sample 后同步给 overlay）
 *
 * 时间戳来自 Compose frame clock（由 overlay 调用 [applyVisualPatchAtFrame] /
 * [sampleVisualScene] 时传入），不在这里用 `System.nanoTime()` 猜当前帧。
 *
 * @param targetId 当前编辑目标 ID — 用于结构化诊断事件。
 * @param initialDrawsVisualCursor 初始视觉光标状态 — smooth cursor 开启时从 attach 后一直为 true。
 * @param classifier 本地视觉 plan 分类器 — 生产环境默认 [CoreLocalVisualPlanClassifier] 直接调 Core，
 *   测试环境（Robolectric）注入 fake 绕过原生库加载。
 */
class ComposeEditorVisualState(
    private val targetId: String,
    initialDrawsVisualCursor: Boolean = false,
    private val classifier: LocalVisualPlanClassifier = CoreLocalVisualPlanClassifier,
) {
    companion object {
        private const val TAG = "EditorVisualState"

        /** 1 ms = 1_000_000 ns。 */
        private const val NANOS_PER_MS = 1_000_000L
    }

    /** 帧协调器 — 只回答"旧屏幕帧到新屏幕帧改了什么"。 */
    private val frameCoordinator = ComposeVisualFrameCoordinator(targetId)

    /** 持续视觉时间线 — 真正长期存在的屏幕动画状态。 */
    private val visualTimeline = ComposeVisualTimeline()

    /** 最新 layout 快照 — 供 overlay 读取 bounding box。 */
    private val _latestLayout = MutableStateFlow<ComposeLayoutSnapshot?>(null)
    val latestLayout: StateFlow<ComposeLayoutSnapshot?> = _latestLayout.asStateFlow()

    /**
     * 视觉光标是否由 overlay 绘制 —
     * smooth cursor 开启：编辑器 attach 以后一直为 true（系统光标透明）。
     * smooth cursor 关闭：一直为 false（系统光标正常画）。
     * 仅由设置/attach 生命周期决定，不在某笔 patch 到达时改写。
     */
    private val _drawsVisualCursor = MutableStateFlow(initialDrawsVisualCursor)
    val drawsVisualCursor: StateFlow<Boolean> = _drawsVisualCursor.asStateFlow()

    /**
     * 当前视觉场景 — overlay 读取绘制。
     * 每次 [sampleVisualScene] 后更新。
     */
    private val _visualScene = MutableStateFlow(ComposeVisualScene.Empty)
    val visualScene: StateFlow<ComposeVisualScene> = _visualScene.asStateFlow()

    /**
     * 待消费的 patch 队列 — 解决快速输入时 LaunchedEffect 取消旧协程导致丢 patch 的问题。
     * 使用队列而非 conflated state，确保每一笔 patch 都能被处理。
     */
    private val pendingPatches = ArrayDeque<ComposeVisualPatch>()
    private val _patchVersion = MutableStateFlow(0L)
    val patchVersion: StateFlow<Long> = _patchVersion.asStateFlow()

    /**
     * 最新生成的 patch — 仅保留给日志/调试使用，timeline 输入不再依赖它。
     */
    private val _latestPatch = MutableStateFlow<ComposeVisualPatch?>(null)
    val latestPatch: StateFlow<ComposeVisualPatch?> = _latestPatch.asStateFlow()

    /**
     * #691：静止光标 rect — 当没有光标动画时，overlay 从这里读取光标的最终真实位置。
     * 由 [onAuthoritativeLayout] 更新，始终反映当前 selection 对应的光标几何。
     */
    private val _restingCursorRect = MutableStateFlow<Rect?>(null)
    val restingCursorRect: StateFlow<Rect?> = _restingCursorRect.asStateFlow()

    /**
     * #691 评论 5679242735 修改2：运行时 policy 切换的最新 effective policy。
     *
     * 非 null 时，[drainPendingPatchesAtFrame] 会把已入队 patch 的 motionPolicy 替换成它，
     * 防止旧 patch 带着原来的 insertedUnits/deletedUnits/retainedMoves 再进入 timeline
     * 把文字动画重新启动（用户已关闭文字动画或打开 reduce-motion）。
     */
    private var currentMotionPolicy: EditorMotionPolicy? = null

    /**
     * #694 评论第 1/3 步：本地输入视觉事实 tracker —
     * 普通 [ArrayDeque]，不是 Compose State。记录 InputTransformation 拿到的本地输入，
     * 等 onAuthoritativeLayout 的新 layout 到达时配对生成 ComposeVisualPatch(intent=null)。
     */
    private val localInputTracker = LocalInputVisualEditTracker()

    /**
     * #694 评论第 3 步：上一次真正呈现的 layout — 本地输入配对时的 oldLayout（T0）。
     */
    private var lastPresentedLayout: ComposeLayoutSnapshot? = null

    /**
     * #694 评论 5692161955 问题3：上一次 composition 是否活跃 —
     * 用于 composition 从 active->false 时的过渡同步。
     *
     * composition 活跃期间 [frameCoordinator] 的 lastConsumed 不推进（不调 observePresentedLayout），
     * composition 结束时若最终没有生成 local patch，也要把最终已提交 layout 用
     * [ComposeVisualFrameCoordinator.observePresentedLayout] 同步给 coordinator，
     * 否则后续 Undo/Redo/Programmatic intent 的 pending.baseText 与 lastConsumed.text 对不上。
     */
    private var wasCompositionActive: Boolean = false

    /**
     * #694 评论 5693864609 问题2：composition 独立生命周期入口 —
     * composition 开始时保存的已提交布局（base），composition 结束后用 base -> final 生成 patch。
     */
    private var compositionBaseLayout: ComposeLayoutSnapshot? = null

    /**
     * #694 评论 5693864609 问题2：composition 结束后 final text 对应的新 layout 还没到时暂存，
     * 等下一份 onAuthoritativeLayout 到达时收口。
     */
    private var pendingCompositionCommitText: String? = null

    /**
     * #694 评论 5693864609 问题2：上一次 snapshot 观察时 composition 是否活跃 —
     * 用于检测 composition active->inactive 边沿。
     */
    private var wasCompositionActiveForSnapshot: Boolean = false

    /**
     * #694 评论 5695660885 问题1：composition 视觉生命周期状态机 —
     * 拦截 [onAuthoritativeLayout] 在 bridge outcome 到达前提前发布候选 local patch。
     *
     * 真实运行时 [onAuthoritativeLayout]（BasicTextField onTextLayout 回调）和
     * [onInputSnapshotResolved]（snapshotFlow collector）是两条独立流，
     * onTextLayout 可能先于 bridge outcome 到达。此时若 [localInputTracker] 非空
     * （InputTransformation 无条件 recordLocalInput），旧逻辑会 drainMatchingChain
     * 命中并提前发布候选 local patch。本状态机在 composition 期间/结束后等待 bridge
     * outcome 期间，阻止 onAuthoritativeLayout 走普通 local-input 分支。
     *
     * - [Idle]：无 composition 活动，onAuthoritativeLayout 走正常路径。
     * - [Composing]：snapshot 看到 composition active，保存了 compositionBaseLayout。
     *   onAuthoritativeLayout 在此 phase 只缓存 layout/cursor，不发布 patch。
     * - [AwaitingBridgeResolution]：composition 结束（compositionActive=false）但 bridge
     *   outcome 尚未到达。onAuthoritativeLayout 在此 phase 只缓存 layout/cursor，不发布 patch。
     * - [AcceptedAwaitingFinalLayout]：bridge 已 LocalCommitAccepted 但 final layout 还没到，
     *   等下一份 matching layout 收口。
     */
    private enum class CompositionVisualPhase {
        Idle,
        Composing,
        AwaitingBridgeResolution,
        AcceptedAwaitingFinalLayout,
    }

    private var compositionVisualPhase: CompositionVisualPhase = CompositionVisualPhase.Idle

    /**
     * #694 评论第 3 步：本地输入 patch ID 计数器 —
     * 从 1_000_000L 起避免与 [ComposeVisualFrameCoordinator] 的 patch id 冲突。
     */
    private var nextLocalPatchId: Long = 1_000_000L

    /**
     * #708 评论 5723410606 第二节：首帧 ghost unit key 计数器 —
     * 从 2_000_000L 起避免与 [ComposeVisualTimeline] 内部 nextUnitKey（从 1L 起）
     * 和 [nextLocalPatchId]（从 1_000_000L 起）冲突。
     * 首帧 ghost 是 onAuthoritativeLayout 建立的临时 unit，
     * 下一帧 drainPendingPatchesAtFrame 后由 timeline 的正式 scene 取代。
     */
    private var nextHandoffUnitKey: Long = 2_000_000L

    /**
     * #708 评论 5723410606 第一节：draw 阶段原子快照 —
     * 由 [drawSnapshot] 在 drawWithContent 里一次性取走。
     * 关键：这个 State 只能在 drawWithContent 里读。
     * 动画每帧更新时，Compose 只重跑 Draw，不重新执行 BasicTextField 的 Composition/Layout。
     */
    private var drawSnapshotState: ComposeEditorDrawSnapshot by mutableStateOf(ComposeEditorDrawSnapshot())

    /**
     * #708 评论 5723410606 第三节：layout fingerprint 持久状态 —
     * 把 fingerprint 做成明确 data class，不再 List<Any>。
     * onAuthoritativeLayout 最前面先算 fingerprint，相同正文+相同几何时直接返回，
     * 不更新 layout epoch、不调用 frameCoordinator.onLayout/observePresentedLayout、不重新发布相同 TextLayoutResult。
     */
    private var lastObservedLayoutFingerprint: LayoutFingerprint? = null

    /**
     * Core 视觉意图到达 — 只把 intent 交给 frameCoordinator，不启动动画、不改 layout。
     *
     * @param intent Core 视觉意图。
     * @param motionPolicy 动画策略 — 传入前先 effective() 收口 reduce-motion。
     */
    fun onVisualIntent(
        intent: EditorVisualIntent,
        motionPolicy: EditorMotionPolicy,
    ) {
        val update = frameCoordinator.onVisualIntent(intent, motionPolicy.effective())
        applyFrameUpdate(update)
    }

    /**
     * #694 评论第 1/3 步：本地输入入口 — 只写普通 pending queue（[LocalInputVisualEditTracker]）。
     *
     * 由 [WritingEditorSurface] 的 InputTransformation 调用，不等 Core，不启动动画。
     * 等下一份真实 TextLayoutResult 到达时由 [onAuthoritativeLayout] 配对生成
     * ComposeVisualPatch(intent=null) 入队。
     *
     * #708 评论 5723410606 第二节：不再在 InputTransformation 阶段武装整屏 barrier —
     * 那一刻还没有本次新文字的 TextLayoutResult，冻结画面会把上一整屏当成 baseScene/baseLayout，
     * 是"整行闪、全工作区闪、旧字残留"的来源。只记录 edit，由 onAuthoritativeLayout 在 layout 阶段
     * 配对出 local patch 后建立局部 handoff scene。
     */
    fun recordLocalInput(
        oldText: String,
        newText: String,
        oldSelection: TextRange,
        newSelection: TextRange,
        changes: List<LocalInputChange>,
    ) {
        localInputTracker.record(oldText, newText, oldSelection, newSelection, changes)
    }

    /**
     * #708 评论 5723410606 第一节：draw 层在 drawWithContent 里一次性取走 draw 阶段原子快照。
     * 关键：这个 State 只能在 drawWithContent 里读。
     */
    internal fun drawSnapshot(): ComposeEditorDrawSnapshot = drawSnapshotState

    /**
     * #694 评论 5694645209 问题1：根据 bridge 的 [InputSnapshotOutcome] 收口 —
     * 由 [WritingPaneRoute] 的 `SetupInputSnapshotCollector` 在 `snapshotFlow.collect` 中调用，
     * **在 bridge.onInputSnapshot 之后**，用 bridge 的真实决定收口本地视觉状态。
     *
     * - [InputSnapshotOutcome.Composing]：只记录 composition start/base（保存 compositionBaseLayout = lastPresentedLayout）。
     * - [InputSnapshotOutcome.LocalCommitAccepted]：才允许 [finishCompositionCommit]、发布 local patch、
     *   推进 local/Core 对齐后的屏幕 baseline。
     * - [InputSnapshotOutcome.AuthoritativeApplied] / [InputSnapshotOutcome.LocalCommitRejected]：
     *   取消本次 composition local visual state，清掉本次 preedit 留下的 local tracker/pending commit，
     *   **不要**发布候选 local patch，也不要把 coordinator 推到候选文本；后续让权威 layout / external intent 正常接管。
     * - [InputSnapshotOutcome.NoTextChange]：composition 生命周期边沿仍需记录（如 composition 开始时保存 base），
     *   但不生成 patch。
     *
     * @param snapshot 当前 IME 输入快照（text + selection + composition）。
     * @param outcome bridge 对本次 snapshot 的处理结果。
     */
    fun onInputSnapshotResolved(
        snapshot: EditorInputSnapshot,
        outcome: InputSnapshotOutcome,
    ) {
        val compositionActive = snapshot.composition != null
        // composition 生命周期边沿：composition 刚开始时保存 base
        // #694 评论 5696786245：只有 phase == Idle 时才允许初始化 compositionBaseLayout。
        // layout 路径（onAuthoritativeLayout）可能已经把 phase 武装成 Composing，
        // 此时晚到的 snapshotFlow active emission 只能更新 wasCompositionActiveForSnapshot，
        // 不能用已变成 preedit 的 lastPresentedLayout 覆盖 layout 路径保存的正确 base。
        if (compositionActive && compositionVisualPhase == CompositionVisualPhase.Idle) {
            compositionBaseLayout = lastPresentedLayout
            // #694 评论 5695660885 问题1：进入 Composing phase，
            // 拦截 onAuthoritativeLayout 在 bridge outcome 到达前提前发布候选 local patch。
            compositionVisualPhase = CompositionVisualPhase.Composing
        }
        when (outcome) {
            InputSnapshotOutcome.Composing -> {
                // composition 仍活跃：只记录 composition start/base（上面已保存），不生成 patch。
                // phase 已在 composition active 边沿设为 Composing。
            }
            InputSnapshotOutcome.NoTextChange -> {
                // composition 结束但无变化：不生成 patch。
                // 若 final layout 还没到，不暂存 pendingCompositionCommitText（无变化无需收口）。
                // #694 评论 5695660885 问题1：NoTextChange 如果上一状态是 composition
                // （Composing/AwaitingBridgeResolution），这就是 composition 取消/回到原文。
                // 必须清 compositionBaseLayout / pendingCompositionCommitText /
                // localInputTracker / wasCompositionActive。
                // 如果已经缓存了与 snapshot.text 相同的 final layout，只把它设成
                // lastPresentedLayout / 同步 coordinator baseline，不要生成 local patch。
                if (compositionVisualPhase == CompositionVisualPhase.Composing ||
                    compositionVisualPhase == CompositionVisualPhase.AwaitingBridgeResolution
                ) {
                    val latest = _latestLayout.value
                    if (latest != null && latest.result.layoutInput.text.text == snapshot.text) {
                        // 同步 coordinator baseline，不生成 local patch
                        frameCoordinator.observePresentedLayout(latest)
                        lastPresentedLayout = latest
                    }
                    cancelCompositionLocalVisualState()
                }
            }
            InputSnapshotOutcome.LocalCommitAccepted -> {
                // bridge 已接受本地 commit：视觉层可以 finishCompositionCommit。
                // #694 评论 5696394554：不要把 wasCompositionActiveForSnapshot 当唯一前提。
                // 只要 phase 是 Composing / AwaitingBridgeResolution，就说明 composition 事实
                // 已经可能由 layout 路径确认过；Core 接受后应正常 finishCompositionCommit 或
                // 进入 AcceptedAwaitingFinalLayout。
                val compositionPhaseActive =
                    compositionVisualPhase == CompositionVisualPhase.Composing ||
                        compositionVisualPhase == CompositionVisualPhase.AwaitingBridgeResolution
                if (!compositionActive && (wasCompositionActiveForSnapshot || compositionPhaseActive)) {
                    val latest = _latestLayout.value
                    if (latest != null && latest.result.layoutInput.text.text == snapshot.text) {
                        finishCompositionCommit(snapshot.text, latest)
                    } else {
                        // final text 对应的新 layout 还没到，记 pending
                        // #694 评论 5694645209 问题1：只在 LocalCommitAccepted 路径下才设置 pendingCompositionCommitText。
                        pendingCompositionCommitText = snapshot.text
                        // #694 评论 5695660885 问题1：进入 AcceptedAwaitingFinalLayout phase，
                        // 等下一份 matching layout 收口。
                        compositionVisualPhase = CompositionVisualPhase.AcceptedAwaitingFinalLayout
                    }
                }
            }
            InputSnapshotOutcome.AuthoritativeApplied,
            InputSnapshotOutcome.LocalCommitRejected,
            -> {
                // bridge 消费了 pending authoritative 或 Core 拒绝了本地 commit：
                // 取消本次 composition local visual state，不发布候选 local patch，
                // 也不把 coordinator 推到候选文本；后续让权威 layout / external intent 正常接管。
                // #694 评论 5696394554：以 compositionVisualPhase 作为视觉事务是否存在的真值来收口/清理。
                // wasCompositionActiveForSnapshot 最多保留成辅助状态，不要决定能不能收口。
                // 只有 phase 处于 composition 相关状态时才 cancel，避免在 Idle 时不必要地重置。
                if (compositionVisualPhase == CompositionVisualPhase.Composing ||
                    compositionVisualPhase == CompositionVisualPhase.AwaitingBridgeResolution ||
                    compositionVisualPhase == CompositionVisualPhase.AcceptedAwaitingFinalLayout
                ) {
                    cancelCompositionLocalVisualState()
                }
            }
        }
        wasCompositionActiveForSnapshot = compositionActive
    }

    /**
     * #708 评论 5724568261 缺口1/缺口2：本地编辑首帧 scene 统一发布入口 —
     *
     * 取代已删除的 [ComposeLocalEditHandoff] 死状态 + [bindLocalPatchHandoff] 空绑定。
     * 两条路径（[onAuthoritativeLayout] 普通本地输入 / [finishCompositionCommit] composition 最终提交）
     * 都调用本方法，把"这一笔编辑哪些局部区域暂时由 overlay 接管"直接写进 [_visualScene] 和
     * [drawSnapshotState]，不再经过一个从未被真正建立的中间 handoff 字段。
     *
     * #708 评论 5725706551：scene rebase — 不再直接复制旧 hiddenRanges/units（旧坐标系），
     * 而是调用 [ComposeLocalHandoffRebase.rebase] 把旧 visible scene 映射到新正文坐标系：
     *
     * 1. **Scene rebase**：旧 active unit（targetRange != null）通过 offsetMap 映射到新正文坐标；
     *    存活 slice 改 newRange/newLayout，保持当前屏幕位置；被删除 slice 从当前可见 alpha/position
     *    转 handoff ghost（不新建 alpha=1 的完整 ghost）。已有 ghost 保持当前状态。
     * 2. **hiddenRanges 重新推导**：从 rebase 后所有 targetRange != null 的 unit 重新推导
     *    （不再从旧 hiddenRanges 复制），再加 [patch.insertedUnits] 和 reflow slice 的 newRange。
     *    这确保 hiddenRanges 和 newLayout 属于同一个坐标系。
     * 3. **deletedUnits 只补差集**：先收集 rebase 阶段已经转成 ghost 的旧正文范围（ghostedCoverage），
     *    只给"没有被旧 active unit 接管"的 deletedUnits 新建 alpha=1 的完整 ghost。
     *    这避免同一 glyph 同时出现 Inserted + DeletedGhost 的重影。
     * 4. **Reflow**：保持现有差集逻辑（[ComposeOverlayOwnership.subtractOwnedRanges]），
     *    但 ownedRanges 从 rebase 后的 units 取。
     * 5. **Cursor**：保持现有首帧光标处理逻辑。
     *
     * @param patch 本笔 local patch（含 insertedUnits/deletedUnits/reflowMoves/originCursorRect）。
     * @param oldLayout T0 布局（建 ghost / reflow oldBounds 来源）。
     * @param newLayout Tn 布局（reflow unit 所属 layout）。
     * @param restingCursorRect 当前静止光标 rect — 写进 drawSnapshotState.restingCursorRect。
     */
    private fun publishLocalHandoffScene(
        patch: ComposeVisualPatch,
        oldLayout: ComposeLayoutSnapshot,
        newLayout: ComposeLayoutSnapshot,
        restingCursorRect: Rect?,
    ) {
        _visualScene.update { scene ->
            // #708 评论 5725706551 步骤1：调用 ComposeLocalHandoffRebase.rebase 做 scene rebase —
            // 把旧 visible scene（旧正文坐标）映射到新正文坐标系。
            // 旧 active unit 通过 splitMappedRangeForward 映射到新正文坐标：
            //   - 存活 slice：targetRange/range 改成 newRange，layout 改成 newLayout，alpha/position 固定在当前可见值；
            //   - 被删除 slice：从当前可见 alpha/position 转 handoff ghost（不新建 alpha=1 的完整 ghost）；
            //   - 已有 ghost：保持当前状态不变。
            // ghostedCoverage 记录 rebase 阶段已经转成 ghost 的旧正文范围。
            val rebased = ComposeLocalHandoffRebase.rebase(scene, patch)
            val rebasedUnits = rebased.units.toMutableList()

            // #708 评论 5725706551 步骤2：hiddenRanges 从 rebase 后所有 targetRange != null 的 unit 重新推导 —
            // 不再从旧 hiddenRanges 复制（旧坐标系），确保 hiddenRanges 和 newLayout 属于同一个坐标系。
            val mergedHidden = mutableListOf<TextRange>()
            for (unit in rebasedUnits) {
                val tr = unit.targetRange
                if (tr != null && tr.start < tr.end) {
                    val alreadyHidden = mergedHidden.any { it.start == tr.start && it.end == tr.end }
                    if (!alreadyHidden) {
                        mergedHidden.add(tr)
                    }
                }
            }
            // 加 patch.insertedUnits — BasicTextField 先不画新字，由 overlay 吐字
            for (ins in patch.insertedUnits) {
                if (ins.start < ins.end &&
                    mergedHidden.none { it.start == ins.start && it.end == ins.end }
                ) {
                    mergedHidden.add(ins)
                }
            }

            // #708 评论 5726837636：用 subtractRanges 算真正差集 —
            // ghostedCoverage 是 rebase 阶段已经转成 ghost 的旧正文范围（旧坐标系）。
            // 只给"deletedUnits - ghostedCoverage"的剩余部分新建 alpha=1 的完整 ghost，
            // 部分覆盖时只补未被接管的 slice，不把已由旧 active unit 接管的部分重画一遍。
            // 旧实现用 ghostedCoverage.any{整段覆盖} 判断，部分覆盖（如 del=[0,2) 而
            // ghostedCoverage=[1,2)）时 alreadyGhosted=false，会为整个 [0,2) 新建 alpha=1 ghost，
            // 导致 [1,2) 被画两次（旧 active unit 转 ghost 画一次 + 新建 ghost 画一次）。
            val remainingDeleted =
                ComposeVisualRebase.subtractRanges(
                    candidates = patch.deletedUnits,
                    blockers = rebased.ghostedCoverage,
                )
            for (del in remainingDeleted) {
                if (del.start >= del.end) continue
                // 检查 rebasedUnits 里是否已有覆盖此 range 的 ghost
                if (rebasedUnits.any { it.targetRange == null && it.range == del }) continue
                // 从 oldLayout 取旧位置建立静态 ghost
                val oldBounds =
                    ComposeVisualRebase.safePathBounds(
                        oldLayout.result, del,
                    ) ?: continue
                val oldPosition = Offset(oldBounds.left, oldBounds.top)
                nextHandoffUnitKey++
                rebasedUnits +=
                    VisualTextUnit(
                        key = nextHandoffUnitKey,
                        layout = oldLayout,
                        range = del,
                        targetRange = null,
                        // 完整可见：alpha=1，不动画
                        alpha = TimedFloat(1f, 1f, 0L, 0L),
                        position = TimedOffset(oldPosition, oldPosition, 0L, 0L),
                        role = VisualUnitRole.DeletedGhost,
                    )
            }

            // #708 评论 5725146968 缺口2：reflow 首帧 — 为每个 reflowMove 建一个首帧静止的 ReflowMove unit。
            // 使用 ComposeOverlayOwnership.subtractOwnedRanges 做真正的差集，
            // 只给没有被其他 active unit 接管的 slice 建临时 ReflowMove。
            // 同时把差集后剩余 slice 的 newRange 加进 hiddenRanges，
            // 让 BasicTextField 已落到新行的那份字先裁掉，由 overlay 从 oldBounds -> newBounds 平移。
            // 首帧静止在旧位置（position from=to=oldPosition），alpha 永远 1。
            // 下一帧 timeline.applyPatch 正式创建 oldBounds -> newBounds 的 ReflowMove 后直接覆盖首帧 scene。
            // #708 评论 5725706551：ownedRanges 从 rebase 后的 units 取（新坐标系）。
            val ownedRanges = rebasedUnits.mapNotNull { it.targetRange }
            for (move in patch.reflowMoves) {
                val remainingSlices =
                    ComposeOverlayOwnership.subtractOwnedRanges(
                        move = move,
                        ownedRanges = ownedRanges,
                        oldLayout = oldLayout,
                        newLayout = newLayout,
                    )
                for (slice in remainingSlices) {
                    val nr = slice.newRange
                    if (nr.start >= nr.end) continue
                    if (nr.end > newLayout.result.layoutInput.text.length) continue
                    // hiddenRanges：让 BasicTextField 已落到新行的那份字先裁掉
                    if (mergedHidden.none { it.start == nr.start && it.end == nr.end }) {
                        mergedHidden.add(nr)
                    }
                    // 已有同 newRange 的 ReflowMove 首帧 unit 则跳过
                    if (rebasedUnits.any {
                            it.targetRange == nr && it.role == VisualUnitRole.ReflowMove
                        }
                    ) {
                        continue
                    }
                    val oldPosition = Offset(slice.oldBounds.left, slice.oldBounds.top)
                    nextHandoffUnitKey++
                    rebasedUnits +=
                        VisualTextUnit(
                            key = nextHandoffUnitKey,
                            layout = newLayout,
                            range = nr,
                            targetRange = nr,
                            // 首帧静止：alpha 永远 1
                            alpha = TimedFloat(1f, 1f, 0L, 0L),
                            // 首帧静止在旧位置
                            position = TimedOffset(oldPosition, oldPosition, 0L, 0L),
                            role = VisualUnitRole.ReflowMove,
                        )
                }
            }

            // #708 评论 5725146968：首帧光标所有权 —
            // 不只是删除，所有有光标动画的 patch（cursorEnabled && cursorMotionPath != null）
            // 都用同一份 T0 caret 作为首帧 scene.cursorRect，
            // 防止纯插入时首帧 draw 层直接算新光标位置、下一帧 timeline 又用旧位置做起点导致光标回抽。
            // cursor animation 关闭时（cursorEnabled=false）不抢系统光标，保持 null。
            val handoffCursorRect =
                if (patch.motionPolicy.effective().cursorEnabled &&
                    patch.cursorMotionPath != null
                ) {
                    patch.originCursorRect ?: computeCursorRectFromLayout(oldLayout)
                } else {
                    null
                }
            val hiddenChanged = mergedHidden.size != scene.hiddenRanges.size
            val unitsChanged = rebasedUnits.size != scene.units.size
            val cursorChanged = handoffCursorRect != null && handoffCursorRect != scene.cursorRect
            if (!hiddenChanged && !unitsChanged && !cursorChanged) {
                scene
            } else {
                scene.copy(
                    hiddenRanges = mergedHidden,
                    units = rebasedUnits,
                    cursorRect = handoffCursorRect ?: scene.cursorRect,
                )
            }
        }
        // 同步把首帧 scene 写进 draw snapshot — draw 层下一帧 drawWithContent 直接读
        drawSnapshotState =
            drawSnapshotState.copy(
                scene = _visualScene.value,
                layout = newLayout,
                restingCursorRect = restingCursorRect,
            )
    }

    /**
     * #694 评论 5694645209 问题1：取消本次 composition local visual state —
     * 清空 [compositionBaseLayout]、[pendingCompositionCommitText]，
     * 重置 [wasCompositionActive]/[wasCompositionActiveForSnapshot]，
     * 清掉本次 preedit 留下的 local tracker（[localInputTracker].clear()）。
     *
     * 不发布候选 local patch，也不把 coordinator 推到候选文本。
     * 后续让权威 layout / external intent 正常接管。
     */
    private fun cancelCompositionLocalVisualState() {
        compositionBaseLayout = null
        pendingCompositionCommitText = null
        wasCompositionActive = false
        wasCompositionActiveForSnapshot = false
        // #694 评论 5695660885 问题1：重置 composition 视觉生命周期 phase。
        compositionVisualPhase = CompositionVisualPhase.Idle
        // 清掉本次 preedit 留下的 local tracker — 后续权威 layout 到达时不会配对出 a->an 的 local patch。
        localInputTracker.clear()
    }

    /**
     * #694 评论 5693864609 问题2：composition 结束后用 base -> final 生成 patch 并入队。
     *
     * @param commitText composition 结束后的最终正文。
     * @param finalLayout commitText 对应的最终布局。
     */
    private fun finishCompositionCommit(
        commitText: String,
        finalLayout: ComposeLayoutSnapshot,
    ) {
        val baseLayout = compositionBaseLayout
        if (baseLayout != null && baseLayout.result.layoutInput.text.text != commitText) {
            // 用 compositionBaseLayout -> finalLayout 生成 patch
            val baseOldText = baseLayout.result.layoutInput.text.text
            val localChain = localInputTracker.drainMatchingChain(baseOldText, commitText)
            if (localChain != null) {
                val localPatch = buildLocalInputPatch(localChain, baseLayout, finalLayout)
                if (localPatch != null) {
                    // #708 评论 5724568261 缺口1：composition 最终提交路径也发布局部首帧 scene —
                    // 不再只做 addLast + patchVersion + bindLocalPatchHandoff（旧 bindLocalPatchHandoff
                    // 因 pendingLocalEditHandoff 从未被建立只走 Log.w，首帧 scene 从未发布）。
                    // 现在统一调 publishLocalHandoffScene，让中文 composition commit 后 local timeline
                    // 立即接管，不出现"最终字先裸画一帧 -> 动画再接手"的窗口。
                    publishLocalHandoffScene(
                        patch = localPatch,
                        oldLayout = baseLayout,
                        newLayout = finalLayout,
                        restingCursorRect = computeCursorRectFromLayout(finalLayout),
                    )
                    pendingPatches.addLast(localPatch)
                    _patchVersion.update { it + 1L }
                    _latestPatch.update { localPatch }
                }
            }
        }
        // 同步 Core/external baseline
        frameCoordinator.observePresentedLayout(finalLayout)
        lastPresentedLayout = finalLayout
        // 清掉本次 composition 状态
        compositionBaseLayout = null
        pendingCompositionCommitText = null
        wasCompositionActive = false
        // #694 评论 5695660885 问题1：composition commit 收口后回到 Idle phase。
        compositionVisualPhase = CompositionVisualPhase.Idle
    }

    /**
     * #694 评论第 3/4 步 + 评论 5691696678 问题1：从配对的连续 [LocalInputVisualEdit] chain
     * + old/new layout 构造 ComposeVisualPatch(coreTransactionIds = emptyList(), intent = null)。
     *
     * 不为了本地输入伪造 Core transaction — [ComposeVisualPatch.intent] 本来就是 nullable。
     *
     * #694 评论 5691696678 问题1：接收 chain 而非单笔 edit。
     * - T0 = chain.first().oldText / oldSelection
     * - Tn = chain.last().newText / newSelection
     * - offset map 用 [ComposeLocalVisualRebase.composeLocalChainOffsetMap] 逐 stage 合成
     *   （不同坐标系的 changes 不能直接摊平）。
     * - 防御性检查：chain.first().oldText == oldLayout.text && chain.last().newText == newLayout.text。
     */
    private fun buildLocalInputPatch(
        chain: List<LocalInputVisualEdit>,
        oldLayout: ComposeLayoutSnapshot,
        newLayout: ComposeLayoutSnapshot,
    ): ComposeVisualPatch? {
        if (chain.isEmpty()) return null
        val firstEdit = chain.first()
        val lastEdit = chain.last()
        val oldText = firstEdit.oldText
        val newText = lastEdit.newText
        // 防御性：配对的 chain 首笔 oldText / 末笔 newText 必须与 layout 一致
        if (oldText != oldLayout.result.layoutInput.text.text) return null
        if (newText != newLayout.result.layoutInput.text.text) return null
        val oldLength = oldText.length
        val newLength = newText.length

        // 从 chain 各笔的 changes 逐 stage 合成 T0→Tn 的 unchanged offset map
        val offsetMap = ComposeLocalVisualRebase.composeLocalChainOffsetMap(chain)
        // 从最终 composed map 的补集算净变化（用于 transactionTextKind 判定）
        val changedRanges =
            ComposeLocalVisualRebase.changedRangesFromOffsetMap(offsetMap, oldLength, newLength)
        val transactionTextKind =
            when {
                changedRanges.oldRanges.isEmpty() && changedRanges.newRanges.isEmpty() ->
                    TextVisualKind.None
                changedRanges.oldRanges.isEmpty() -> TextVisualKind.Insert
                changedRanges.newRanges.isEmpty() -> TextVisualKind.Delete
                else -> TextVisualKind.Move
            }

        val motionPolicy = (currentMotionPolicy ?: EditorMotionPolicy()).effective()
        val customTextAnimationEnabled =
            motionPolicy.textEnabled && transactionTextKind != TextVisualKind.None

        // #694 评论 5691696678 问题3：insertedUnits/deletedUnits 用通用 stage-map 版本合成，
        // 保留多字符吐字顺序（a/b/c 三个 unit 而非单个 [0,3)）。
        // 每笔 edit 的 insertedUnits/deletedUnits 从该笔 stage offset map 补集算，
        // 然后沿后续 stage offset map 映射到最终 Tn / 最初 T0。
        val composedInserted = ComposeLocalVisualRebase.composeLocalChainInsertedUnits(chain)
        val composedDeleted = ComposeLocalVisualRebase.composeLocalChainDeletedUnits(chain)

        // #694 评论 5692161955 问题1/2：调用注入的 classifier 做视觉分类，
        // 得到 animationMode 和按 grapheme cluster 拆分的 animation units。
        // 不再硬编码 CLUSTER_ANIMATION，不再按 UTF-16 +1 硬切。
        // #694 评论 5693864609 问题3：通过注入的 [LocalVisualPlanClassifier] 调用，
        // 生产用 Core，测试用 fake（绕过 Robolectric 原生库加载）。
        val corePlan =
            classifier.classify(
                oldText = oldText,
                newText = newText,
                oldAffectedRanges = changedRanges.oldRanges,
                newAffectedRanges = changedRanges.newRanges,
                animationEnabled = customTextAnimationEnabled,
            )
        val planAnimationMode = corePlan.animationMode
        val planInsertedUnits =
            if (customTextAnimationEnabled) {
                ComposeLocalVisualRebase.utf16AnimationUnitsFromPlan(newText, corePlan.newAnimationUnits)
            } else {
                emptyList()
            }
        val planDeletedUnits =
            if (customTextAnimationEnabled) {
                ComposeLocalVisualRebase.utf16AnimationUnitsFromPlan(oldText, corePlan.oldAnimationUnits)
            } else {
                emptyList()
            }

        val insertedUnits =
            if (customTextAnimationEnabled) {
                when (transactionTextKind) {
                    TextVisualKind.Insert, TextVisualKind.Move -> {
                        // #694 评论 5693864609 问题1：Core plan 负责切分 + local stage 负责排序。
                        // orderedStageRanges = composedInserted（local chain 的时间顺序）。
                        val orderedStageRanges = composedInserted
                        if (planInsertedUnits.isNotEmpty() && orderedStageRanges.isNotEmpty()) {
                            ComposeLocalVisualRebase.orderPlanUnitsByStageRanges(planInsertedUnits, orderedStageRanges)
                        } else if (planInsertedUnits.isNotEmpty()) {
                            planInsertedUnits
                        } else if (orderedStageRanges.isNotEmpty()) {
                            orderedStageRanges
                        } else {
                            changedRanges.newRanges
                        }
                    }
                    TextVisualKind.Delete, TextVisualKind.None -> emptyList()
                }
            } else {
                emptyList()
            }

        val deletedUnits =
            if (customTextAnimationEnabled) {
                when (transactionTextKind) {
                    TextVisualKind.Delete, TextVisualKind.Move -> {
                        // #694 评论 5693864609 问题1：Core plan 负责切分 + local stage 负责排序。
                        // orderedStageRanges = composedDeleted（local chain 的时间顺序）。
                        val orderedStageRanges = composedDeleted
                        if (planDeletedUnits.isNotEmpty() && orderedStageRanges.isNotEmpty()) {
                            ComposeLocalVisualRebase.orderPlanUnitsByStageRanges(planDeletedUnits, orderedStageRanges)
                        } else if (planDeletedUnits.isNotEmpty()) {
                            planDeletedUnits
                        } else if (orderedStageRanges.isNotEmpty()) {
                            orderedStageRanges
                        } else {
                            changedRanges.oldRanges
                        }
                    }
                    TextVisualKind.Insert, TextVisualKind.None -> emptyList()
                }
            } else {
                emptyList()
            }

        // retained reflow 只比较 oldLayout -> newLayout 的真实几何
        // #698 评论 5697612595 chainSize > 1 reflow 收口 —
        // chainSize > 1 时只从最后一份真实 oldLayout（= lastPresentedLayout，即 oldLayout 参数）
        // 到当前真实 newLayout（= snapshot，即 newLayout 参数）做一次 reflow。
        // offsetMap 用 chain 各笔 changes 合成保留输入顺序（composeLocalChainOffsetMap），
        // 但不为 chain 中间笔虚构中间 layout 对象 — 中间笔可能从未真正 layout 过
        // （快速输入中间 layout 被跳过），虚构中间 layout 会引入不存在的几何导致 reflow 跳变。
        // #703 评论 C：本地删除先取消整行 retainedMoves 接管 —
        // 被删除的 glyph 可以由 visual layer 接管（ghost）；
        // 后续普通排版回流先交给 BasicTextField 自己；
        // 不要因为一次 Backspace 就把整行幸存文字全部切到 overlay。
        // #708 评论 5723410606 第四节：独立 reflow 通道 —
        // 不再把"整行/整段 path.getBounds()"当一个块移动（retainedMoves），
        // 改成 ComposeReflowPlanner.plan 按 old/new 行边界切细段，每段在两边都只落在单一视觉行，
        // 只有位置真的变化才生成 ComposeReflowMove。reflowMoves 是与 retainedMoves 不同的所有权：
        // 字仍存在，但从 oldBounds 平移到 newBounds，始终全亮。
        val retainedMoves = emptyList<RetainedMove>()
        val reflowMoves =
            ComposeReflowPlanner.plan(
                oldLayout = oldLayout,
                newLayout = newLayout,
                offsetMap = offsetMap,
            )

        // #694 评论 5693864609 问题1：cursor path 改用 buildLocalChainCursorPath —
        // 对每一笔 edit 用该笔 newSelection.end 作为阶段 caret，
        // 保留快速连续删除/插入的中间光标位置。
        val cursorMotionPath =
            ComposeLocalVisualRebase.buildLocalChainCursorPath(
                chain = chain,
                oldLayout = oldLayout,
                newLayout = newLayout,
                insertedUnits = insertedUnits,
                deletedUnits = deletedUnits,
            )

        // #703 评论 5709208101 问题3：本地编辑的旧 caret 不要再依赖 lastPresentedLayout.selection。
        // 优先使用 chain.first().oldSelection.end + oldLayout.result 生成本次 edit 的明确 origin，
        // 让 barrier 和 timeline 共用这一份 origin。
        // 纯 selection 变化后 lastPresentedLayout.selection 可能 stale（onAuthoritativeLayout 去重 return 不更新），
        // 用 chain.first().oldSelection.end 才是真实的 T0 caret。
        val originCursorRect =
            try {
                val originOffset =
                    firstEdit.oldSelection.end
                        .coerceIn(0, oldLayout.result.layoutInput.text.length)
                oldLayout.cursorRect(originOffset)
            } catch (_: Throwable) {
                null
            }

        nextLocalPatchId++
        return ComposeVisualPatch(
            id = nextLocalPatchId,
            coreTransactionIds = emptyList(),
            oldLayout = oldLayout,
            newLayout = newLayout,
            offsetMap = offsetMap,
            insertedUnits = insertedUnits,
            deletedUnits = deletedUnits,
            retainedMoves = retainedMoves,
            reflowMoves = reflowMoves,
            cursorMotionPath = cursorMotionPath,
            // 本地输入时长由 motionPolicy 决定（timeline 用 policy.textDurationMillis）
            durationMs = 0L,
            // #694 评论 5692161955 问题2：使用 Core plan 返回的 animationMode，
            // 不再硬编码 CLUSTER_ANIMATION。
            animationMode = planAnimationMode,
            motionPolicy = motionPolicy,
            intent = null,
            originCursorRect = originCursorRect,
        )
    }

    /**
     * 系统给出权威布局 — 只记录，不修改输入几何。
     *
     * 得到 patch 后不要启动一笔新事务，只把 patch 暂存/发布给 overlay 的时间线入口。
     *
     * #691：同时更新 restingCursorRect — 当没有光标动画时 overlay 从这里读取最终位置。
     *
     * #694 评论第 2/3 步：[compositionActive] 表示当前 IME composition 是否活跃
     * （bridge.state.composition != null）。composition 活跃时只推进布局基线，
     * 不播放 preedit 的吞吐；composition 结束后的最终输入再配对 [LocalInputVisualEdit]
     * 生成 ComposeVisualPatch(intent=null) 入队，不等 Core 回声。
     *
     * @param result 系统 [BasicTextField] 的 onTextLayout 给出的最终布局结果。
     * @param selection 当前选区（UTF-16）。
     * @param scrollY 当前滚动位置（px）。
     * @param compositionActive 当前 IME composition 是否活跃。
     */
    fun onAuthoritativeLayout(
        result: TextLayoutResult,
        selection: TextRange,
        scrollY: Int,
        compositionActive: Boolean = false,
    ) {
        val snapshot = ComposeLayoutSnapshot(result, selection, scrollY)

        // #708 评论 5723410606 第三节：layout 回路真正断开 —
        // onAuthoritativeLayout 最前面先算 fingerprint。
        // 相同正文+相同几何时：
        // - 可以更新纯 selection/caret 的 draw 数据；
        // - 不更新 layout epoch；
        // - 不调用 frameCoordinator.onLayout/observePresentedLayout；
        // - 不重新发布相同 TextLayoutResult；
        // - 直接返回。
        // 只有真实 text/line geometry 变化才把新 layout 写进 draw snapshot。
        // 旧实现先 _latestLayout.update 再 hasSameTextAndGeometry 去重，顺序反了。
        val fingerprint = layoutFingerprint(snapshot)
        val fingerprintUnchanged = !compositionActive && fingerprint == lastObservedLayoutFingerprint
        if (fingerprintUnchanged) {
            // 纯 selection/caret 变化：只更新 draw snapshot 的 restingCursorRect，
            // 不更新 layout epoch、不调 frameCoordinator、不重新发布相同 TextLayoutResult。
            val cursorRect = computeCursorRectFromLayout(snapshot)
            _restingCursorRect.update { cursorRect }
            drawSnapshotState =
                drawSnapshotState.copy(
                    layout = snapshot,
                    restingCursorRect = cursorRect,
                )
            return
        }
        lastObservedLayoutFingerprint = fingerprint

        // 真实 text/line geometry 变化：把新 layout 写进 _latestLayout 和 draw snapshot
        _latestLayout.update { snapshot }

        // #694 评论 5693864609 问题2：composition 结束后 final text 对应的新 layout 还没到时，
        // 暂存 pendingCompositionCommitText，等下一份 onAuthoritativeLayout 到达时收口。
        val pending = pendingCompositionCommitText
        val newTextForPending = result.layoutInput.text.text
        if (pending != null && newTextForPending == pending && !compositionActive) {
            pendingCompositionCommitText = null
            finishCompositionCommit(pending, snapshot)
            return
        }

        // #694 评论 5695660885 问题1：composition 视觉生命周期拦截 —
        // onTextLayout（onAuthoritativeLayout）可能先于 snapshotFlow collector
        // （onInputSnapshotResolved）到达。若 phase 还是 Composing/AwaitingBridgeResolution，
        // 说明 bridge outcome 还没到，此时只缓存 latest layout / resting cursor，直接 return；
        // 不要 drain localInputTracker，不要发布 patch，不要推进 frameCoordinator。
        // 等 bridge outcome 到达后由 onInputSnapshotResolved 收口（LocalCommitAccepted 调
        // finishCompositionCommit / AuthoritativeApplied/Rejected/NoTextChange 清 state）。
        if (!compositionActive &&
            (
                compositionVisualPhase == CompositionVisualPhase.Composing ||
                    compositionVisualPhase == CompositionVisualPhase.AwaitingBridgeResolution
            )
        ) {
            val cursorRectAwaiting = computeCursorRectFromLayout(snapshot)
            _restingCursorRect.update { cursorRectAwaiting }
            drawSnapshotState =
                drawSnapshotState.copy(
                    layout = snapshot,
                    restingCursorRect = cursorRectAwaiting,
                )
            compositionVisualPhase = CompositionVisualPhase.AwaitingBridgeResolution
            return
        }

        // #691：更新静止光标 rect
        val cursorRect = computeCursorRectFromLayout(snapshot)
        _restingCursorRect.update { cursorRect }

        // #694 评论第 3 步 + 评论 5691696678 问题1：配对 pending local edit chain 生成 ComposeVisualPatch(intent=null)。
        // composition 活跃时只推进布局基线，不播放 preedit 的吞吐。
        // 用 drainMatchingChain 按 lastPresentedLayout.text -> newText 找连续 chain，
        // 修复快速输入中间 layout 被跳过时旧 drainMatching 只返回最后一笔导致 patch 被丢的问题。
        val newText = result.layoutInput.text.text
        val presentedOldText = lastPresentedLayout?.result?.layoutInput?.text?.text ?: ""
        val localChain =
            if (!compositionActive) {
                localInputTracker.drainMatchingChain(presentedOldText, newText)
            } else {
                null
            }
        if (localChain != null) {
            val oldLayout = lastPresentedLayout
            if (oldLayout != null) {
                val localPatch = buildLocalInputPatch(localChain, oldLayout, snapshot)
                if (localPatch != null) {
                    // #708 评论 5724568261 缺口1/缺口2：统一调 publishLocalHandoffScene 发布首帧 scene —
                    // 不再用从未被建立的 pendingLocalEditHandoff + bindLocalPatchHandoff。
                    publishLocalHandoffScene(
                        patch = localPatch,
                        oldLayout = oldLayout,
                        newLayout = snapshot,
                        restingCursorRect = cursorRect,
                    )
                    pendingPatches.addLast(localPatch)
                    _patchVersion.update { it + 1L }
                    _latestPatch.update { localPatch }
                    Log.d(
                        TAG,
                        "local_patch_published: id=${localPatch.id} " +
                            "oldLen=${oldLayout.result.layoutInput.text.length} " +
                            "newLen=${newText.length} chainSize=${localChain.size} " +
                            "drawsVisualCursor=${_drawsVisualCursor.value}",
                    )
                }
            }
            // #694 评论 5691696678 问题2：本地输入命中后推进 frameCoordinator 屏幕基线，
            // 否则后续 Undo/Redo/Programmatic intent 的 pending.baseText 与
            // coordinator.lastConsumed.text 对不上，tryBuildPatch 一直返回 Empty。
            frameCoordinator.observePresentedLayout(snapshot)
            lastPresentedLayout = snapshot
            // #694 评论 5692161955 问题3：本地 commit 成功生成 local patch 后，
            // composition 已结束，重置 wasCompositionActive。
            wasCompositionActive = false
            return
        }

        // composition 活跃时只推进布局基线，不生成 patch（不播放 preedit 的吞吐）
        if (compositionActive) {
            // #694 评论 5692161955 问题3：composition 活跃分支只更新 lastPresentedLayout，
            // 不调 frameCoordinator.observePresentedLayout(snapshot)。
            // 原因：frameCoordinator.lastConsumed 是 Core/external coordinator 的基线，
            // 必须只跟 Core 已提交正文，不能推到未提交给 Core 的 preedit。
            // 否则 Undo 时 pending.baseText(Core 已提交) != lastConsumed(preedit)，external patch 卡死。
            // lastPresentedLayout 可跟 preedit（供本地输入配对），但 coordinator 基线不动。
            // #694 评论 5696394554：onAuthoritativeLayout 自己也必须能独立武装 composition phase，
            // 不能依赖 snapshotFlow — snapshotFlow 会 conflate 中间状态，可能跳过 active emission。
            // 当 compositionActive == true 且 phase 还是 Idle 时，在覆盖 lastPresentedLayout 之前，
            // 把当前已提交的 lastPresentedLayout 保存到 compositionBaseLayout，直接进入 Composing phase。
            if (compositionVisualPhase == CompositionVisualPhase.Idle) {
                compositionBaseLayout = lastPresentedLayout
                compositionVisualPhase = CompositionVisualPhase.Composing
            }
            lastPresentedLayout = snapshot
            wasCompositionActive = true
            drawSnapshotState =
                drawSnapshotState.copy(
                    layout = snapshot,
                    restingCursorRect = cursorRect,
                )
            return
        }

        // #694 评论 5692161955 问题3：composition 从 active->false 过渡同步。
        // composition 活跃期间 frameCoordinator.lastConsumed 没有推进，
        // composition 结束时若最终没有生成 local patch（localChain == null 分支），
        // 也要把最终已提交 layout 用 observePresentedLayout 同步给 coordinator，
        // 否则后续 Undo/Redo/Programmatic intent 的 pending.baseText 与 lastConsumed.text 对不上。
        if (wasCompositionActive) {
            frameCoordinator.observePresentedLayout(snapshot)
            wasCompositionActive = false
        }

        // Core visual path（Undo/Redo/Programmatic/Load/Format 等真正需要 Core 驱动的修改）
        val update = frameCoordinator.onLayout(snapshot)
        applyFrameUpdate(update)
        lastPresentedLayout = snapshot
        // 同步 draw snapshot — Core visual path 也要让 draw 层读到最新 layout/scene
        drawSnapshotState =
            drawSnapshotState.copy(
                scene = _visualScene.value,
                layout = snapshot,
                restingCursorRect = cursorRect,
            )
    }

    /**
     * #708 评论 5723410606 第三节：真正的 layout fingerprint —
     * 把 fingerprint 做成明确 data class，不再 List<Any>。
     * 包含 text + size + lineCount + 每行 start/end/top/bottom/left/right/baseline。
     *
     * selection 不纳入 fingerprint — 纯 selection 变化继续走 live selection 光标
     * （draw 层用 latestLayout + liveSelection 实时算光标），不触发布局 epoch。
     * 纯滚动也不触发布局 epoch — scrollY 变化时 text/行几何不变，fingerprint 相同，
     * 去重分支 return，不需要重新推进动画基线（纯滚动不需要重新播放动画）。
     */
    internal data class LineFingerprint(
        val start: Int,
        val end: Int,
        val top: Float,
        val bottom: Float,
        val left: Float,
        val right: Float,
        val baseline: Float,
    )

    internal data class LayoutFingerprint(
        val text: String,
        val width: Int,
        val height: Int,
        val lines: List<LineFingerprint>,
    )

    /**
     * 计算 layout 的 fingerprint。相同正文+相同几何时 fingerprint 相等。
     */
    private fun layoutFingerprint(snapshot: ComposeLayoutSnapshot): LayoutFingerprint {
        val result = snapshot.result
        val lineCount = result.lineCount
        val lines =
            (0 until lineCount).map { i ->
                LineFingerprint(
                    start = result.getLineStart(i),
                    end = result.getLineEnd(i),
                    top = result.getLineTop(i),
                    bottom = result.getLineBottom(i),
                    left = result.getLineLeft(i),
                    right = result.getLineRight(i),
                    baseline = result.getLineBaseline(i),
                )
            }
        return LayoutFingerprint(
            text = result.layoutInput.text.text,
            width = result.size.width,
            height = result.size.height,
            lines = lines,
        )
    }

    /**
     * 把帧协调器的更新结果应用到本地状态 — 暂存 patch 到待消费队列供 overlay 推进 timeline。
     */
    private fun applyFrameUpdate(update: FrameUpdate) {
        when (update) {
            is FrameUpdate.Empty -> {
                // 无新 patch — 首帧、无 pending、或 pending 与 layout 尚未匹配。
            }
            is FrameUpdate.NewPatch -> {
                pendingPatches.addLast(update.patch)
                _patchVersion.update { it + 1L }
                _latestPatch.update { update.patch }
                Log.d(
                    TAG,
                    "patch_published: id=${update.patch.id} " +
                        "coreTxnIds=${update.patch.coreTransactionIds} " +
                        "drawsVisualCursor=${_drawsVisualCursor.value}",
                )
            }
        }
    }

    /**
     * 在 Compose 帧时钟的回调里消费所有待处理的 patch 并应用到 timeline。
     *
     * overlay 监听 [patchVersion]，在 `withFrameNanos` 里调用本方法，
     * 把队列中所有 pending patch 逐个应用到 timeline。时间戳必须来自 Compose frame clock。
     *
     * #691：同时把光标 motion 并入 timeline — 与文字在同一个 applyPatch 内处理，
     * 保证 cursor 和 text units 使用同一个 frameTimeNanos。
     *
     * #691 评论 5679242735 修改2：如果 [currentMotionPolicy] 非 null，
     * 把已入队 patch 的 motionPolicy 替换成最新 policy，
     * 防止旧 patch 把文字动画重新启动。
     *
     * #691 评论 5679242735 修改3：cursor 参数从 (fromRect, toRect) 改成
     * (fromRect, path: List<CursorMotionPoint>)，支持多段路径。
     *
     * @param frameTimeNanos 当前帧时间戳（来自 Compose frame clock）。
     * @return 本次帧实际应用的 patch 列表。
     */
    fun drainPendingPatchesAtFrame(frameTimeNanos: Long): List<ComposeVisualPatch> {
        if (pendingPatches.isEmpty()) return emptyList()
        // #694 评论第 7 步：同一 VSync 不能逐笔重定向几何。
        // 一次取完这一帧的 patch，先合成一个屏幕 transition（ComposeVisualPatchBatch.compose），
        // 再只 visualTimeline.applyPatch() 一次。oldLayout=batch.first().oldLayout,
        // newLayout=batch.last().newLayout, retainedMoves 只按第一份旧 layout 和最后一份新 layout 算一次。
        val batch = mutableListOf<ComposeVisualPatch>()
        while (pendingPatches.isNotEmpty()) {
            val raw = pendingPatches.removeFirst()
            // #691 评论 5679242735 修改2：用 currentMotionPolicy 替换 patch 的 motionPolicy
            val patch = currentMotionPolicy?.let { raw.copy(motionPolicy = it) } ?: raw
            batch.add(patch)
        }
        val framePatch = ComposeVisualPatchBatch.compose(batch) ?: return emptyList()
        // #708 评论 5723410606 第二节：不再有整屏 barrier redirect —
        // 旧 timeline 不再按"已经过去了多少真实时间"偷偷向前跑，也不再从 baseScene 重定向。
        // 首帧 scene 已由 onAuthoritativeLayout 建立，timeline.applyPatch 直接从当前 timeline 状态继续。
        // 光标起点用 patch.originCursorRect / oldLayout，不从已删除的 baseScene.cursorRect 猜起点。
        val cursorParams = computeCursorParamsForPatch(framePatch, fromRectOverride = null)
        visualTimeline.applyPatch(
            patch = framePatch,
            frameTimeNanos = frameTimeNanos,
            cursorFromRect = cursorParams?.fromRect,
            cursorPath = cursorParams?.points,
            cursorDurationNanos = cursorParams?.durationNanos ?: 0L,
        )
        // #708 评论 5723410606 第二节：配对完成后清 handoff —
        // 不再有"matching layout/local patch 到齐、timeline 从 barrier.baseScene redirect"的步骤，
        // timeline.applyPatch 已直接处理，sample 出同一 frame 的新 scene 后清 handoff。
        // #708 评论 5724568261 缺口1：pendingLocalEditHandoff 已删除（死状态），
        // 首帧 scene 由 publishLocalHandoffScene 直接发布，无需在此清理。
        return listOf(framePatch)
    }

    /**
     * 是否还有待处理的 patch — overlay 据此决定是否继续推进帧时钟。
     */
    fun hasPendingPatches(): Boolean = pendingPatches.isNotEmpty()

    /**
     * #689 评论 5674631257 步骤7：在 Compose 帧时钟的回调里应用 patch 到 timeline。
     *
     * 已废弃 — 请改用 [drainPendingPatchesAtFrame]。
     * 保留此方法是为了兼容旧调用路径。
     *
     * @param patch 要应用的屏幕 diff。
     * @param frameTimeNanos 当前帧时间戳（来自 Compose frame clock）。
     */
    fun applyVisualPatchAtFrame(
        patch: ComposeVisualPatch,
        frameTimeNanos: Long,
    ) {
        visualTimeline.applyPatch(patch, frameTimeNanos)
    }

    /**
     * #689 评论 5674631257 步骤7：采样当前视觉场景 — overlay 在每帧 draw 前调用。
     *
     * 每次 sample 后把结果同步给 [_visualScene]。
     * #698 评论 5697612595：不再把 [ComposeVisualScene.hiddenRanges] 同步给对外的 hiddenRanges StateFlow —
     * 对外 hiddenRanges 已删除。draw 层（[EditorTextFieldDrawLayer]）直接读 [visualScene].hiddenRanges
     * 做正文裁切，不再回流给 OutputTransformation。
     *
     * @param frameTimeNanos 当前帧时间戳（来自 Compose frame clock）。
     * @return 当前应绘制的视觉场景。
     */
    fun sampleVisualScene(frameTimeNanos: Long): ComposeVisualScene {
        val scene = visualTimeline.sample(frameTimeNanos)
        _visualScene.update { scene }
        // #708 评论 5723410606 第一节：同步 draw snapshot 的 scene —
        // draw 层下一帧 drawWithContent 直接读，不在 Composable 主体读 visualScene StateFlow。
        drawSnapshotState = drawSnapshotState.copy(scene = scene)
        return scene
    }

    /**
     * 是否还有活动动画 — overlay 据此决定是否继续推进帧时钟。
     *
     * @param frameTimeNanos 当前帧时间戳。
     */
    fun hasActiveVisuals(frameTimeNanos: Long): Boolean {
        return visualTimeline.hasActiveAnimation(frameTimeNanos)
    }

    // ==================== #691 统一光标位置 ====================

    /**
     * #691 评论 5679242735 修改3：光标 motion 参数 — fromRect + 完整 path + durationNanos。
     *
     * internal 可见性以便测试访问。
     */
    internal data class CursorMotionParams(
        val fromRect: Rect,
        val points: List<CursorMotionPoint>,
        val durationNanos: Long,
    )

    /**
     * #691：计算 patch 的光标 motion 参数 — 返回 [CursorMotionParams] 或 null。
     * 由 [drainPendingPatchesAtFrame] 传入 [ComposeVisualTimeline.applyPatch]，
     * 保证 cursor 和 text units 使用同一个 frameTimeNanos。
     *
     * #691 评论 5679242735 修改3：返回完整 path（List<CursorMotionPoint>），
     * 不再只取 path.points.last().rect。多字符一次提交时多段 cursor path 不再被压成一条直线。
     */
    private fun computeCursorParamsForPatch(
        patch: ComposeVisualPatch,
        fromRectOverride: Rect? = null,
    ): CursorMotionParams? {
        val motionPolicy = patch.motionPolicy.effective()
        if (!motionPolicy.cursorEnabled) {
            // 光标动画关闭 — 不创建 cursorChannel，使用静态光标
            return null
        }

        val path = patch.cursorMotionPath
        if (path == null || path.points.isEmpty()) {
            // 无光标 motion — snap 到新 layout 的光标位置，返回单点 path
            val newCursorRect = computeCursorRectFromLayout(patch.newLayout) ?: return null
            return CursorMotionParams(
                fromRect = newCursorRect,
                points = listOf(CursorMotionPoint(rect = newCursorRect, endFraction = 1f)),
                durationNanos = 0L,
            )
        }

        // #691 评论 5679242735 修改3：fromRect = 旧 layout 真实光标位置
        // （取不到回退 path.points.first().rect），points = path.points 完整保留。
        //
        // #703 评论 5709208101 问题3：fromRect 优先用 patch.originCursorRect
        // （本地编辑从 chain.first().oldSelection.end + oldLayout.result 取的真实 T0 caret），
        // 不依赖 patch.oldLayout.selection（纯 selection 变化后可能 stale）。
        // Core/external 路径 originCursorRect 为 null，回退到 computeCursorRectFromLayout(patch.oldLayout)。
        //
        // #706 评论 5718539128 修复1：fromRectOverride 优先级最高 —
        // barrier handoff 时传 barrier.baseScene.cursorRect（用户最后真正看到的屏幕光标位置），
        // 不从 patch.originCursorRect / oldLayout 猜起点（旧 layout 可能已 stale 或与屏幕不一致）。
        val fromRect =
            fromRectOverride
                ?: patch.originCursorRect
                ?: computeCursorRectFromLayout(patch.oldLayout)
                ?: path.points.first().rect

        // #691 评论 5686733880：cursor 时长决定逻辑收口到此一处。
        // 只有真正的协同文字事务才用 textDurationMillis，必须同时满足四个条件：
        //   1. textEnabled=true（文字动画开启，才有"文字事务时长"可言）
        //   2. cursorEnabled=true（光标动画开启）
        //   3. coordinated=true（用户选择协同）
        //   4. 不是 CURSOR_ONLY（有 insertedUnits/deletedUnits/retainedMoves 文字视觉变化）
        // 任意一个不满足都用 cursorDurationMillis。
        //
        // 这覆盖设置矩阵 D（textEnabled=false, cursorEnabled=true, coordinated=true）：
        // 即使 coordinated=true 且 patch 含 insertedUnits（isCursorOnly=false），
        // 因为 textEnabled=false，也不应使用 textDurationMillis，而应使用 cursorDurationMillis。
        // 旧逻辑只判断 `motionPolicy.coordinated && !isCursorOnly`，漏掉 textEnabled/cursorEnabled，
        // 导致 textEnabled=false 时仍错误走到 textDurationMillis 分支。
        val isCursorOnly =
            patch.insertedUnits.isEmpty() &&
                patch.deletedUnits.isEmpty() &&
                patch.retainedMoves.isEmpty()
        val usesCoordinatedTextTimeline =
            motionPolicy.textEnabled &&
                motionPolicy.cursorEnabled &&
                motionPolicy.coordinated &&
                !isCursorOnly
        val effectiveDurationNanos =
            if (usesCoordinatedTextTimeline) {
                // 真正的协同文字事务：光标与文字共享 textDurationMillis
                // 作为整条编辑视觉事务时长（用户设置）。
                motionPolicy.textDurationMillis.coerceAtLeast(0L) * NANOS_PER_MS
            } else {
                // 其他所有情况（textEnabled=false / cursorEnabled=false /
                // coordinated=false / CURSOR_ONLY）：光标使用独立的 cursorDurationMillis。
                motionPolicy.cursorDurationMillis.coerceAtLeast(0L) * NANOS_PER_MS
            }

        return CursorMotionParams(
            fromRect = fromRect,
            points = path.points,
            durationNanos = effectiveDurationNanos,
        )
    }

    /**
     * #691：从 layout + selection 计算光标 rect（屏幕坐标）。
     */
    private fun computeCursorRectFromLayout(layout: ComposeLayoutSnapshot): Rect? {
        return try {
            val selectionEnd =
                layout.selection.end.coerceIn(0, layout.result.layoutInput.text.length)
            layout.cursorRect(selectionEnd)
        } catch (_: Throwable) {
            null
        }
    }

    /**
     * 清除所有状态 — 章节切换或 detach 时调用。
     */
    fun clear() {
        frameCoordinator.clear()
        visualTimeline.clear()
        pendingPatches.clear()
        _patchVersion.update { 0L }
        _latestLayout.update { null }
        _visualScene.update { ComposeVisualScene.Empty }
        _latestPatch.update { null }
        _restingCursorRect.update { null }
        // #691 评论 5679242735 修改2：重置运行时 policy 切换状态
        currentMotionPolicy = null
        // #694 评论第 3 步：清空本地输入配对状态
        localInputTracker.clear()
        lastPresentedLayout = null
        // #694 评论 5692161955 问题3：重置 composition 过渡同步状态
        wasCompositionActive = false
        // #694 评论 5693864609 问题2：重置 composition 独立生命周期状态
        compositionBaseLayout = null
        pendingCompositionCommitText = null
        wasCompositionActiveForSnapshot = false
        // #694 评论 5695660885 问题1：重置 composition 视觉生命周期 phase
        compositionVisualPhase = CompositionVisualPhase.Idle
        // #708 评论 5723410606 第一节/第二节/第三节：重置 draw snapshot / fingerprint
        // （pendingLocalEditHandoff 已删除 — 缺口1 死状态移除）
        drawSnapshotState = ComposeEditorDrawSnapshot()
        lastObservedLayoutFingerprint = null
        nextHandoffUnitKey = 2_000_000L
        // 光标所有权只由设置/attach 决定，clear 不重置 _drawsVisualCursor。
    }

    /**
     * 设置 smooth cursor 状态 — 由外部设置变更驱动。
     * smooth cursor 开启时，编辑器 attach 以后系统光标一直透明。
     */
    fun setSmoothCursorEnabled(enabled: Boolean) {
        _drawsVisualCursor.update { enabled }
    }

    /**
     * #691 评论 5679242735 修改2：运行时 policy 切换 — 在帧边界应用新 motion policy。
     *
     * 场景：patch 已入队但还没 drain，此时用户关闭文字动画或打开 reduce-motion；
     * 旧 patch 会带着原来的 insertedUnits/deletedUnits/retainedMoves 再进入 timeline，
     * 把文字动画重新启动。本方法：
     * 1. 记录最新 effective policy 到 [currentMotionPolicy]，
     *    [drainPendingPatchesAtFrame] 会用它替换已入队 patch 的 motionPolicy。
     * 2. 调用 [ComposeVisualTimeline.settleForPolicyChange] 清掉旧 text units / ghost / cursorChannel。
     * 3. 把已入队 patch 的 motionPolicy 替换成最新 policy（防止旧 patch 重新启动文字动画）。
     *
     * @param newPolicy 新的动画策略 — 内部会先 effective() 收口 reduce-motion。
     */
    fun applyMotionPolicyAtFrame(newPolicy: EditorMotionPolicy) {
        val effective = newPolicy.effective()
        currentMotionPolicy = effective
        // 清掉旧 text units / ghost / cursorChannel
        visualTimeline.settleForPolicyChange()
        // #691 评论 5679815971 问题1：一次性同步所有 UI 状态，
        // 不要让 setSmoothCursorEnabled() 和 motion policy 走两套生命周期。
        // smooth cursor 所有权跟随 policy.cursorEnabled；
        // 已发布给 Compose 的旧 visualScene 清空，直到下一次 sampleVisualScene() 重建。
        _drawsVisualCursor.update { effective.cursorEnabled }
        _visualScene.update { ComposeVisualScene.Empty }
        // #708 评论 5723410606 第一节：同步清 draw snapshot 的 scene
        drawSnapshotState = drawSnapshotState.copy(scene = ComposeVisualScene.Empty)
        // 把已入队 patch 的 motionPolicy 替换成最新 policy
        if (pendingPatches.isNotEmpty()) {
            val updated = mutableListOf<ComposeVisualPatch>()
            while (pendingPatches.isNotEmpty()) {
                val p = pendingPatches.removeFirst()
                updated.add(p.copy(motionPolicy = effective))
            }
            updated.forEach { pendingPatches.addLast(it) }
        }
    }
}
