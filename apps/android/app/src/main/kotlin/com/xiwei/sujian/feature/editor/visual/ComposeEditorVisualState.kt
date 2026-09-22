package com.xiwei.sujian.feature.editor.visual

import android.util.Log
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.core.interop.diagnostics.EditorDiagnosticsEvents
import com.xiwei.sujian.feature.editor.input.EditorInputSnapshot
import com.xiwei.sujian.feature.editor.input.InputSnapshotOutcome
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.layout.boundsForRawRange
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
 */
class ComposeEditorVisualState(
    private val targetId: String,
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

    /**
     * Issue #728 评论 5754839786 缺口2：静止 caret rect —
     * 无 active motion 时（纯 selection 移动 / 静止 / 动画完成）屏幕 caret 应停留的位置。
     *
     * 背景：#728 把系统 caret 设成透明后，"静止光标"和"纯 selection 移动"没有闭环 —
     * [sampleVisualScene] 在 motion finished 后把 [activeEditMotion] 置 null，
     * [ComposeEditorDrawSnapshot.caretRect] 随之变 null，draw 层不画 caret，屏幕 caret 消失。
     * 本字段收成单一状态：onAuthoritativeLayout / onInputSnapshotResolved / publishLocalHandoffScene
     * 把"当前应停留的 caret 几何"写进 restingCaretRect；sampleVisualScene 在无 active motion 时
     * 用 restingCaretRect 填 drawSnapshotState.caretRect，保证静止 caret 一直可见。
     * active motion 期间 restingCaretRect 保存 motion 的 target，motion finished 后无缝接上。
     */
    private var restingCaretRect: Rect? = null

    /**
     * Issue #728 评论 5755336403 缺口2：pending 纯 selection caret 移动 —
     * onInputSnapshotResolved 没有 frameTimeNanos，不能直接创建 forSelectionMove motion
     * （用 0L 会让 motion 立即到 target）。先记录 old/new caret target，
     * 到 drainPendingPatchesAtFrame 的真实 frameTime 再创建/重定向 forSelectionMove。
     */
    private data class PendingSelectionCaretMove(
        val originCaretRect: Rect,
        val targetCaretRect: Rect,
    )

    private var pendingSelectionCaretTarget: PendingSelectionCaretMove? = null

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

    private val _frameRequestVersion = MutableStateFlow(0L)
    val frameRequestVersion: StateFlow<Long> = _frameRequestVersion.asStateFlow()

    /**
     * 最新生成的 patch — 仅保留给日志/调试使用，timeline 输入不再依赖它。
     */
    private val _latestPatch = MutableStateFlow<ComposeVisualPatch?>(null)
    val latestPatch: StateFlow<ComposeVisualPatch?> = _latestPatch.asStateFlow()

    /**
     * Issue #732 评论 5763493968 第3节：当前 effective policy — 非 nullable，初始值 [EditorMotionPolicy]。
     *
     * [drainPendingPatchesAtFrame] 开头先应用 [pendingMotionPolicy]（如果有），
     * 然后再 drain patch、更新 timeline、创建/redirect [ComposeEditMotion]。
     * policy 切换、patch 消费、motion 创建全部进同一只 frame clock。
     */
    private var currentMotionPolicy: EditorMotionPolicy = EditorMotionPolicy()

    /**
     * Issue #732 评论 5763493968 第3节：pending policy — [updateMotionPolicy] 只写此字段，
     * [drainPendingPatchesAtFrame] 开头应用（currentMotionPolicy = pendingMotionPolicy; pendingMotionPolicy = null）。
     */
    private var pendingMotionPolicy: EditorMotionPolicy? = null

    /**
     * Issue #735 评论 5771063665：编辑事实到达 — 只把 fact 交给 frameCoordinator，不启动动画、不改 layout。
     *
     * 取代已删除的 onVisualIntent — Core 已不再返回视觉意图，
     * Android 从 [EditorEditFact] 的 cause/operationKind/offsetMap 推导动画策略。
     *
     * @param fact 编辑事实（从 Core EditorEditResult 映射）。
     */
    fun onEditFact(fact: EditorEditFact) {
        val update = frameCoordinator.onEditFact(fact)
        applyFrameUpdate(update)
    }

    /**
     * #713 评论 5739986801：上一次 resolved 的 selection —
     * 用于检测纯 selection 变化（text 不变、composition 为空、selection 变了）。
     */
    private var lastResolvedSelection: TextRange? = null

    /**
     * Issue #728 评论 5755336403 缺口1：上一次 resolved 的 text —
     * 用于严格判断纯 selection 移动（text 未变、只有 selection 变了）。
     * 不能用 _latestLayout.text == snapshot.text 判断，因为 onTextLayout 和 snapshotFlow
     * 是两条独立流，layout 可能先到，正常打字时也会误判为纯 selection。
     */
    private var lastResolvedText: String? = null

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
        // Issue #728 评论 5755336403 缺口1+缺口2：纯 selection 移动闭环 —
        // 严格判断：text 与上次 resolved 完全相同（不是与 layout text 比较）、selection 变了、
        // composition 不活跃。正常 text edit 的 selection 变化不进入此分支，不清 activeEditMotion。
        // composition 的 selection 变化也不混进来。
        val layoutForSelection = _latestLayout.value
        val isPureSelectionMove =
            !compositionActive &&
                snapshot.text == lastResolvedText &&
                snapshot.selection != lastResolvedSelection &&
                layoutForSelection != null &&
                layoutForSelection.result.layoutInput.text.text == snapshot.text
        if (isPureSelectionMove) {
            // 缺口2：记录 pending selection caret target，触发帧循环在真实 frameTime 创建 forSelectionMove。
            // cursor 动画关闭时直接跳到 target（写 restingCaretRect + drawSnapshotState.caretRect）。
            // isPureSelectionMove 已保证 layoutForSelection != null，这里断言一次拿到非空引用。
            val layout = layoutForSelection!!
            val targetCaret = layout.cursorRect(snapshot.selection.end)
            val originCaret =
                restingCaretRect
                    ?: layout.cursorRect(lastResolvedSelection?.end ?: snapshot.selection.start)
            val policy = currentMotionPolicy.effective()
            if (policy.cursorAnimationEnabledForEdit && policy.selectionCursorDurationMillis > 0L) {
                // 有平滑光标：记录 pending target，等真实 frameTime 创建 motion
                pendingSelectionCaretTarget =
                    PendingSelectionCaretMove(
                        originCaretRect = originCaret,
                        targetCaretRect = targetCaret,
                    )
                _frameRequestVersion.update { it + 1L }
            } else {
                // cursor 动画关闭：直接跳到 target
                restingCaretRect = targetCaret
                drawSnapshotState = drawSnapshotState.copy(caretRect = targetCaret)
                activeEditMotion = null
            }
        }
        lastResolvedSelection = snapshot.selection
        lastResolvedText = snapshot.text
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

            // Issue #728 评论 5754045689：系统 caret 已透明，可见 caret 由 EditorTextFieldDrawLayer 画 —
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
                // Issue #732 评论 5763493968 第4节：coordinatedSpatialClip 不再从旧 patch policy
                // 保存成 timeline 状态，而是由当前 effective policy + 当前 motion sample 导出。
                // handoff scene 用当前 currentMotionPolicy 导出（drain 时会用最新 policy 覆盖）。
                coordinatedSpatialClip =
                    currentMotionPolicy.effective().let { it.textAnimationEnabledForEdit && it.coordinated },
            )
        }
        // 同步把首帧 scene 写进 draw snapshot — draw 层下一帧 drawWithContent 直接读
        // Issue #728 评论 5755336403 缺口3：handoff 不提前把 caret 推到 target —
        // handoff 只换文字 scene，caret 保持当前屏幕真实位置：
        // - 无旧 motion 时用 patch.originCaretRect（编辑前位置）
        // - 有旧 motion 时保持上一帧 draw caret（motion 中间 sample）
        // 到真实 frameTime 时 drainPendingPatchesAtFrame 创建/重定向 motion，一次把 caret 和文字推进到同一帧。
        // restingCaretRect 不在 motion 开始前先写 target；只在 motion 完成/瞬时完成后落到 target。
        val handoffCaretRect = activeEditMotion?.let { drawSnapshotState.caretRect } ?: patch.originCaretRect
        drawSnapshotState =
            drawSnapshotState.copy(
                scene = _visualScene.value,
                layout = newLayout,
                caretRect = handoffCaretRect,
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
                    // 不再只做 addLast + frameRequestVersion + bindLocalPatchHandoff（旧 bindLocalPatchHandoff
                    // 因 pendingLocalEditHandoff 从未被建立只走 Log.w，首帧 scene 从未发布）。
                    // 现在统一调 publishLocalHandoffScene，让中文 composition commit 后 local timeline
                    // 立即接管，不出现"最终字先裸画一帧 -> 动画再接手"的窗口。
                    publishLocalHandoffScene(
                        patch = localPatch,
                        oldLayout = baseLayout,
                        newLayout = finalLayout,
                    )
                    pendingPatches.addLast(PendingPatch(sequence = nextPendingSequence++, patch = localPatch))
                    _frameRequestVersion.update { it + 1L }
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

        // Issue #732 评论 5763493968 第2节：buildLocalInputPatch 不再读 currentMotionPolicy —
        // 本地输入和 Core/external 输入都生成同一种事实 patch（inserted/deleted units 总是合成），
        // 是否播放由 [drainPendingPatchesAtFrame] 的当前 effective policy 决定。
        // #694 评论 5691696678 问题3：insertedUnits/deletedUnits 用通用 stage-map 版本合成，
        // 保留多字符吐字顺序（a/b/c 三个 unit 而非单个 [0,3)）。
        // 每笔 edit 的 insertedUnits/deletedUnits 从该笔 stage offset map 补集算，
        // 然后沿后续 stage offset map 映射到最终 Tn / 最初 T0。
        val composedInserted = ComposeLocalVisualRebase.composeLocalChainInsertedUnits(chain)
        val composedDeleted = ComposeLocalVisualRebase.composeLocalChainDeletedUnits(chain)

        // Issue #735 评论 5771063665：调用 Android 自己的纯计算分类器 —
        // 不再通过 FFI 问 Core，用 BreakIterator 做 grapheme cluster 拆分。
        val corePlan =
            ComposeLocalVisualRebase.classifyLocalVisualPlan(
                oldText = oldText,
                newText = newText,
                oldAffectedRanges = changedRanges.oldRanges,
                newAffectedRanges = changedRanges.newRanges,
                animationEnabled = true,
            )
        val planAnimationMode = corePlan.animationMode
        val planInsertedUnits = corePlan.newAnimationUnits
        val planDeletedUnits = corePlan.oldAnimationUnits

        val insertedUnits =
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

        val deletedUnits =
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
        // Issue #728 评论 5754839786 缺口1：本地 patch 也写真实 caret 两端 —
        // 从 oldLayout + firstEdit.oldSelection.end 算 origin caret，
        // 从 newLayout + lastEdit.newSelection.end 算 target caret。
        // 旧实现漏传，ComposeVisualPatch 的 Rect.Zero 默认值让本地 patch 的 caret 两端变成 (0,0,0,0)，
        // ComposeEditMotion 从原点插值到原点，本地编辑时屏幕 caret 不动。
        val originCaretRect = oldLayout.cursorRect(firstEdit.oldSelection.end)
        val targetCaretRect = newLayout.cursorRect(lastEdit.newSelection.end)
        return ComposeVisualPatch(
            id = nextLocalPatchId,
            coreTransactionIds = emptyList(),
            oldLayout = oldLayout,
            newLayout = newLayout,
            offsetMap = offsetMap,
            insertedUnits = insertedUnits,
            deletedUnits = deletedUnits,
            retainedMoves = retainedMoves,
            originCaretRect = originCaretRect,
            targetCaretRect = targetCaretRect,
            // 本地输入时长由 motionPolicy 决定（timeline 用 policy.textDurationMillis）
            durationMs = 0L,
            // Issue #735 评论 5771063665：使用 Android 自己推导的 animationMode。
            animationMode = planAnimationMode,
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
            // Issue #728 评论 5754839786 缺口2：fingerprintUnchanged 时无 active motion（纯 selection），
            // 直接把 restingCaretRect 更新到当前 selection.end 对应的 caret rect，
            // 并写进 drawSnapshotState.caretRect，保证静止/selection 移动后屏幕 caret 停在新位置。
            restingCaretRect = snapshot.cursorRect(snapshot.selection.end)
            drawSnapshotState =
                drawSnapshotState.copy(
                    layout = snapshot,
                    caretRect = restingCaretRect,
                )
            return
        }
        lastObservedLayoutFingerprint = fingerprint

        // 真实 text/line geometry 变化：把新 layout 写进 _latestLayout 和 draw snapshot
        _latestLayout.update { snapshot }
        // Issue #728 评论 5754839786 缺口2：真实 layout 变化后更新 restingCaretRect —
        // 后续分支（composition 缓存 / composition active / Core visual path / local patch）会
        // 在此基础上把 drawSnapshotState.caretRect 同步给 draw 层。active motion 期间 motion sample
        // 覆盖此值；motion finished 后 sampleVisualScene 用此值无缝接上。
        restingCaretRect = snapshot.cursorRect(snapshot.selection.end)

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
            // Issue #728 评论 5754839786 缺口2：composition 缓存分支也同步 restingCaretRect + drawSnapshot caret。
            // 此分支无 active motion（composition 还没收口），直接用 restingCaretRect 填 drawSnapshot。
            drawSnapshotState =
                drawSnapshotState.copy(
                    layout = snapshot,
                    caretRect = restingCaretRect,
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
                    _frameRequestVersion.update { it + 1L }
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
            // Issue #728 评论 5754839786 缺口2：composition active 分支同步 restingCaretRect + drawSnapshot caret。
            // composition 期间无 active motion（preedit 不播放吞吐），用 restingCaretRect 填 drawSnapshot。
            drawSnapshotState =
                drawSnapshotState.copy(
                    layout = snapshot,
                    caretRect = restingCaretRect,
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
        // Issue #728 评论 5754839786 缺口2：Core visual path 无 active motion 时用 restingCaretRect 填 caret；
        // 有 active motion 时保留 motion sample 给的 caretRect（由 sampleVisualScene 每帧覆盖）。
        drawSnapshotState =
            drawSnapshotState.copy(
                scene = _visualScene.value,
                layout = snapshot,
                caretRect = activeEditMotion?.let { drawSnapshotState.caretRect } ?: restingCaretRect,
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
                _frameRequestVersion.update { it + 1L }
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
     * overlay 监听 [frameRequestVersion]，在 `withFrameNanos` 里调用本方法，
     * 把队列中所有 pending patch 逐个应用到 timeline。时间戳必须来自 Compose frame clock。
     *
     * #691：同时把光标 motion 并入 timeline — 与文字在同一个 applyPatch 内处理，
     * 保证 cursor 和 text units 使用同一个 frameTimeNanos。
     *
     * Issue #732 评论 5763493968 第3节：开头先应用 [pendingMotionPolicy]（如果有），
     * 然后再 drain patch、更新 timeline、创建/redirect [ComposeEditMotion]。
     * policy 切换、patch 消费、motion 创建全部进同一只 frame clock。
     */
    fun hasPendingPatches(): Boolean = pendingPatches.isNotEmpty() || pendingSelectionCaretTarget != null

    /**
     * 在 Compose 帧时钟的回调里消费所有待处理的 patch 并应用到 timeline。
     *
     * overlay 监听 [frameRequestVersion]，在 `withFrameNanos` 里调用本方法，
     * 把队列中所有 pending patch 逐个应用到 timeline。时间戳必须来自 Compose frame clock。
     *
     * Issue #728 评论 5754045689：系统 caret 已透明，可见 caret 由 [EditorTextFieldDrawLayer] 画，
     * applyPatch 不再接受 cursor 参数。
     *
     * @param frameTimeNanos 当前帧时间戳（来自 Compose frame clock）。
     * @return 本次帧实际应用的 patch 列表。
     */
    fun drainPendingPatchesAtFrame(frameTimeNanos: Long): List<ComposeVisualPatch> {
        // Issue #732 评论 5763493968 第3节：开头先应用 pending policy —
        // policy 切换、patch 消费、motion 创建全部进同一只 frame clock。
        val pendingPolicy = pendingMotionPolicy
        if (pendingPolicy != null) {
            pendingMotionPolicy = null
            val previousPolicy = currentMotionPolicy
            currentMotionPolicy = pendingPolicy.effective()
            // policy 真正变化时 settle timeline — 清掉旧 text units / ghost / motion，
            // 让后续 drain 用新 policy 重新决定是否创建 track。
            if (previousPolicy != currentMotionPolicy) {
                visualTimeline.settleForPolicyChange()
                _visualScene.update { ComposeVisualScene.Empty }
                activeEditMotion = null
                // 保留 restingCaretRect / drawSnapshotState.caretRect —
                // policy 切换不必然产生新 text layout，静止 caret 应保留当前屏幕位置。
                drawSnapshotState =
                    drawSnapshotState.copy(
                        scene = ComposeVisualScene.Empty,
                        // caretRect 保持当前值（restingCaretRect），不清空
                    )
            }
        }

        // Issue #728 评论 5755336403 缺口2：先处理 pending 纯 selection caret 移动 —
        // 在真实 frameTime 创建/重定向 forSelectionMove motion。
        val pendingSelection = pendingSelectionCaretTarget
        if (pendingSelection != null) {
            pendingSelectionCaretTarget = null
            val policy = currentMotionPolicy.effective()
            val selectionCursorDurationNanos =
                policy.selectionCursorDurationMillis.coerceAtLeast(0L) * NANOS_PER_MS
            val textDurationNanos = policy.textDurationMillis.coerceAtLeast(0L) * NANOS_PER_MS
            val existing = activeEditMotion
            val caretDuration =
                if (policy.cursorAnimationEnabledForEdit) selectionCursorDurationNanos else 0L
            activeEditMotion =
                if (existing != null && !existing.isFinished(frameTimeNanos)) {
                    // Issue #728 评论 5755928697 问题1：文字动画还没结束时移动光标，
                    // 用 redirectCaretTo 保留现有 glyph channel，不丢正在吐的字。
                    // caret 用 cursorDurationNanos；glyph 继续用 textDurationNanos 让文字吐完。
                    existing.redirectCaretTo(
                        newOriginCaretRect = pendingSelection.originCaretRect,
                        newTargetCaretRect = pendingSelection.targetCaretRect,
                        frameTimeNanos = frameTimeNanos,
                        caretDurationNanos = caretDuration,
                        glyphDurationNanos =
                            if (policy.textAnimationEnabledForEdit) textDurationNanos else 0L,
                    )
                } else {
                    ComposeEditMotion.forSelectionMove(
                        originCaretRect = pendingSelection.originCaretRect,
                        targetCaretRect = pendingSelection.targetCaretRect,
                        frameTimeNanos = frameTimeNanos,
                        caretDurationNanos = caretDuration,
                    )
                }
            // restingCaretRect 落到 target（motion 完成后无缝接上）
            restingCaretRect = pendingSelection.targetCaretRect
        }
        if (pendingPatches.isEmpty()) {
            return emptyList()
        }
        val batch = mutableListOf<ComposeVisualPatch>()
        for (raw in pendingPatches) {
            // Issue #732 评论 5763493968 第2节：patch 不再有 motionPolicy 字段 —
            // 直接用 raw.patch，是否播放由当前 currentMotionPolicy 在 applyPatch/motion 创建时决定。
            batch.add(raw.patch)
        }
        pendingPatches.clear()
        val framePatch = ComposeVisualPatchBatch.compose(batch) ?: return emptyList()
        // Issue #728 评论 5761525795：applyPatch 之前先 sample 当前 activeEditMotion —
        // timeline 在 split/rekey 时需要 parent 的当前 motion fraction 投影到 child 局部区间，
        // 得到 child 首帧应继承的 fraction。不传 motionSample 时 split child 无继承信息，
        // redirectTo 会把 child 当成全新 unit 从 0/1 重启（闪烁/重影）。
        val motionSampleForPatch = activeEditMotion?.sample(frameTimeNanos)
        visualTimeline.applyPatch(
            patch = framePatch,
            frameTimeNanos = frameTimeNanos,
            motionPolicy = currentMotionPolicy,
            motionSample = motionSampleForPatch,
        )
        // Issue #728 评论 5754045689：构造/重定向 activeEditMotion —
        // 从 timeline 拿当前 inserted/deleted unit descriptors，用 patch 的 caret rect 构造或重定向 motion。
        // Issue #728 评论 5756468643 问题2：descriptor 已按正文 range 排序（inserted 按 targetRange.start，
        // deleted 按 range.start），提取 key 时保持这个顺序传给 allocateEditRanges，
        // 让 glyph schedule 顺序和光标经过顺序一致，而不是按 unit.key 编号排序。
        val (insertedDescriptors, deletedDescriptors) = visualTimeline.activeEditUnits()
        // 保持 descriptor 列表的顺序（按正文位置排序），不转 Set 避免丢失顺序
        val insertedKeys = insertedDescriptors.map { it.key }
        val deletedKeys = deletedDescriptors.map { it.key }
        // Issue #732 评论 5763493968 第2/3节：policy 不再从 patch 读 —
        // 用当前 currentMotionPolicy（drain 开头已应用 pending policy）。
        val policy = currentMotionPolicy.effective()
        val textDurationNanos = policy.textDurationMillis.coerceAtLeast(0L) * NANOS_PER_MS
        val cursorDurationNanos = policy.cursorDurationMillis.coerceAtLeast(0L) * NANOS_PER_MS
        // Issue #728 评论 5755336403 缺口4：selection-only 必须用 oldText == newText 判断 —
        // Enter / 删除 Enter 是正文编辑，只是换行符没有 glyph，允许没有 inserted/deleted visual unit。
        // 不能用 "unit key 为空" 推断 "没有文本编辑"。
        val oldText = framePatch.oldLayout.result.layoutInput.text.text
        val newText = framePatch.newLayout.result.layoutInput.text.text
        val isSelectionOnly = oldText == newText && insertedKeys.isEmpty() && deletedKeys.isEmpty()
        if (isSelectionOnly) {
            // selection-only 移动：caret 单独用 selectionCursorDurationMillis，文字 units 为空
            // Issue #732 评论 5764716281 硬问题1：用派生值 cursorAnimationEnabledForEdit 和
            // selectionCursorDurationMillis，coordinated 模式下不被旧 cursorEnabled=false 卡死。
            val selectionCursorDurationNanos =
                policy.selectionCursorDurationMillis.coerceAtLeast(0L) * NANOS_PER_MS
            activeEditMotion =
                ComposeEditMotion.forSelectionMove(
                    originCaretRect = framePatch.originCaretRect,
                    targetCaretRect = framePatch.targetCaretRect,
                    frameTimeNanos = frameTimeNanos,
                    caretDurationNanos =
                        if (policy.cursorAnimationEnabledForEdit) selectionCursorDurationNanos else 0L,
                )
        } else {
            // text edit（包括 Enter / 删除 Enter 无 glyph unit 的情况）—
            // caret old→new 按 edit policy 运行，文字按 glyph policy 运行。
            // insertedKeys/deletedKeys 可能为空（Enter 无 glyph unit），创建无 unit channel 的 edit motion。
            val existing = activeEditMotion
            // Issue #728 评论 5755928697 问题2：coordinated=false 时 caret 和 glyph 独立时长。
            // coordinated=true：一只钟，caret 和 glyph 共用 textDurationMillis。
            // coordinated=false：caret 用 cursorDurationMillis，glyph 用 textDurationMillis。
            // Issue #732 评论 5764716281 硬问题1：用派生值，coordinated 模式下不被隐藏开关关掉。
            val caretDurationNanos: Long
            val glyphDurationNanos: Long
            if (policy.coordinated) {
                val sharedDuration =
                    if (policy.textAnimationEnabledForEdit) textDurationNanos else 0L
                caretDurationNanos = sharedDuration
                glyphDurationNanos = sharedDuration
            } else {
                caretDurationNanos =
                    if (policy.cursorAnimationEnabledForEdit) cursorDurationNanos else 0L
                glyphDurationNanos =
                    if (policy.textAnimationEnabledForEdit) textDurationNanos else 0L
            }
            // 快速连续输入：从当前 sample 重定向，不重新起播
            // Issue #728 评论 5761525795：把 descriptor 的继承 fraction 传给 redirectTo —
            // split/rekey child 从 parent 投影后的 fraction 继续，不从 0/1 重启。
            val inheritedFractionsByKey =
                (insertedDescriptors.asSequence() + deletedDescriptors.asSequence())
                    .mapNotNull { d ->
                        val f = d.inheritedFraction
                        if (f != null) d.key to f else null
                    }
                    .toMap()
            // Issue #728 评论 5762435453：motion finished 不等于 timeline 已收口 —
            // 只要 timeline 本帧从旧 parent split/rekey 出 child（inheritedFractionsByKey 非空），
            // 就必须继续走 redirectTo 保留 lineage，不能因 motion.finished 走 forEdit 丢弃继承 fraction。
            // 否则 surviving inserted child 会从 0 重吐、deleted child 会从 1 重吞。
            val shouldRedirect =
                existing != null &&
                    (!existing.isFinished(frameTimeNanos) || inheritedFractionsByKey.isNotEmpty())
            activeEditMotion =
                if (shouldRedirect) {
                    existing.redirectTo(
                        newOriginCaretRect = framePatch.originCaretRect,
                        newTargetCaretRect = framePatch.targetCaretRect,
                        newInsertedUnitKeys = insertedKeys,
                        newDeletedUnitKeys = deletedKeys,
                        frameTimeNanos = frameTimeNanos,
                        caretDurationNanos = caretDurationNanos,
                        glyphDurationNanos = glyphDurationNanos,
                        inheritedFractionsByKey = inheritedFractionsByKey,
                    )
                } else {
                    ComposeEditMotion.forEdit(
                        originCaretRect = framePatch.originCaretRect,
                        targetCaretRect = framePatch.targetCaretRect,
                        insertedUnitKeys = insertedKeys,
                        deletedUnitKeys = deletedKeys,
                        frameTimeNanos = frameTimeNanos,
                        caretDurationNanos = caretDurationNanos,
                        glyphDurationNanos = glyphDurationNanos,
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
        // Issue #728 评论 5754839786 缺口2：caretRect 由 activeEditMotion 统一产生；
        // 无 active motion 时用 restingCaretRect 填，保证静止/selection 移动后屏幕 caret 不消失。
        drawSnapshotState =
            drawSnapshotState.copy(
                scene = scene,
                caretRect = motionSample?.caretRect ?: restingCaretRect,
            )
        // motion 完成后清掉，避免持续 sample 已结束的 motion
        if (motionSample != null && motionSample.finished) {
            // Issue #728 评论 5754839786 缺口2：motion finished 后把 target caret 落到 restingCaretRect，
            // 再清 activeEditMotion — 下一帧 sampleVisualScene 用 restingCaretRect 填 drawSnapshot，
            // 屏幕 caret 停在 motion 终点，不跳回原点也不消失。
            restingCaretRect = motionSample.caretRect
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
        // Issue #732 评论 5764716281 硬问题2：coordinated 模式下 timeline 必须跟 activeEditMotion
        // 同生共死 — 如果 activeEditMotion==null，timeline 不应继续维持动画（文字应已静态收口）。
        val policy = currentMotionPolicy.effective()
        if (policy.coordinated && policy.textAnimationEnabledForEdit && activeEditMotion == null) {
            return false
        }
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
        _frameRequestVersion.update { 0L }
        _latestLayout.update { null }
        _visualScene.update { ComposeVisualScene.Empty }
        _latestPatch.update { null }
        // Issue #732 评论 5763493968 第3节：重置运行时 policy 切换状态
        currentMotionPolicy = EditorMotionPolicy()
        pendingMotionPolicy = null
        // Issue #728：清空统一编辑 motion
        activeEditMotion = null
        // Issue #728 评论 5754839786 缺口2：清空静止 caret rect
        restingCaretRect = null
        // Issue #728 评论 5755336403 缺口2：清空 pending 纯 selection caret 移动
        pendingSelectionCaretTarget = null
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
        // Issue #728 评论 5755336403 缺口1：重置 resolved text 追踪
        lastResolvedText = null
        // Issue #723 评论 5750100004：重置入队序号计数器
        nextPendingSequence = 0L
    }

    /**
     * Issue #732 评论 5763493968 第3节：运行时 policy 切换 — 只写 [pendingMotionPolicy] + [frameRequestVersion]。
     *
     * 场景：patch 已入队但还没 drain，此时用户关闭文字动画或打开 reduce-motion；
     * 旧 patch 会带着原来的 insertedUnits/deletedUnits/retainedMoves 再进入 timeline，
     * 把文字动画重新启动。本方法只把 effective policy 写进 [pendingMotionPolicy]，
     * 并增加一次 [frameRequestVersion] 唤醒帧循环；
     * [drainPendingPatchesAtFrame] 开头会应用 pending policy（settle timeline + 清 motion），
     * 然后再 drain patch、创建/redirect [ComposeEditMotion]。
     *
     * 不在 UI effect 里立刻 settle timeline — policy 切换、patch 消费、motion 创建
     * 全部进同一只 frame clock（[EditorTextFieldDrawLayer] 的 withFrameNanos）。
     *
     * @param policy 新的动画策略 — 内部会先 effective() 收口 reduce-motion。
     */
    fun updateMotionPolicy(policy: EditorMotionPolicy) {
        pendingMotionPolicy = policy.effective()
        _frameRequestVersion.update { it + 1L }
    }
}
