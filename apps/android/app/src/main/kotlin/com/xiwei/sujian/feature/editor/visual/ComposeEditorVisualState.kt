package com.xiwei.sujian.feature.editor.visual

import android.util.Log
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.input.EditorInputSnapshot
import com.xiwei.sujian.feature.editor.input.InputSnapshotOutcome
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
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
        val retainedMoves =
            ComposeLocalVisualRebase.computeRetainedMoves(oldLayout, newLayout, offsetMap)

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
            cursorMotionPath = cursorMotionPath,
            // 本地输入时长由 motionPolicy 决定（timeline 用 policy.textDurationMillis）
            durationMs = 0L,
            // #694 评论 5692161955 问题2：使用 Core plan 返回的 animationMode，
            // 不再硬编码 CLUSTER_ANIMATION。
            animationMode = planAnimationMode,
            motionPolicy = motionPolicy,
            intent = null,
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
            compositionVisualPhase = CompositionVisualPhase.AwaitingBridgeResolution
            return
        }

        // #691：更新静止光标 rect
        val cursorRect = computeCursorRectFromLayout(snapshot)
        _restingCursorRect.update { cursorRect }

        // #698 评论 5697612595 / 5699401353 修复3：真实 layout 去重 —
        // 相同正文/几何不能重复推进动画基线，防止 onTextLayout 因非真实变化重复触发形成回路
        // （动画 hiddenRanges -> OutputTransformation 改正文显示 -> BasicTextField 再 layout ->
        // VisualState 再消费 layout）。只在 !compositionActive 时检查：composition 活跃时 preedit
        // 可能正在变化，即使此刻正文/几何与 lastPresentedLayout 相同，也需要进入 composition 分支武装 phase。
        // 去重分支只更新 _latestLayout（已在上方更新）和 _restingCursorRect（已在上方更新），
        // 直接 return，不 drain localInputTracker、不发布 patch、不推进 frameCoordinator、不更新 lastPresentedLayout。
        // "正文+几何相同"定义（#698 评论 5699401353 修复3）：text + size + lineCount +
        // 每行 start/end/top/bottom/left/right/baseline 全部相同（[layoutFingerprint]）。
        // selection 不纳入 fingerprint — 纯 selection 变化时 draw 层用 latestLayout + liveSelection
        // 实时算光标（computeRestingCursorRect），不依赖 lastPresentedLayout.selection，不需要重新推进基线。
        // 纯滚动也不触发布局 epoch — scrollY 变化时行几何不变，fingerprint 相同，去重 return，
        // 不需要重新推进动画基线（纯滚动不需要重新播放动画）。
        if (!compositionActive && hasSameTextAndGeometry(lastPresentedLayout, snapshot)) {
            return
        }

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
    }

    /**
     * #698 评论 5697612595 / 5699401353 修复3：判断新 snapshot 与上次呈现的 layout 是否"正文+几何相同"。
     *
     * 旧实现只有 `text 相同 && size 相同 && lineCount 相同`。同样的 size/lineCount，
     * 行起止 offset、每行 top/bottom/left/right/baseline 仍可能变化。误判后直接 return，
     * 让后续动画继续拿旧几何。
     *
     * 新实现建立真正的 layout fingerprint（[layoutFingerprint]），至少包括：
     * - text
     * - size（width/height）
     * - lineCount
     * - 每行 start/end（getLineStart/getLineEnd）
     * - 每行 top/bottom/left/right（getLineTop/getLineBottom/getLineLeft/getLineRight）
     * - 每行 baseline（getLineBaseline）
     *
     * selection 不纳入 fingerprint — 纯 selection 变化继续走 live selection 光标
     * （draw 层用 latestLayout + liveSelection 实时算光标），不触发布局 epoch。
     * 纯滚动也不触发布局 epoch — scrollY 变化时 text/行几何不变，fingerprint 相同，
     * 去重分支 return，不需要重新推进动画基线（纯滚动不需要重新播放动画）。
     *
     * 用于 [onAuthoritativeLayout] 去重，防止 onTextLayout 因非真实变化重复触发形成回路。
     *
     * @param last 上次真正呈现的 layout；null 时返回 false。
     * @param snapshot 本次权威 layout。
     * @return true 表示正文+几何相同（可去重，不推进动画基线）。
     */
    private fun hasSameTextAndGeometry(
        last: ComposeLayoutSnapshot?,
        snapshot: ComposeLayoutSnapshot,
    ): Boolean {
        if (last == null) return false
        return layoutFingerprint(snapshot) == layoutFingerprint(last)
    }

    /**
     * #698 评论 5699401353 修复3：真正的 layout fingerprint —
     * 把 text + size + lineCount + 每行 start/end/top/bottom/left/right/baseline
     * 全部纳入比较，避免同 size/lineCount 但行几何变化时误判为相同。
     *
     * @param snapshot layout 快照。
     * @return fingerprint 值列表（String/Int/Float 元素，用 List.equals 精确比较）。
     */
    private fun layoutFingerprint(snapshot: ComposeLayoutSnapshot): List<Any> {
        val result = snapshot.result
        val lineCount = result.lineCount
        return buildList {
            add(result.layoutInput.text.text)
            add(result.size.width)
            add(result.size.height)
            add(lineCount)
            for (i in 0 until lineCount) {
                add(result.getLineStart(i))
                add(result.getLineEnd(i))
                add(result.getLineTop(i))
                add(result.getLineBottom(i))
                add(result.getLineLeft(i))
                add(result.getLineRight(i))
                add(result.getLineBaseline(i))
            }
        }
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
        // #691：光标 motion 与文字在同一个 applyPatch 调用内处理
        val cursorParams = computeCursorParamsForPatch(framePatch)
        visualTimeline.applyPatch(
            patch = framePatch,
            frameTimeNanos = frameTimeNanos,
            cursorFromRect = cursorParams?.fromRect,
            cursorPath = cursorParams?.points,
            cursorDurationNanos = cursorParams?.durationNanos ?: 0L,
        )
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
    private fun computeCursorParamsForPatch(patch: ComposeVisualPatch): CursorMotionParams? {
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
        val fromRect = computeCursorRectFromLayout(patch.oldLayout) ?: path.points.first().rect

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
            layout.result.getCursorRect(selectionEnd)
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
