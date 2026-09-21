package com.xiwei.sujian.feature.editor.visual

import android.util.Log
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.core.interop.diagnostics.EditorDiagnosticsEvents
import com.xiwei.sujian.feature.editor.input.EditorInputSnapshot
import com.xiwei.sujian.feature.editor.input.InputSnapshotOutcome
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.layout.boundsForRawRange
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
 * @param classifier 本地视觉 plan 分类器 — 生产环境默认 [CoreLocalVisualPlanClassifier] 直接调 Core，
 *   测试环境（Robolectric）注入 fake 绕过原生库加载。
 */
class ComposeEditorVisualState(
    private val targetId: String,
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

    /**
     * Issue #728 评论 5754045689：统一编辑 motion —
     * 一笔编辑只创建一个 motion，同一只钟驱动 caret 移动和文字吞吐。
     * null 表示无 active motion（首帧/动画完成/policy 切换后）。
     */
    private var activeEditMotion: ComposeEditMotion? = null

    /** 最新 layout 快照 — 供 overlay 读取 bounding box。 */
    private val _latestLayout = MutableStateFlow<ComposeLayoutSnapshot?>(null)
    val latestLayout: StateFlow<ComposeLayoutSnapshot?> = _latestLayout.asStateFlow()

    /**
     * 当前视觉场景 — overlay 读取绘制。
     * 每次 [sampleVisualScene] 后更新。
     */
    private val _visualScene = MutableStateFlow(ComposeVisualScene.Empty)
    val visualScene: StateFlow<ComposeVisualScene> = _visualScene.asStateFlow()

    /**
     * 待消费的 patch 队列 — 解决快速输入时 LaunchedEffect 取消旧协程导致丢 patch 的问题。
     * 使用队列而非 conflated state，确保每一笔 patch 都能被处理。
     *
     * Issue #723 评论 5750100004：队列元素是 [PendingPatch]（携带统一入队序号 [PendingPatch.sequence]），
     * 不再直接存 [ComposeVisualPatch]。release/drain 按 sequence 判断"release 之前/之后"，
     * 不受 [ComposeVisualPatch.id] 两套来源（Core 从 0 起、local 从 1_000_000 起）影响。
     */
    private val pendingPatches = ArrayDeque<PendingPatch>()

    /**
     * Issue #723 评论 5750100004：pending patch 统一入队序号 —
     * 任何来源（本地输入 / Core / external）的 patch 入 [pendingPatches] 时统一分配，
     * 从 0 起单调递增。与 [ComposeVisualPatch.id]（两套来源、不连续）解耦，
     * 只表达"入队先后"。
     */
    private var nextPendingSequence: Long = 0L

    private val _patchVersion = MutableStateFlow(0L)
    val patchVersion: StateFlow<Long> = _patchVersion.asStateFlow()

    /**
     * 最新生成的 patch — 仅保留给日志/调试使用，timeline 输入不再依赖它。
     */
    private val _latestPatch = MutableStateFlow<ComposeVisualPatch?>(null)
    val latestPatch: StateFlow<ComposeVisualPatch?> = _latestPatch.asStateFlow()

    /**
     * #691 评论 5679242735 修改2：运行时 policy 切换的最新 effective policy。
     *
     * 非 null 时，[drainPendingPatchesAtFrame] 会把已入队 patch 的 motionPolicy 替换成它，
     * 防止旧 patch 带着原来的 insertedUnits/deletedUnits/retainedMoves 再进入 timeline
     * 把文字动画重新启动（用户已关闭文字动画或打开 reduce-motion）。
     */
    private var currentMotionPolicy: EditorMotionPolicy? = null

    /**
     * Issue #720 评论 5747339452：测试用 override — 非 null 时 [buildLocalInputPatch] 生成的
     * patch 使用此 intent 而非 null，绕过本地 reflow 释放门控
     * （[ComposeLocalHandoffRebase.rebase] / [ComposeVisualTimeline.mapSurvivingUnits]
     * 中 `patch.intent == null && naturalGeometryChanged` 判定）。
     *
     * Robolectric 下 [TextLayoutResult.getPathForRange] 跨文本 bounds 不稳定
     * （同 range 在不同文本中 left/right 不同），导致 [ComposeVisualRebase.naturalGeometryChanged]
     * 误判为几何变化、survivor 被误释放。#708 系列测试验证的是 rebase/split 机制
     * （非 #720 释放），用非 null intent 绕过释放门控。生产环境保持 null。
     */
    @androidx.annotation.VisibleForTesting
    internal var localInputIntentOverride: EditorVisualIntent? = null

    /**
     * #713 评论 5739986801：上一次 resolved 的 selection —
     * 用于检测纯 selection 变化（text 不变、composition 为空、selection 变了）。
     */
    private var lastResolvedSelection: TextRange? = null

    /**
     * Issue #723 评论 5750100004：pending patch 队列项 —
     * 携带只属于本 visual state 的单调入队序号 [sequence]，与 [ComposeVisualPatch.id] 解耦。
     *
     * [ComposeVisualPatch.id] 有两套来源（Core/external 从 0 起、local 从 1_000_000 起），
     * 不能跨来源表达"入队先后"。[sequence] 由 [nextPendingSequence] 统一分配，
     * 任何来源的 patch 入 pendingPatches 时都拿到一个连续递增的 sequence，
     * release/drain 按此 sequence 判断"release 之前/之后"，不受两套 patch.id 影响。
     */
    private data class PendingPatch(
        val sequence: Long,
        val patch: ComposeVisualPatch,
    )

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
     * #708 评论 5729482707 修复3：handoff 临时 unit key 唯一 allocator —
     * 所有 handoff 临时 unit（split child、remaining delete ghost）统一走此入口，
     * 不再手写 ++，避免两种自增写法混用导致连续 handoff 撞 key。
     */
    private fun allocateHandoffUnitKey(): Long = nextHandoffUnitKey++

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
                    // Issue #717 评论 5742904417 修复1：文本身份用 rawText（不含 U+200B）。
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
                    // Issue #717 评论 5742904417 修复1：文本身份用 rawText（不含 U+200B）。
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
        // #713 评论 5740279418：selection 真正变化时记一次诊断事件
        if (snapshot.selection != lastResolvedSelection) {
            EditorDiagnosticsEvents.editorSelectionChanged(
                oldStart = lastResolvedSelection?.start ?: -1,
                oldEnd = lastResolvedSelection?.end ?: -1,
                newStart = snapshot.selection.start,
                newEnd = snapshot.selection.end,
                // Issue #717 评论 5742904417 修复1：正文长度记 raw 长度。
                layoutTextLength = _latestLayout.value?.result?.layoutInput?.text?.text?.length ?: -1,
            )
        }
        lastResolvedSelection = snapshot.selection
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
     *    （不再从旧 hiddenRanges 复制），再加 [patch.insertedUnits] 的 newRange。
     *    这确保 hiddenRanges 和 newLayout 属于同一个坐标系。
     * 3. **deletedUnits 只补差集**：先收集 rebase 阶段已经转成 ghost 的旧正文范围（ghostedCoverage），
     *    只给"没有被旧 active unit 接管"的 deletedUnits 新建 alpha=1 的完整 ghost。
     *    这避免同一 glyph 同时出现 Inserted + DeletedGhost 的重影。
     * 4. **Cursor**：保持现有首帧光标处理逻辑。
     *
     * @param patch 本笔 local patch（含 insertedUnits/deletedUnits/retainedMoves）。
     * @param oldLayout T0 布局（建 ghost 来源）。
     * @param newLayout Tn 布局（unit 所属 layout）。
     */
    private fun publishLocalHandoffScene(
        patch: ComposeVisualPatch,
        oldLayout: ComposeLayoutSnapshot,
        newLayout: ComposeLayoutSnapshot,
    ) {
        _visualScene.update { scene ->
            // #708 评论 5725706551 步骤1：调用 ComposeLocalHandoffRebase.rebase 做 scene rebase —
            // 把旧 visible scene（旧正文坐标）映射到新正文坐标系。
            // 旧 active unit 通过 splitMappedRangeForward 映射到新正文坐标：
            //   - 存活 slice：targetRange/range 改成 newRange，layout 改成 newLayout，alpha/position 固定在当前可见值；
            //   - 被删除 slice：从当前可见 alpha/position 转 handoff ghost（不新建 alpha=1 的完整 ghost）；
            //   - 已有 ghost：保持当前状态不变。
            // ghostedCoverage 记录 rebase 阶段已经转成 ghost 的旧正文范围。
            // #708 评论 5727808906：传入 key allocator — split 时为每个子 unit 分配独立新 key，
            // 不再共用父 key，避免 unitClipFractions 同 key 互相覆盖。
            val rebased = ComposeLocalHandoffRebase.rebase(scene, patch) { allocateHandoffUnitKey() }
            val rebasedUnits = rebased.units.toMutableList()

            // #708 评论 5725706551 步骤2：hiddenRanges 从 rebase 后所有 targetRange != null 的 unit 重新推导 —
            // 不再从旧 hiddenRanges 复制（旧坐标系），确保 hiddenRanges 和 newLayout 属于同一个坐标系。
            //
            // Issue #720 评论 5746323050：不再补第二套 reflow。
            // handoff rebase 仍保留 surviving slice（rebase 到新坐标 + alpha 动画），
            // ComposeVisualTimeline.mapSurvivingUnits() 在本地 patch + 跨行 reflow 时释放它（不产生 position tween）。
            // handoff scene 是 patch drain 前的瞬态，survivor 已 rebase 到新位置；
            // drain 后 timeline 不持有它，BasicTextField 直接画最终位置。
            // hiddenRanges 从 rebasedUnits.targetRange 重建，与 handoff 保持一致。
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
            // #708 评论 5731952690 修复1a：不查所有历史 rebasedUnits 做去重 —
            // 旧实现 `rebasedUnits.any { it.targetRange == null && it.range == del }` 查所有历史
            // rebasedUnits，连续 Forward Delete 时历史 a ghost（range=[0,1) layout="ab"）会挡住
            // 本次 b ghost（range=[0,1) layout="b"）的创建。remainingDeleted 已做过
            // deletedUnits - ghostedCoverage，防御性去重只查本次 handoff 新建 ghost 的 range 集合，
            // 不查历史 rebasedUnits。
            // #708 评论 5731952690 修复3：记录本次 handoff 新建 remaining delete ghost 的 key —
            // 这些 ghost 从 oldLayout 新建（alpha=1），上一帧在 BasicTextField 完整可见，
            // T0 fraction 应为 1（完整可见），不闪没。
            val handoffNewGhostRanges = mutableSetOf<TextRange>()
            val remainingGhostKeys = mutableSetOf<Long>()
            for (del in remainingDeleted) {
                if (del.start >= del.end) continue
                // 防御性去重：只查本次 handoff 新建的 ghost range，不查历史 rebasedUnits
                if (del in handoffNewGhostRanges) continue
                // 从 oldLayout 取旧位置建立静态 ghost
                // Issue #717 评论 5742273757 修复3：del 是 raw 坐标，通过 snapshot 做 raw→display。
                val oldBounds =
                    oldLayout.boundsForRawRange(del) ?: continue
                val oldPosition = Offset(oldBounds.left, oldBounds.top)
                val ghostKey = allocateHandoffUnitKey()
                rebasedUnits +=
                    VisualTextUnit(
                        key = ghostKey,
                        layout = oldLayout,
                        range = del,
                        targetRange = null,
                        // 完整可见：alpha=1，reveal=1，不动画
                        alpha = TimedFloat(1f, 1f, 0L, 0L),
                        position = TimedOffset(oldPosition, oldPosition, 0L, 0L),
                        reveal = TimedFloat(1f, 1f, 0L, 0L),
                        role = VisualUnitRole.DeletedGhost,
                    )
                handoffNewGhostRanges.add(del)
                remainingGhostKeys.add(ghostKey)
            }

            // Issue #725 评论 5750735497：停止自绘屏幕 caret —
            // 不再计算 handoffCursorRect / handoffCursor，clipFraction 改由 alpha 通道直接驱动。
            // rebasedClipFractions 只继承 rebase 阶段记录的真实首帧 fraction 和 remaining delete ghost 的 fraction=1。
            val rebasedClipFractions =
                run {
                    val clipMap = mutableMapOf<Long, Float>()
                    for (child in rebasedUnits) {
                        if (child.key in remainingGhostKeys) {
                            clipMap[child.key] = 1f
                            continue
                        }
                        val initialFraction =
                            rebased.initialClipFractionsByKey[child.key]
                                ?: scene.unitClipFractions[child.key]
                        if (child.targetRange == null && initialFraction != null) {
                            // ghost slice：继承真实首帧 fraction
                            clipMap[child.key] = initialFraction
                        }
                        // initialFraction == null：不写入 map，draw 层用默认 fraction=1，由 alpha 主导
                        // surviving slice：不加 map，timeline 重算
                    }
                    clipMap
                }
            scene.copy(
                hiddenRanges = mergedHidden,
                units = rebasedUnits,
                unitClipFractions = rebasedClipFractions,
                // Issue #725：自绘 caret 已删除，cursorEnabled 不再参与计算。
                coordinatedSpatialClip =
                    patch.motionPolicy.effective().textEnabled &&
                        patch.motionPolicy.effective().coordinated,
            )
        }
        // 同步把首帧 scene 写进 draw snapshot — draw 层下一帧 drawWithContent 直接读
        drawSnapshotState =
            drawSnapshotState.copy(
                scene = _visualScene.value,
                layout = newLayout,
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
        // Issue #717 评论 5742904417 修复1：文本身份用 rawText（不含 U+200B）。
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
                    )
                    pendingPatches.addLast(PendingPatch(sequence = nextPendingSequence++, patch = localPatch))
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
        // Issue #717 评论 5742904417 修复1：文本身份用 rawText（不含 U+200B）。
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
        // #711 评论 5738906634：删除 ReflowMove 路线 —
        // 软换行几何已由 BasicTextField + TextLayoutResult 给出，不再自建第二套幸存文字位移系统。
        // 没被插入、没被删除、只是因为系统软换行换了位置的正文，永远不进 hiddenRanges，
        // 直接让 BasicTextField 画最终位置。
        val retainedMoves = emptyList<RetainedMove>()

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
            // 本地输入时长由 motionPolicy 决定（timeline 用 policy.textDurationMillis）
            durationMs = 0L,
            // #694 评论 5692161955 问题2：使用 Core plan 返回的 animationMode，
            // 不再硬编码 CLUSTER_ANIMATION。
            animationMode = planAnimationMode,
            motionPolicy = motionPolicy,
            // Issue #720 评论 5747339452：默认 null（本地输入 → reflow 释放门控生效）；
            // 测试可通过 [localInputIntentOverride] 注入非 null intent 绕过门控，
            // 以验证 rebase/split 机制（#708 系列）。
            intent = localInputIntentOverride,
        )
    }

    /**
     * 系统给出权威布局 — 只记录，不修改输入几何。
     *
     * 得到 patch 后不要启动一笔新事务，只把 patch 暂存/发布给 overlay 的时间线入口。
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
     * @param projection Issue #717 评论 5741910919：西文软断行显示投影，
     *     把 raw offset 映射到含 U+200B 的 display offset。
     * @param rawText Issue #717 评论 5742904417 修复1 / 评论 5743443030 修复1：原始正文（不含 U+200B），
     *     用于文本身份/diff/intent 匹配。visual pipeline 的文本身份判断必须用 rawText。
     *     `null` 表示"没传"（测试/旧调用方 fallback 到 result.layoutInput.text.text），
     *     非 `null` 表示显式传入真实 raw 正文（含 `""` 真实空正文）。
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
            // 纯 selection/caret 变化：不更新 layout epoch、不调 frameCoordinator、不重新发布相同 TextLayoutResult。
            // Issue #725 评论 5750735497：屏幕 caret 始终由 BasicTextField 自己画，不再更新 restingCursorRect。
            drawSnapshotState =
                drawSnapshotState.copy(
                    layout = snapshot,
                )
            return
        }
        lastObservedLayoutFingerprint = fingerprint

        // 真实 text/line geometry 变化：把新 layout 写进 _latestLayout 和 draw snapshot
        _latestLayout.update { snapshot }

        // #694 评论 5693864609 问题2：composition 结束后 final text 对应的新 layout 还没到时，
        // 暂存 pendingCompositionCommitText，等下一份 onAuthoritativeLayout 到达时收口。
        val pending = pendingCompositionCommitText
        // Issue #717 评论 5742904417 修复1：文本身份用 rawText（不含 U+200B）。
        val newTextForPending = snapshot.result.layoutInput.text.text
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
            drawSnapshotState =
                drawSnapshotState.copy(
                    layout = snapshot,
                )
            compositionVisualPhase = CompositionVisualPhase.AwaitingBridgeResolution
            return
        }

        // #694 评论第 3 步 + 评论 5691696678 问题1：配对 pending local edit chain 生成 ComposeVisualPatch(intent=null)。
        // composition 活跃时只推进布局基线，不播放 preedit 的吞吐。
        // 用 drainMatchingChain 按 lastPresentedLayout.text -> newText 找连续 chain，
        // 修复快速输入中间 layout 被跳过时旧 drainMatching 只返回最后一笔导致 patch 被丢的问题。
        // Issue #717 评论 5742904417 修复1：文本身份用 rawText（不含 U+200B）。
        val newText = snapshot.result.layoutInput.text.text
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
                    )
                    pendingPatches.addLast(PendingPatch(sequence = nextPendingSequence++, patch = localPatch))
                    _patchVersion.update { it + 1L }
                    _latestPatch.update { localPatch }
                    Log.d(
                        TAG,
                        "local_patch_published: id=${localPatch.id} " +
                            "oldLen=${oldLayout.result.layoutInput.text.text.length} " +
                            "newLen=${newText.length} chainSize=${localChain.size}",
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
            // Issue #717 评论 5742904417 修复1：fingerprint 用 rawText（不含 U+200B），
            // 与 visual pipeline 文本身份判断一致。
            text = snapshot.result.layoutInput.text.text,
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
                pendingPatches.addLast(PendingPatch(sequence = nextPendingSequence++, patch = update.patch))
                _patchVersion.update { it + 1L }
                _latestPatch.update { update.patch }
                Log.d(
                    TAG,
                    "patch_published: id=${update.patch.id} " +
                        "coreTxnIds=${update.patch.coreTransactionIds}",
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
     */
    fun hasPendingPatches(): Boolean = pendingPatches.isNotEmpty()

    /**
     * 在 Compose 帧时钟的回调里消费所有待处理的 patch 并应用到 timeline。
     *
     * overlay 监听 [patchVersion]，在 `withFrameNanos` 里调用本方法，
     * 把队列中所有 pending patch 逐个应用到 timeline。时间戳必须来自 Compose frame clock。
     *
     * Issue #725 评论 5750735497：不再处理 cursor redirect / suppressCursorThroughSequence —
     * 屏幕 caret 始终由 BasicTextField 自己画，applyPatch 不再接受 cursor 参数。
     *
     * @param frameTimeNanos 当前帧时间戳（来自 Compose frame clock）。
     * @return 本次帧实际应用的 patch 列表。
     */
    fun drainPendingPatchesAtFrame(frameTimeNanos: Long): List<ComposeVisualPatch> {
        if (pendingPatches.isEmpty()) {
            return emptyList()
        }
        val batch = mutableListOf<ComposeVisualPatch>()
        for (raw in pendingPatches) {
            val patch = currentMotionPolicy?.let { raw.patch.copy(motionPolicy = it) } ?: raw.patch
            batch.add(patch)
        }
        pendingPatches.clear()
        val framePatch = ComposeVisualPatchBatch.compose(batch) ?: return emptyList()
        visualTimeline.applyPatch(
            patch = framePatch,
            frameTimeNanos = frameTimeNanos,
        )
        // Issue #728 评论 5754045689：构造/重定向 activeEditMotion —
        // 从 timeline 拿当前 inserted/deleted unit keys，用 patch 的 caret rect 构造或重定向 motion。
        val (insertedKeys, deletedKeys) = visualTimeline.activeEditUnitKeys()
        val policy = framePatch.motionPolicy.effective()
        val textDurationNanos = policy.textDurationMillis.coerceAtLeast(0L) * NANOS_PER_MS
        val cursorDurationNanos = policy.cursorDurationMillis.coerceAtLeast(0L) * NANOS_PER_MS
        if (insertedKeys.isEmpty() && deletedKeys.isEmpty()) {
            // selection-only 移动：caret 单独用 cursorDurationMillis，文字 units 为空
            activeEditMotion =
                ComposeEditMotion.forSelectionMove(
                    originCaretRect = framePatch.originCaretRect,
                    targetCaretRect = framePatch.targetCaretRect,
                    frameTimeNanos = frameTimeNanos,
                    durationNanos = if (policy.cursorEnabled) cursorDurationNanos else 0L,
                )
        } else {
            val existing = activeEditMotion
            // 一笔编辑一只钟：text edit 时 caret 和文字共用 textDurationMillis
            val editDurationNanos = if (policy.textEnabled) textDurationNanos else 0L
            activeEditMotion =
                if (existing != null && !existing.isFinished(frameTimeNanos)) {
                    // 快速连续输入：从当前 sample 重定向，不重新起播
                    existing.redirectTo(
                        newOriginCaretRect = framePatch.originCaretRect,
                        newTargetCaretRect = framePatch.targetCaretRect,
                        newInsertedUnitKeys = insertedKeys,
                        newDeletedUnitKeys = deletedKeys,
                        frameTimeNanos = frameTimeNanos,
                        durationNanos = editDurationNanos,
                    )
                } else {
                    ComposeEditMotion.forEdit(
                        originCaretRect = framePatch.originCaretRect,
                        targetCaretRect = framePatch.targetCaretRect,
                        insertedUnitKeys = insertedKeys,
                        deletedUnitKeys = deletedKeys,
                        frameTimeNanos = frameTimeNanos,
                        durationNanos = editDurationNanos,
                    )
                }
        }
        return listOf(framePatch)
    }

    /**
     * #689 评论 5674631257 步骤7：在 Compose 帧时钟的回调里采样当前视觉场景 — overlay 在每帧 draw 前调用。
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
        // Issue #728 评论 5754045689：先 sample activeEditMotion，再把同一份 sample 传给 timeline。
        val motionSample = activeEditMotion?.sample(frameTimeNanos)
        val scene = visualTimeline.sample(frameTimeNanos, motionSample)
        _visualScene.update { scene }
        // #708 评论 5723410606 第一节：同步 draw snapshot 的 scene + caretRect —
        // draw 层下一帧 drawWithContent 直接读，不在 Composable 主体读 visualScene StateFlow。
        // Issue #728：caretRect 由 activeEditMotion 统一产生，写进 draw snapshot 供 draw 层画 caret。
        drawSnapshotState =
            drawSnapshotState.copy(
                scene = scene,
                caretRect = motionSample?.caretRect,
            )
        // motion 完成后清掉，避免持续 sample 已结束的 motion
        if (motionSample != null && motionSample.finished) {
            activeEditMotion = null
        }
        return scene
    }

    /**
     * 是否还有活动动画 — overlay 据此决定是否继续推进帧时钟。
     *
     * @param frameTimeNanos 当前帧时间戳。
     */
    fun hasActiveVisuals(frameTimeNanos: Long): Boolean {
        return visualTimeline.hasActiveAnimation(frameTimeNanos) ||
            (activeEditMotion != null && !activeEditMotion!!.isFinished(frameTimeNanos))
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
        // #691 评论 5679242735 修改2：重置运行时 policy 切换状态
        currentMotionPolicy = null
        // Issue #728：清空统一编辑 motion
        activeEditMotion = null
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
        drawSnapshotState = ComposeEditorDrawSnapshot()
        lastObservedLayoutFingerprint = null
        nextHandoffUnitKey = 2_000_000L
        // #713 评论 5739986801：重置 selection 追踪
        lastResolvedSelection = null
        // Issue #723 评论 5750100004：重置入队序号计数器
        nextPendingSequence = 0L
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
        // 清掉旧 text units / ghost
        visualTimeline.settleForPolicyChange()
        _visualScene.update { ComposeVisualScene.Empty }
        // Issue #728：清掉旧 activeEditMotion
        activeEditMotion = null
        // #708 评论 5723410606 第一节：同步清 draw snapshot 的 scene + caretRect
        drawSnapshotState =
            drawSnapshotState.copy(
                scene = ComposeVisualScene.Empty,
                caretRect = null,
            )
        // 把已入队 patch 的 motionPolicy 替换成最新 policy
        // Issue #723 评论 5750100004：p 现在是 PendingPatch，保持 sequence 不变，只替换 patch 的 motionPolicy。
        if (pendingPatches.isNotEmpty()) {
            val updated = mutableListOf<PendingPatch>()
            while (pendingPatches.isNotEmpty()) {
                val p = pendingPatches.removeFirst()
                updated.add(p.copy(patch = p.patch.copy(motionPolicy = effective)))
            }
            updated.forEach { pendingPatches.addLast(it) }
        }
    }
}
