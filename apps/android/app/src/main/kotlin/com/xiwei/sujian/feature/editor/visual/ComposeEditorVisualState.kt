package com.xiwei.sujian.feature.editor.visual

import android.util.Log
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
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
 */
class ComposeEditorVisualState(
    private val targetId: String,
    initialDrawsVisualCursor: Boolean = false,
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
     * 当前应由 overlay 接管、BasicTextField 需设透明的 ranges —
     * 每一帧直接从当前 [VisualTextUnit.targetRange] != null 且仍由 overlay 绘制的 unit 推导，
     * 不从"上一事务 suppressed ranges"继承。
     */
    private val _hiddenRanges = MutableStateFlow<List<TextRange>>(emptyList())
    val hiddenRanges: StateFlow<List<TextRange>> = _hiddenRanges.asStateFlow()

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

        // #694 评论 5692161955 问题1/2：调用 Core 纯计算 API classify_local_visual_plan
        // 做视觉分类，得到 animationMode 和按 grapheme cluster 拆分的 animation units。
        // 不再硬编码 CLUSTER_ANIMATION，不再按 UTF-16 +1 硬切。
        // Core API 不可用时（如 Robolectric 测试环境）回退到 Kotlin fallback
        // （java.text.BreakIterator + chooseAnimationMode 投影）。
        val corePlan =
            ComposeLocalVisualRebase.classifyLocalVisualPlanFromCore(
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
                        // #694 评论 5692161955 问题1：优先用 Core plan 的 grapheme cluster units；
                        // 其次用合成的 ordered units 保留吐字顺序；
                        // 最后回退到净变化 newRanges。
                        when {
                            planInsertedUnits.isNotEmpty() -> planInsertedUnits
                            composedInserted.isNotEmpty() -> composedInserted
                            else -> changedRanges.newRanges
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
                        when {
                            planDeletedUnits.isNotEmpty() -> planDeletedUnits
                            composedDeleted.isNotEmpty() -> composedDeleted
                            else -> changedRanges.oldRanges
                        }
                    }
                    TextVisualKind.Insert, TextVisualKind.None -> emptyList()
                }
            } else {
                emptyList()
            }

        // retained reflow 只比较 oldLayout -> newLayout 的真实几何
        val retainedMoves =
            ComposeLocalVisualRebase.computeRetainedMoves(oldLayout, newLayout, offsetMap)

        // cursor 从 chain 首笔 oldSelection.end -> 末笔 newSelection.end 构造
        val cursorMotionPath =
            ComposeLocalVisualRebase.buildCursorPath(
                oldLayout = oldLayout,
                newLayout = newLayout,
                oldSelection = firstEdit.oldSelection,
                newSelection = lastEdit.newSelection,
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
     * 每次 sample 后把 [ComposeVisualScene.hiddenRanges] 同步给 [_hiddenRanges]，
     * [OutputTransformation] 继续只负责把这些正在由 overlay 画的最终正文 range 设透明。
     *
     * @param frameTimeNanos 当前帧时间戳（来自 Compose frame clock）。
     * @return 当前应绘制的视觉场景。
     */
    fun sampleVisualScene(frameTimeNanos: Long): ComposeVisualScene {
        val scene = visualTimeline.sample(frameTimeNanos)
        _visualScene.update { scene }
        _hiddenRanges.update { scene.hiddenRanges }
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
        _hiddenRanges.update { emptyList() }
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
        // 已发布给 Compose 的旧 visualScene/hiddenRanges 清空，直到下一次 sampleVisualScene() 重建。
        _drawsVisualCursor.update { effective.cursorEnabled }
        _hiddenRanges.update { emptyList() }
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
