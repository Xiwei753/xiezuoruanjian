package com.xiwei.sujian.feature.editor.visual

import android.util.Log
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.core.interop.diagnostics.EditorDiagnosticsEvents
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
     * #694 评论第 3 步：上一次真正呈现的 layout。
     */
    private var lastPresentedLayout: ComposeLayoutSnapshot? = null

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
     * 不更新 layout epoch、不调用 frameCoordinator.onLayout、不重新发布相同 TextLayoutResult。
     */
    private var lastObservedLayoutFingerprint: LayoutFingerprint? = null

    /**
     * #708 评论 5723410606 第一节：draw 层在 drawWithContent 里一次性取走 draw 阶段原子快照。
     * 关键：这个 State 只能在 drawWithContent 里读。
     */
    internal fun drawSnapshot(): ComposeEditorDrawSnapshot = drawSnapshotState

    /**
     * #694 评论 5694645209 问题1：根据 bridge 的 [InputSnapshotOutcome] 收口 —
     * 由 [WritingPaneRoute] 的 `SetupInputSnapshotCollector` 在 `snapshotFlow.collect` 中调用，
     * **在 bridge.onInputSnapshot 之后**，用 bridge 的真实决定收口视觉状态。
     *
     * Issue #735 评论 5773604666 问题1：删除双视觉入口后，所有正文编辑统一走 [onEditFact]。
     * 本方法不再生成 local patch，只负责：
     * - 纯 selection 移动检测（text 不变、只有 selection 变了）。
     * - selection 变化诊断事件。
     *
     * @param snapshot 当前 IME 输入快照（text + selection + composition）。
     * @param outcome bridge 对本次 snapshot 的处理结果。
     */
    fun onInputSnapshotResolved(
        snapshot: EditorInputSnapshot,
        outcome: InputSnapshotOutcome,
    ) {
        val compositionActive = snapshot.composition != null
        // Issue #735 评论 5774895427：删除 wasCompositionActive 后，composition 结束/取消/拒绝
        // 不再需要提前推进 coordinator baseline。所有正文 layout 统一走 onAuthoritativeLayout ->
        // frameCoordinator.onLayout，由 onEditFact/onLayout 双向合流配对生成 patch。
        // when 块保留以文档化各 outcome 的处理归属。
        when (outcome) {
            InputSnapshotOutcome.Composing -> {
                // composition 仍活跃：onAuthoritativeLayout 的 compositionActive 分支只缓存 preedit layout，
                // 不碰 coordinator 的 committed baseline。
            }
            InputSnapshotOutcome.LocalCommitAccepted -> {
                // Core 已接受本地 commit，会通过 onEditFact 发送 EditorEditFact 驱动视觉 patch。
                // 后续 onAuthoritativeLayout(compositionActive=false) 走 frameCoordinator.onLayout 配对。
            }
            InputSnapshotOutcome.NoTextChange,
            InputSnapshotOutcome.AuthoritativeApplied,
            InputSnapshotOutcome.LocalCommitRejected,
            -> {
                // composition 取消/无变化/Core 拒绝：后续权威 layout / external intent 正常接管。
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
    }

    /**
    * 系统给出权威布局 — 只记录，不修改输入几何。
    *
    * 得到 patch 后不要启动一笔新事务，只把 patch 暂存/发布给 overlay 的时间线入口。
    *
    * #694 评论第 2/3 步：[compositionActive] 表示当前 IME composition 是否活跃
    * （bridge.state.composition != null）。composition 活跃时只推进布局基线，
    * 不播放 preedit 的吞吐；composition 结束后由 Core 通过 [onEditFact] 驱动视觉 patch。
    *
     * Issue #735 评论 5773604666 问题1：删除双视觉入口后，本方法不再配对 local input 生成 local patch。
     * 所有正文编辑统一走 [onEditFact] → [ComposeVisualFrameCoordinator.onEditFact] → [applyFrameUpdate]。
     * 本方法只负责：
     * - fingerprint 去重（纯 selection/scroll 不触发布局 epoch）。
     * - composition 活跃时只缓存 preedit layout（不调 frameCoordinator，不碰 committed baseline）。
     * - composition 结束后一律把真实 layout 交给 [ComposeVisualFrameCoordinator.onLayout]，
     *   由 onEditFact/onLayout 双向合流配对生成 patch（Issue #735 评论 5774895427）。
     * - 非 composition 的 Core visual path（Undo/Redo/Programmatic/Load/Format）。
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
        // - 不调用 frameCoordinator.onLayout；
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
       // 后续分支（composition active / Core visual path）会
       // 在此基础上把 drawSnapshotState.caretRect 同步给 draw 层。active motion 期间 motion sample
       // 覆盖此值；motion finished 后 sampleVisualScene 用此值无缝接上。
        restingCaretRect = snapshot.cursorRect(snapshot.selection.end)

         // composition 活跃时只缓存 preedit layout，不生成 patch（不播放 preedit 的吞吐）
         if (compositionActive) {
             // #694 评论 5692161955 问题3 / Issue #735 评论 5774895427：
             // composition 活跃分支只更新 lastPresentedLayout，不调 frameCoordinator。
             // 原因：frameCoordinator.lastConsumed 是 committed baseline，
             // 必须只跟 Core 已提交正文，不能推到未提交给 Core 的 preedit。
             // composition 结束后由 onLayout 统一接管，onEditFact/onLayout 双向合流配对。
             lastPresentedLayout = snapshot
             // Issue #728 评论 5754839786 缺口2：composition active 分支同步 restingCaretRect + drawSnapshot caret。
             // composition 期间无 active motion（preedit 不播放吞吐），用 restingCaretRect 填 drawSnapshot。
             drawSnapshotState =
                 drawSnapshotState.copy(
                     layout = snapshot,
                     caretRect = restingCaretRect,
                 )
             return
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
                    // Issue #735 评论 5773604666 问题2：单一 duration —
                    // caret 和 glyph 共用同一只钟；文字继续用 textDurationNanos 让文字吐完。
                    existing.redirectCaretTo(
                        newOriginCaretRect = pendingSelection.originCaretRect,
                        newTargetCaretRect = pendingSelection.targetCaretRect,
                        frameTimeNanos = frameTimeNanos,
                        durationNanos =
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
            // Issue #735 评论 5773604666 问题2：正文 edit motion 一律一只钟 —
            // caret 和 glyph 共用同一个 durationNanos/progress，不再根据 coordinated 分两只钟。
            // - textAnimationEnabledForEdit=true：durationNanos = textDurationNanos
            // - 否则：durationNanos = 0L（瞬时完成）
            val editDurationNanos =
                if (policy.textAnimationEnabledForEdit) textDurationNanos else 0L
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
            val existing = activeEditMotion
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
                        durationNanos = editDurationNanos,
                        inheritedFractionsByKey = inheritedFractionsByKey,
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
        // Issue #735 评论 5773604666 问题2：删除 coordinated 特殊判断 —
        // 正文 edit motion 一律一只钟，timeline 和 activeEditMotion 自然同生共死，
        // 不再需要 coordinated 模式下额外判断 activeEditMotion==null 时 timeline 不应维持动画。
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
        lastPresentedLayout = null
        // #708 评论 5723410606 第一节/第二节/第三节：重置 draw snapshot / fingerprint
        drawSnapshotState = ComposeEditorDrawSnapshot()
        lastObservedLayoutFingerprint = null
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
