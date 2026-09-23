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
 * Issue #737：Compose 显示层视觉状态 — 重写编排逻辑。
 *
 * 核心原则：**一笔编辑只有一个 motion**。caret 和吞字/吐字共用同一个 [CaretTraversal]、
 * 同一个 progress、同一个生命周期。要有都有，要没有都没有。
 *
 * 删除（旧架构）：
 * - `visualTimeline: ComposeVisualTimeline` — 持续视觉时间线
 * - `activeEditMotion: ComposeEditMotion?` — 旧统一编辑 motion
 * - `_visualScene` / `visualScene` StateFlow — 不再需要 [ComposeVisualScene]
 * - drainPendingPatchesAtFrame 中"先 timeline.applyPatch 接管文字，再创建 motion"两阶段
 *
 * 改成（新架构）：
 * - [activeMotion]: [CoordinatedEditMotion]? — 唯一 motion
 * - [drainPendingPatchesAtFrame]：从 patch 直接构造 [CoordinatedEditMotion.fromPatch] —
 *   一次性构造，不再先接管文字再创建 motion
 * - [sampleVisualScene]：`motionSample = activeMotion?.sample(frameTimeNanos)` →
 *   更新 [drawSnapshotState]（motionSample + restingCaretRect）
 *
 * 保留不变的公共方法签名（被 [EditorTextFieldDrawLayer] / [WritingPaneRoute] 调用）：
 * [onEditFact]、[onAuthoritativeLayout]、[onInputSnapshotResolved]、
 * [drainPendingPatchesAtFrame]、[sampleVisualScene]、[hasPendingPatches]、
 * [hasActiveVisuals]、[clear]、[updateMotionPolicy]、[drawSnapshot]、
 * [frameRequestVersion]、[latestLayout]、[latestPatch]。
 *
 * 时间戳来自 Compose frame clock（由 overlay 调用 [drainPendingPatchesAtFrame] /
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

    /**
     * Issue #737：唯一协调 motion — 一笔编辑只创建一个 motion，
     * 同一只钟驱动 caret 移动和文字吞吐。
     * null 表示无 active motion（首帧/动画完成/policy 切换后）。
     */
    private var activeMotion: CoordinatedEditMotion? = null

    /**
     * Issue #728 评论 5754839786 缺口2：静止 caret rect —
     * 无 active motion 时（纯 selection 移动 / 静止 / 动画完成）屏幕 caret 应停留的位置。
     *
     * 背景：#728 把系统 caret 设成透明后，"静止光标"和"纯 selection 移动"没有闭环 —
     * [sampleVisualScene] 在 motion finished 后把 [activeMotion] 置 null，
     * [ComposeEditorDrawSnapshot.restingCaretRect] 随之变 null，draw 层不画 caret，屏幕 caret 消失。
     * 本字段收成单一状态：onAuthoritativeLayout / onInputSnapshotResolved
     * 把"当前应停留的 caret 几何"写进 restingCaretRect；sampleVisualScene 在无 active motion 时
     * 用 restingCaretRect 填 drawSnapshotState.restingCaretRect，保证静止 caret 一直可见。
     * active motion 期间 restingCaretRect 保存 motion 的 target，motion finished 后无缝接上。
     */
    private var restingCaretRect: Rect? = null

    /**
     * Issue #728 评论 5755336403 缺口2：pending 纯 selection caret 移动 —
     * onInputSnapshotResolved 没有 frameTimeNanos，不能直接创建 forSelectionMove motion
     * （用 0L 会让 motion 立即到 target）。先记录 old/new caret target，
     * 到 drainPendingPatchesAtFrame 的真实 frameTime 再创建 forSelectionMove。
     *
     * Issue #737 评论 5782106370：同时保存 caret rect 和 offset，保证 caret rect / offset / line
     * 同源。forSelectionMove 不再从 layout.selection 读 offset（oldLayout/newLayout 可能是同一个
     * snapshot，从 selection 读会让 old/new offset 相同，CaretTraversal 误判同行）。
     */
    private data class PendingSelectionCaretMove(
        val originCaretRect: Rect,
        val targetCaretRect: Rect,
        val originCaretOffset: Int,
        val targetCaretOffset: Int,
    )

    private var pendingSelectionCaretTarget: PendingSelectionCaretMove? = null

    /** 最新 layout 快照 — 供 overlay 读取 bounding box。 */
    private val _latestLayout = MutableStateFlow<ComposeLayoutSnapshot?>(null)
    val latestLayout: StateFlow<ComposeLayoutSnapshot?> = _latestLayout.asStateFlow()

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
     * 最新生成的 patch — 仅保留给日志/调试使用。
     */
    private val _latestPatch = MutableStateFlow<ComposeVisualPatch?>(null)
    val latestPatch: StateFlow<ComposeVisualPatch?> = _latestPatch.asStateFlow()

    /**
     * Issue #732 评论 5763493968 第3节：当前 effective policy — 非 nullable，初始值 [EditorMotionPolicy]。
     *
     * [drainPendingPatchesAtFrame] 开头先应用 [pendingMotionPolicy]（如果有），
     * 然后再 drain patch、创建 [CoordinatedEditMotion]。
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
     * Issue #737 评论 5782769758：presentation generation 计数器 —
     * 每次 [applyFrameUpdate] 原子切换 draw snapshot 时递增。
     * layout 和 motionSample 带同一 generation，防止 old sample + new layout 混搭配
     * （buildHiddenPath 拿旧 sample 的 hiddenRanges 裁新 layout → 裁错字或闪一下）。
     */
    private var presentationGeneration: Long = 0L

    /**
     * Issue #737 评论 5784705864 缺口1：layout 先到、fact 后到时的 pending presentation ownership。
     *
     * 当 onAuthoritativeLayout 收到新 text/几何但 fact 还没配对（FrameUpdate.Empty）时，
     * BasicTextField 已是新正文，drawContent() 会裸画最终态。旧实现只"不更新 drawSnapshotState.layout"
     * 想用旧 layout 当屏障，但 drawContent() 画的是 BasicTextField 本身，不是 drawSnapshotState.layout，
     * 屏障无效。
     *
     * pending presentation 用"上一份已呈现 layout + 当前新 layout"立即建立屏幕 ownership：
     * - new layout 新增/替换的 range 从 BasicTextField drawContent 裁掉（hiddenRanges）
     * - old layout 被删/替换的 range 用 old layout 静态画 ghost（glyphOverlays，Deleted，clipFraction=1）
     * - caret 停在旧 presentation 的 origin
     * fact 到达后由 [applyPreparedMotionFromPatch] 原子升级成 prepared motion（视觉一致，无缝）。
     */
    private data class PendingPresentation(
        val oldLayout: ComposeLayoutSnapshot,
        val newLayout: ComposeLayoutSnapshot,
        val sample: CoordinatedEditMotion.Sample,
    )

    private var pendingPresentation: PendingPresentation? = null

    /** pending presentation ghost key 计数器（负值，与 CoordinatedEditMotion.nextGlyphKey 正值解耦）。 */
    private var nextPendingGlyphKey: Long = -1L

    /**
     * Issue #737 评论 5784705864 缺口2：当前 prepared motion（[activeMotion].isPrepared）覆盖的
     * pending queue 最大 sequence。drain 时用它判断 prepared motion 是否就是整个 pending queue
     * 的合成结果 — 是则直接 start，否则重新 compose。
     * -1L 表示无 prepared motion 或已 start。
     */
    private var preparedMotionSequence: Long = -1L

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
                // composition 仍活跃：onAuthoritativeLayout 的 compositionActive 分支把 preedit layout
                // 当候选几何交给 frameCoordinator.onProvisionalLayout（只缓存 latest，不碰 committed baseline）。
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
        // composition 不活跃。正常 text edit 的 selection 变化不进入此分支，不清 activeMotion。
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
            // cursor 动画关闭时直接跳到 target（写 restingCaretRect + drawSnapshotState.restingCaretRect）。
            // isPureSelectionMove 已保证 layoutForSelection != null，这里断言一次拿到非空引用。
            val layout = layoutForSelection!!
            // Issue #737 评论 5782447373：纯 selection move 的 origin/target offset 和 rect 必须在
            // 同一份平台 layout 上一起生成，不再用 restingCaretRect 代替 originRect。
            // restingCaretRect 可能在 onAuthoritativeLayout 的 fingerprintUnchanged 分支里被更新成
            // 当前新 selection 的 caret rect（target），用它当 originRect 会让 originRect == targetRect，
            // CaretTraversal 直接判无效（oldCaretRect == newCaretRect），光标动画消失。
            // 上一笔 motion 还没结束时，lastResolvedSelection.end 是上一笔的逻辑 target，
            // 从它开新 motion 等同于"明确结束上一笔后从其 target 开新 motion"。
            val originCaretOffset = lastResolvedSelection?.end ?: snapshot.selection.start
            val targetCaretOffset = snapshot.selection.end
            val originCaret = layout.cursorRect(originCaretOffset)
            val targetCaret = layout.cursorRect(targetCaretOffset)
            val policy = currentMotionPolicy.effective()
            if (policy.cursorAnimationEnabledForEdit && policy.selectionCursorDurationMillis > 0L) {
                // 有平滑光标：记录 pending target，等真实 frameTime 创建 motion
                // Issue #737 评论 5782106370：同时缓存 origin/target caret offset，保证
                // forSelectionMove 拿到的 rect/offset/line 同源。
                pendingSelectionCaretTarget =
                    PendingSelectionCaretMove(
                        originCaretRect = originCaret,
                        targetCaretRect = targetCaret,
                        originCaretOffset = originCaretOffset,
                        targetCaretOffset = targetCaretOffset,
                    )
                _frameRequestVersion.update { it + 1L }
            } else {
                // cursor 动画关闭：直接跳到 target
                restingCaretRect = targetCaret
                drawSnapshotState = drawSnapshotState.copy(restingCaretRect = targetCaret)
                activeMotion = null
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
     * - composition 活跃时把 preedit layout 当候选几何交给
     *   [ComposeVisualFrameCoordinator.onProvisionalLayout]（不推进 committed baseline）。
     * - composition 结束后一律把真实 layout 交给 [ComposeVisualFrameCoordinator.onLayout]，
     *   由 onEditFact/onLayout 双向合流配对生成 patch（Issue #735 评论 5774895427）。
     *   即使 commit 后没有新的 onTextLayout 回调，preedit 阶段缓存的候选几何也够用
     *   （Issue #735 评论 5775326365）。
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
        val fingerprint = layoutFingerprint(snapshot)
        val fingerprintUnchanged = !compositionActive && fingerprint == lastObservedLayoutFingerprint
        if (fingerprintUnchanged) {
            // 纯 selection/caret 或纯滚动：不更新 layout epoch、不调 frameCoordinator、不重新发布相同 TextLayoutResult。
            // Issue #737 评论 5782447373：selection 变化时不要抢先把 restingCaretRect 落到 target —
            // selection 的可见运动统一交给 selection motion（onInputSnapshotResolved 记录 pending，
            // drainPendingPatchesAtFrame 创建 forSelectionMove）。若这里先把可见 resting caret 跳到 target，
            // draw 层会先把自定义 caret 画到目标位置，之后 motion 再从旧 origin 动画到同一 target，
            // 出现"瞬间跳到终点"；且 onInputSnapshotResolved 会拿到 restingCaretRect=target 当 originRect，
            // 让 traversal 判无效。只有明确不播放 cursor animation 时才静态落 target。
            val previousSelection = drawSnapshotState.layout?.selection
            val selectionChanged = snapshot.selection != previousSelection
            if (selectionChanged) {
                val policy = currentMotionPolicy.effective()
                val willAnimateSelection =
                    policy.cursorAnimationEnabledForEdit && policy.selectionCursorDurationMillis > 0L
                if (!willAnimateSelection) {
                    // 不播放光标动画：静态落 target，draw 层直接画到新 selection 位置
                    restingCaretRect = snapshot.cursorRect(snapshot.selection.end)
                }
                // 播放光标动画时：不更新 restingCaretRect，交给 selection motion；
                // motion 期间 motionSample 覆盖 resting caret，motion finished 后 sampleVisualScene 落 target。
            } else {
                // 纯滚动（selection 没变）：caret offset 没变，restingCaretRect 跟着 layout 更新到新位置。
                restingCaretRect = snapshot.cursorRect(snapshot.selection.end)
            }
            drawSnapshotState =
                drawSnapshotState.copy(
                    layout = snapshot,
                    restingCaretRect = restingCaretRect,
                )
            // Issue #737 评论 5782769758：fingerprintUnchanged 时 text/行几何完全一样，
            // 保留现有 motionSample（上面 copy 没碰 motionSample）是安全的 —
            // old sample 的 hiddenRanges 对新 layout 仍有效，不会裁错。
            return
        }
        lastObservedLayoutFingerprint = fingerprint

        // 真实 text/line geometry 变化：把新 layout 写进 _latestLayout 和 draw snapshot
        _latestLayout.update { snapshot }
        // Issue #728 评论 5754839786 缺口2：真实 layout 变化后算新 target caret —
        // Issue #737 评论 5787285321：target 先存局部变量，按 owner 决策再写字段。
        // 旧实现无条件 `restingCaretRect = target` 会把字段提前写成新 target，
        // 导致 AwaitingFact 分支 buildPendingPresentation 的 originCaret 拿到新 target
        // （settleMotionForPendingPresentation 在 activeMotion == null 时直接 return 不改字段），
        // pending sample caret 瞬间跳到终点；fact 到达升级 prepared motion 后 caret 再从 target 跳回
        // origin 再动画，表现"先跳终点 → 跳回 → 再动画"。现在由各 owner 分支显式决定何时写字段。
        val targetCaretRect = snapshot.cursorRect(snapshot.selection.end)

        // composition 活跃时只缓存 preedit layout，不生成 patch（不播放 preedit 的吞吐）
        if (compositionActive) {
            // #694 评论 5692161955 问题3 / Issue #735 评论 5774895427：
            // composition 活跃分支不调 frameCoordinator.onLayout（那会推进 committed baseline）。
            //
            // Issue #735 评论 5775326365：但要把 preedit layout 当候选几何交给
            // frameCoordinator.onProvisionalLayout — coordinator 只缓存 latest（平台已经有 B 的真实几何），
            // 不推进 committed baseline（仍保持 A）。
            frameCoordinator.onProvisionalLayout(snapshot)
            lastPresentedLayout = snapshot
            // Issue #737 评论 5782769758：composition layout 可能和上一笔 motion 的 text 不同，
            // 清掉过期 motionSample 防止 old sample + new layout 裁错。
            // composition 期间不播放吞吐，activeMotion 清空后由 restingCaretRect 填 drawSnapshot caret。
            // Issue #737 评论 5786405265 漏洞2：静态 owner 接管时必须统一结算旧 owner —
            // 不能只清 activeMotion，还要清 pendingPresentation / preparedMotionSequence / pending patch，
            // 否则旧 pendingPresentation 会在 sampleVisualScene 里复活，旧 prepared motion 的 patch
            // 会在 drainPendingPatchesAtFrame 里重新启动。
            settleToStaticOwner()
            // Issue #737 评论 5787285321：composition 静态状态接管时把 target 落到字段 —
            // 修改1移除无条件赋值后，由各 owner 分支显式决定何时写字段。
            restingCaretRect = targetCaretRect
            presentationGeneration++
            drawSnapshotState =
                drawSnapshotState.copy(
                    layout = snapshot,
                    motionSample = null,
                    restingCaretRect = restingCaretRect,
                    presentationGeneration = presentationGeneration,
                )
            return
        }

        // Core visual path（Undo/Redo/Programmatic/Load/Format 等真正需要 Core 驱动的修改）
        val update = frameCoordinator.onLayout(snapshot)
        applyFrameUpdate(update)
        lastPresentedLayout = snapshot
        // Issue #737 评论 5785295971：按明确状态处理 —
        // LayoutOnly：applyFrameUpdate 已静态发布新 layout + resting caret，无需额外处理。
        // AwaitingFact：text 变了但 fact 还没到 — 建立 pending presentation ownership，
        //   防止 BasicTextField 新正文裸画一帧。关键：先 settle running motion
        //   （互斥规则：不能同时有 running motion 和 pending presentation）。
        // NewPatch：applyFrameUpdate 已处理（包括 prepared motion）。
        when (update) {
            is FrameUpdate.LayoutOnly -> {
                // 初始 baseline 或纯几何变化 — applyFrameUpdate 已静态发布新 layout + resting caret
            }
            is FrameUpdate.AwaitingFact -> {
                // text 变了但 fact 还没到 — 建立 pending presentation ownership
                // 关键：先 settle running motion（互斥规则：不能同时有 running motion 和 pending presentation）
                // Issue #737 评论 5786405265 漏洞1：prepared motion 被 settle 时必须一并结算其
                // 对应的 pending patch + 重置 preparedMotionSequence。否则 P1 唤醒的 frame 先执行
                // drainPendingPatchesAtFrame 时会取出 P1 patch，因 activeMotion == null 重新从 P1
                // 构造 running motion，sampleVisualScene 把 P1 sample 写回 → layout=P2, motionSample=P1。
                // 核心规则：某个 presentation owner 被 settle 掉后，它对应的未启动 patch 也必须
                // 一起结算，不能下一帧重新启动。running motion（非 prepared）的 patch 已在 drain
                // 时消费过，pendingPatches 已空，无需额外清理。
                settleMotionForPendingPresentation()
                val previousLayout = drawSnapshotState.layout
                if (previousLayout != null) {
                    val pending = buildPendingPresentation(previousLayout, snapshot)
                    if (pending != null) {
                        pendingPresentation = pending
                        presentationGeneration++
                        drawSnapshotState =
                            drawSnapshotState.copy(
                                layout = snapshot,
                                motionSample = pending.sample,
                                restingCaretRect = pending.sample.caretRect,
                                presentationGeneration = presentationGeneration,
                            )
                    }
                }
            }
            is FrameUpdate.NewPatch -> {
                // patch 已生成 — applyFrameUpdate 已处理（包括 prepared motion）
            }
        }
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
     * 把帧协调器的更新结果应用到本地状态 — 暂存 patch 到待消费队列供 overlay 推进 motion。
     */
    private fun applyFrameUpdate(update: FrameUpdate) {
        when (update) {
            is FrameUpdate.LayoutOnly -> {
                // Issue #737 评论 5785295971：初始 baseline 或纯几何变化（text 不变）—
                // 可以直接静态发布新 layout + resting caret。
                // Issue #737 评论 5786405265 漏洞2：静态 owner 接管时必须统一结算旧 owner —
                // 此入口由 onAuthoritativeLayout 的 Core visual path 调用，若有 prepared motion /
                // pending patch，说明上一笔的 prepared motion 被 layout 变化打断，必须一并结算，
                // 否则下一帧 drainPendingPatchesAtFrame 会取出旧 patch 重新激活旧 motion。
                settleToStaticOwner()
                // Issue #737 评论 5787285321：LayoutOnly 静态发布新 layout 时同步更新字段本身 —
                // 修改1移除 onAuthoritativeLayout 的无条件赋值后，字段不再被提前写成 target，
                // 由本分支静态发布时显式写。drawSnapshotState.copy 的 restingCaretRect 用同值保持一致。
                restingCaretRect = update.snapshot.cursorRect(update.snapshot.selection.end)
                presentationGeneration++
                drawSnapshotState =
                    drawSnapshotState.copy(
                        layout = update.snapshot,
                        motionSample = null,
                        restingCaretRect = update.snapshot.cursorRect(update.snapshot.selection.end),
                        presentationGeneration = presentationGeneration,
                    )
            }
            is FrameUpdate.AwaitingFact -> {
                // Issue #737 评论 5785295971：text 变了但 fact 还没配对 —
                // 不在这里处理，由 onAuthoritativeLayout 处理（建立 pending presentation ownership）。
            }
            is FrameUpdate.NewPatch -> {
                val patchSequence = nextPendingSequence++
                pendingPatches.addLast(PendingPatch(sequence = patchSequence, patch = update.patch))
                _frameRequestVersion.update { it + 1L }
                _latestPatch.update { update.patch }
                Log.d(
                    TAG,
                    "patch_published: id=${update.patch.id} " +
                        "coreTxnIds=${update.patch.coreTransactionIds}",
                )
                // Issue #737 评论 5782769758：patch 配对成功后立刻建立 prepared motion。
                // Issue #737 评论 5784705864 缺口2：对整个 pending queue 合成，用合成 patch 构造唯一
                // prepared motion — 不只 prepare 当前 patch。这样多笔 patch 时 prepared motion 总是
                // 合成结果，drain 时 sequence 对上直接 start 合成 motion，不会"最后一笔绕过 batch 合成"。
                val composedPatch = ComposeVisualPatchBatch.compose(pendingPatches.map { it.patch })
                if (composedPatch != null) {
                    applyPreparedMotionFromPatch(composedPatch)
                    // Issue #737 评论 5786405265：只有创建 prepared motion 时才记录 sequence。
                    // Static 分支已清 pendingPatches + preparedMotionSequence = -1L，
                    // 不能在这里覆盖回 patchSequence，否则状态不一致。
                    if (activeMotion != null && activeMotion!!.isPrepared) {
                        preparedMotionSequence = patchSequence
                    }
                }
            }
        }
    }

    /**
     * Issue #737 评论 5782769758：patch 配对成功后立刻建立 prepared motion 的构造结果。
     */
    private sealed interface PreparedMotionResult {
        /** SYSTEM_SUPPRESSED：静态落最终画面，不创建 motion。 */
        data class Static(
            val layout: ComposeLayoutSnapshot,
            val caretRect: Rect,
        ) : PreparedMotionResult

        /** 创建了 prepared motion。 */
        data class Motion(
            val motion: CoordinatedEditMotion,
            val newLayout: ComposeLayoutSnapshot,
            val targetCaretRect: Rect,
        ) : PreparedMotionResult
    }

    /**
     * Issue #737 评论 5782769758：从 patch 构造 prepared motion（未开始计时）。
     *
     * 把当前 [drainPendingPatchesAtFrame] 里的 motion 构造逻辑（SYSTEM_SUPPRESSED、
     * isSelectionOnly、policy duration）提取出来，构造 prepared=true 的 motion。
     * prepared motion 的 [CoordinatedEditMotion.sample] 永远返回 progress=0 的结果。
     */
    private fun buildPreparedMotion(patch: ComposeVisualPatch): PreparedMotionResult {
        val policy = currentMotionPolicy.effective()
        // SYSTEM_SUPPRESSED：直接静态落最终画面
        if (patch.animationMode == AnimationMode.SYSTEM_SUPPRESSED) {
            return PreparedMotionResult.Static(
                layout = patch.newLayout,
                caretRect = patch.targetCaretRect,
            )
        }
        val oldText = patch.oldLayout.result.layoutInput.text.text
        val newText = patch.newLayout.result.layoutInput.text.text
        val isSelectionOnly =
            oldText == newText &&
                patch.insertedUnits.isEmpty() &&
                patch.deletedUnits.isEmpty()
        val motion =
            if (isSelectionOnly) {
                val selectionCursorDurationNanos =
                    policy.selectionCursorDurationMillis.coerceAtLeast(0L) * NANOS_PER_MS
                CoordinatedEditMotion.forSelectionMove(
                    oldLayout = patch.oldLayout,
                    newLayout = patch.newLayout,
                    originCaretRect = patch.originCaretRect,
                    targetCaretRect = patch.targetCaretRect,
                    originCaretOffset = patch.originCaretOffset,
                    targetCaretOffset = patch.targetCaretOffset,
                    frameTimeNanos = 0L,
                    durationNanos = if (policy.cursorAnimationEnabledForEdit) selectionCursorDurationNanos else 0L,
                    prepared = true,
                )
            } else {
                val textDurationNanos = policy.textDurationMillis.coerceAtLeast(0L) * NANOS_PER_MS
                val editDurationNanos = if (policy.textAnimationEnabledForEdit) textDurationNanos else 0L
                CoordinatedEditMotion.fromPatch(
                    patch = patch,
                    frameTimeNanos = 0L,
                    durationNanos = editDurationNanos,
                    prepared = true,
                )
            }
        return PreparedMotionResult.Motion(
            motion = motion,
            newLayout = patch.newLayout,
            targetCaretRect = patch.targetCaretRect,
        )
    }

    /**
     * Issue #737 评论 5784705864 缺口1：算 oldText→newText 的公共前后缀，返回被改变的 raw range。
     * oldRange 非 null 表示 oldText 中被删/替换的区间；newRange 非 null 表示 newText 中新增/替换的区间。
     * 纯 selection（text 不变）返回 (null, null)。
     */
    private fun computeChangedRawRanges(
        oldText: String,
        newText: String,
    ): Pair<TextRange?, TextRange?> {
        if (oldText == newText) return null to null
        val oldLen = oldText.length
        val newLen = newText.length
        var prefix = 0
        val minLen = minOf(oldLen, newLen)
        while (prefix < minLen && oldText[prefix] == newText[prefix]) prefix++
        var suffix = 0
        while (
            suffix < (minLen - prefix) &&
            oldText[oldLen - 1 - suffix] == newText[newLen - 1 - suffix]
        ) {
            suffix++
        }
        val oldRange = if (prefix < oldLen - suffix) TextRange(prefix, oldLen - suffix) else null
        val newRange = if (prefix < newLen - suffix) TextRange(prefix, newLen - suffix) else null
        return oldRange to newRange
    }

    /**
     * Issue #737 评论 5784705864 缺口1：从 oldLayout/newLayout 构造 pending presentation。
     * 不依赖 fact — 只用文本 diff 算粗粒度 ownership（整个变化区域裁掉/画 ghost）。
     * fact 到达后 prepared motion 的精确 per-glyph ownership 会原子替换，视觉一致。
     * @return null 表示纯 selection（text 未变），不需要 pending ownership。
     */
    private fun buildPendingPresentation(
        oldLayout: ComposeLayoutSnapshot,
        newLayout: ComposeLayoutSnapshot,
    ): PendingPresentation? {
        val oldText = oldLayout.result.layoutInput.text.text
        val newText = newLayout.result.layoutInput.text.text
        if (oldText == newText) return null
        val (oldChanged, newChanged) = computeChangedRawRanges(oldText, newText)
        val hiddenRanges = if (newChanged != null) listOf(newChanged) else emptyList()
        val glyphOverlays = mutableListOf<CoordinatedEditMotion.GlyphOverlay>()
        if (oldChanged != null) {
            glyphOverlays.add(
                CoordinatedEditMotion.GlyphOverlay(
                    key = nextPendingGlyphKey--,
                    range = oldChanged,
                    layout = oldLayout,
                    role = CoordinatedEditMotion.GlyphRole.Deleted,
                    clipFraction = 1f,
                ),
            )
        }
        // Issue #737 评论 5785295971：caret origin 从上一份 presentation owner 拿，
        // 不从 lastResolvedSelection 猜 — snapshotFlow 可能已经先把它更新成当前新 selection。
        // 上一份 presentation 的 caret 位置就是 restingCaretRect
        // （running motion 被 settle 后 restingCaretRect = motion.newCaretRect；
        //  resting 状态下 restingCaretRect 就是当前 caret 位置）。
        val originCaret = restingCaretRect ?: oldLayout.cursorRect(oldLayout.selection.end)
        val sample =
            CoordinatedEditMotion.Sample(
                caretRect = originCaret,
                glyphOverlays = glyphOverlays,
                hiddenRanges = hiddenRanges,
                finished = false,
                isValid = true,
            )
        return PendingPresentation(oldLayout, newLayout, sample)
    }

    /**
     * Issue #737 评论 5786405265 漏洞1：settle 当前 motion 为 pending presentation 接管做准备。
     *
     * 如果当前有 prepared motion，一并结算其对应的 pending patch + 重置 preparedMotionSequence，
     * 防止下一帧 drainPendingPatchesAtFrame 取出旧 patch 重新构造 running motion。
     * running motion（非 prepared）的 patch 已在 drain 时消费过，pendingPatches 已空，无需额外清理。
     */
    private fun settleMotionForPendingPresentation() {
        val existingMotion = activeMotion ?: return
        restingCaretRect = existingMotion.newCaretRect
        if (existingMotion.isPrepared) {
            val preparedSeq = preparedMotionSequence
            if (preparedSeq >= 0L) {
                pendingPatches.removeAll { it.sequence <= preparedSeq }
            }
            preparedMotionSequence = -1L
        }
        activeMotion = null
    }

    /**
     * Issue #737 评论 5786405265：统一结算旧 presentation owner —
     * 静态状态成为当前唯一 owner 时，必须把旧 owner 的所有状态字段一起清掉，
     * 不能只改 drawSnapshot。否则旧 pendingPresentation / prepared motion / pending patch
     * 会在下一帧被 sampleVisualScene / drainPendingPatchesAtFrame 重新激活，
     * 造成 "old sample + new layout" 或旧 prepared motion 复活。
     *
     * 调用时机：所有"静态状态成为当前唯一 owner"的入口（LayoutOnly / composition active /
     * SYSTEM_SUPPRESSED / PreparedMotionResult.Static）在原子写新静态 draw snapshot 之前调用。
     */
    private fun settleToStaticOwner() {
        activeMotion = null
        pendingPresentation = null
        preparedMotionSequence = -1L
        // prepared motion 对应的 pending patch 也一并结算（running motion 的 patch 已在 drain 时消费）
        if (pendingPatches.isNotEmpty()) {
            pendingPatches.clear()
        }
    }

    /**
     * Issue #737 评论 5782769758：从 patch 构造 prepared motion 并原子更新 draw snapshot。
     * layout 和 motionSample 带同一 presentation generation，防止 old sample + new layout。
     */
    private fun applyPreparedMotionFromPatch(patch: ComposeVisualPatch) {
        // Issue #737 评论 5784705864 缺口1：fact 到达，pending presentation 原子升级成 prepared motion。
        // prepared motion 的 progress=0 sample 视觉与 pending presentation 一致（都是 ownership 状态），
        // drawSnapshotState 原子覆盖，无"先释放再创建"空隙。
        pendingPresentation = null
        presentationGeneration++
        when (val result = buildPreparedMotion(patch)) {
            is PreparedMotionResult.Static -> {
                // Issue #737 评论 5786405265 漏洞2：静态 owner 接管时统一结算旧 owner —
                // pendingPresentation 已在方法开头清，但 preparedMotionSequence / pending patch
                // 仍需清，否则旧 prepared motion 的 patch 会在 drainPendingPatchesAtFrame 里
                // 重新启动，覆盖 Static 的静态画面。
                activeMotion = null
                pendingPresentation = null
                preparedMotionSequence = -1L
                if (pendingPatches.isNotEmpty()) {
                    pendingPatches.clear()
                }
                restingCaretRect = result.caretRect
                drawSnapshotState =
                    drawSnapshotState.copy(
                        layout = result.layout,
                        motionSample = null,
                        restingCaretRect = result.caretRect,
                        presentationGeneration = presentationGeneration,
                    )
            }
            is PreparedMotionResult.Motion -> {
                activeMotion = result.motion
                restingCaretRect = result.targetCaretRect
                val preparedSample = result.motion.sample(0L)
                drawSnapshotState =
                    drawSnapshotState.copy(
                        layout = result.newLayout,
                        motionSample = preparedSample,
                        restingCaretRect = result.targetCaretRect,
                        presentationGeneration = presentationGeneration,
                    )
            }
        }
    }

    /**
     * 是否有待消费的 patch 或 pending selection caret 移动。
     */
    fun hasPendingPatches(): Boolean = pendingPatches.isNotEmpty() || pendingSelectionCaretTarget != null

    /**
     * Issue #737：在 Compose 帧时钟的回调里消费所有待处理的 patch 并构造 [activeMotion]。
     *
     * 新架构：从 patch 直接构造 [CoordinatedEditMotion.fromPatch] — 一次性构造，
     * 不再先 timeline.applyPatch 接管文字再创建 motion。
     *
     * 顺序：
     * 1. 开头先应用 [pendingMotionPolicy]（清 activeMotion）。
     * 2. 处理 pending 纯 selection caret 移动（创建 forSelectionMove motion）。
     * 3. 把所有 pendingPatches 合成 batch，从 batch 直接构造 [CoordinatedEditMotion]。
     *
     * Issue #732 评论 5763493968 第3节：policy 切换、patch 消费、motion 创建
     * 全部进同一只 frame clock。
     *
     * Issue #737 评论 5782769758：[applyFrameUpdate] 收到 NewPatch 时已构造 prepared motion
     * （未开始计时），本方法负责给 prepared motion 盖开始时间（start）。
     * 只有在"没有 prepared motion"（policy 切换清了，或多笔 batch 需要合成）时
     * 才从 batch 重新构造 running motion（保持 batch 合并语义）。
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
            // policy 真正变化时清掉旧 motion — 让后续 drain 用新 policy 重新决定是否创建 motion。
            if (previousPolicy != currentMotionPolicy) {
                activeMotion = null
                preparedMotionSequence = -1L
                drawSnapshotState =
                    drawSnapshotState.copy(
                        motionSample = null,
                        // restingCaretRect 保持当前值，不清空
                    )
            }
        }

        // Issue #728 评论 5755336403 缺口2：先处理 pending 纯 selection caret 移动 —
        // 在真实 frameTime 创建 forSelectionMove motion。
        val pendingSelection = pendingSelectionCaretTarget
        if (pendingSelection != null) {
            pendingSelectionCaretTarget = null
            val policy = currentMotionPolicy.effective()
            val selectionCursorDurationNanos =
                policy.selectionCursorDurationMillis.coerceAtLeast(0L) * NANOS_PER_MS
            val caretDuration =
                if (policy.cursorAnimationEnabledForEdit) selectionCursorDurationNanos else 0L
            val layoutForMove = _latestLayout.value
            if (layoutForMove != null) {
                activeMotion =
                    CoordinatedEditMotion.forSelectionMove(
                        oldLayout = layoutForMove,
                        newLayout = layoutForMove,
                        originCaretRect = pendingSelection.originCaretRect,
                        targetCaretRect = pendingSelection.targetCaretRect,
                        originCaretOffset = pendingSelection.originCaretOffset,
                        targetCaretOffset = pendingSelection.targetCaretOffset,
                        frameTimeNanos = frameTimeNanos,
                        durationNanos = caretDuration,
                    )
            }
            // restingCaretRect 落到 target（motion 完成后无缝接上）
            restingCaretRect = pendingSelection.targetCaretRect
        }

        if (pendingPatches.isEmpty()) {
            // Issue #737 评论 5782769758：无 pending patch 但可能有 prepared motion 需要启动。
            // applyFrameUpdate 构造 prepared motion 后，如果没有 pending patch，
            // 说明 prepared motion 已构造但还没 start — 在这里 start。
            val motion = activeMotion
            if (motion != null && motion.isPrepared) {
                activeMotion = motion.start(frameTimeNanos)
                preparedMotionSequence = -1L
            }
            return emptyList()
        }

        // 收集 consumed patches 用于返回
        val consumed = pendingPatches.map { it.patch }.toList()
        val consumedMaxSequence = pendingPatches.last().sequence
        pendingPatches.clear()

        // Issue #737 评论 5784705864 缺口2：prepared motion 只有在覆盖整个 pending queue
        // （preparedMotionSequence == consumedMaxSequence）时才能直接 start；
        // 否则（policy 切换清了，或 prepared motion 没覆盖整个 queue）重新 compose 构造。
        val existingMotion = activeMotion
        if (existingMotion != null && existingMotion.isPrepared &&
            preparedMotionSequence == consumedMaxSequence
        ) {
            activeMotion = existingMotion.start(frameTimeNanos)
            preparedMotionSequence = -1L
            return consumed
        }

        // 没有 prepared motion 或 prepared motion 没覆盖整个 queue — 重新 compose 构造。
        preparedMotionSequence = -1L
        val framePatch = ComposeVisualPatchBatch.compose(consumed) ?: return consumed

        // Issue #737 评论 5781084709 修复点 5：SYSTEM_SUPPRESSED 直接静态落最终画面，不创建 active motion。
        // ComposeVisualFrameCoordinator 仍会把 AnimationMode.SYSTEM_SUPPRESSED 写进 patch，
        // 但本方法必须读取 framePatch.animationMode 并收口 — 否则仍会正常创建 CoordinatedEditMotion。
        // 这个检查在 isSelectionOnly 判断之前，因为 SYSTEM_SUPPRESSED 优先级最高 —
        // 无论是否 selection-only，系统抑制都应该静态完成。
        if (framePatch.animationMode == AnimationMode.SYSTEM_SUPPRESSED) {
            // Issue #737 评论 5786405265 漏洞2：静态 owner 接管时统一结算旧 owner —
            // activeMotion 已在此设 null，pendingPatches 已在前面 clear（第 884 行），
            // 但 pendingPresentation / preparedMotionSequence 仍需清，否则旧 pendingPresentation
            // 会在 sampleVisualScene 里复活，覆盖 SYSTEM_SUPPRESSED 的静态画面。
            activeMotion = null
            pendingPresentation = null
            preparedMotionSequence = -1L
            restingCaretRect = framePatch.targetCaretRect
            presentationGeneration++
            drawSnapshotState =
                drawSnapshotState.copy(
                    motionSample = null,
                    layout = framePatch.newLayout,
                    restingCaretRect = framePatch.targetCaretRect,
                    presentationGeneration = presentationGeneration,
                )
            return listOf(framePatch)
        }

        // Issue #737：从 patch 直接构造 CoordinatedEditMotion — 一次性构造，
        // 不再先 timeline.applyPatch 接管文字再创建 motion。
        // - traversal 有效时 motion 携带 caret + glyph channels
        // - traversal 无效时 motion 无效，glyph channels 为空，直接显示最终静态正文
        val policy = currentMotionPolicy.effective()
        val textDurationNanos = policy.textDurationMillis.coerceAtLeast(0L) * NANOS_PER_MS
        // Issue #728 评论 5755336403 缺口4：selection-only 必须用 oldText == newText 判断 —
        // Enter / 删除 Enter 是正文编辑，只是换行符没有 glyph，允许没有 inserted/deleted visual unit。
        // 不能用 "unit key 为空" 推断 "没有文本编辑"。
        val oldText = framePatch.oldLayout.result.layoutInput.text.text
        val newText = framePatch.newLayout.result.layoutInput.text.text
        val isSelectionOnly =
            oldText == newText &&
                framePatch.insertedUnits.isEmpty() &&
                framePatch.deletedUnits.isEmpty()
        if (isSelectionOnly) {
            // selection-only 移动：caret 单独用 selectionCursorDurationMillis，文字 channels 为空
            // Issue #732 评论 5764716281 硬问题1：用派生值 cursorAnimationEnabledForEdit 和
            // selectionCursorDurationMillis，coordinated 模式下不被旧 cursorEnabled=false 卡死。
            val selectionCursorDurationNanos =
                policy.selectionCursorDurationMillis.coerceAtLeast(0L) * NANOS_PER_MS
            activeMotion =
                CoordinatedEditMotion.forSelectionMove(
                    oldLayout = framePatch.oldLayout,
                    newLayout = framePatch.newLayout,
                    originCaretRect = framePatch.originCaretRect,
                    targetCaretRect = framePatch.targetCaretRect,
                    originCaretOffset = framePatch.originCaretOffset,
                    targetCaretOffset = framePatch.targetCaretOffset,
                    frameTimeNanos = frameTimeNanos,
                    durationNanos =
                        if (policy.cursorAnimationEnabledForEdit) selectionCursorDurationNanos else 0L,
                )
        } else {
            // text edit（包括 Enter / 删除 Enter 无 glyph unit 的情况）—
            // Issue #735 评论 5773604666 问题2：正文 edit motion 一律一只钟 —
            // caret 和 glyph 共用同一个 durationNanos/progress。
            // - textAnimationEnabledForEdit=true：durationNanos = textDurationNanos
            // - 否则：durationNanos = 0L（瞬时完成）
            val editDurationNanos =
                if (policy.textAnimationEnabledForEdit) textDurationNanos else 0L
            activeMotion =
                CoordinatedEditMotion.fromPatch(
                    patch = framePatch,
                    frameTimeNanos = frameTimeNanos,
                    durationNanos = editDurationNanos,
                )
        }
        return listOf(framePatch)
    }

    /**
     * Issue #737：在 Compose 帧时钟的回调里采样当前 motion — overlay 在每帧 draw 前调用。
     *
     * 新架构：`motionSample = activeMotion?.sample(frameTimeNanos)` →
     * 更新 [drawSnapshotState]（motionSample + restingCaretRect）。
     * 不再先 sample motion 再 sample timeline — timeline 已删除。
     *
     * 方法名保留 [sampleVisualScene]（[EditorTextFieldDrawLayer] 的 LaunchedEffect 调用它），
     * 内部改为 sample [activeMotion]。
     *
     * @param frameTimeNanos 当前帧时间戳（来自 Compose frame clock）。
     * @return 当前帧的 motion sample（null 表示无 active motion）。
     */
    fun sampleVisualScene(frameTimeNanos: Long): CoordinatedEditMotion.Sample? {
        // Issue #737 评论 5785295971：presentation owner 互斥 —
        // 同一时刻只能有一个状态源：running motion 或 pending presentation，不能同时有。
        // onAuthoritativeLayout 建立 pending presentation 时已 settle running motion，
        // 但这里仍做互斥保护：activeMotion 优先，pendingPresentation 次之。
        val motion = activeMotion
        if (motion != null) {
            val motionSample = motion.sample(frameTimeNanos)
            if (motionSample.finished) {
                // Issue #728 评论 5754839786 缺口2：motion finished 后把 target caret 落到 restingCaretRect，
                // 再清 activeMotion — 下一帧 sampleVisualScene 用 restingCaretRect 填 drawSnapshot，
                // 屏幕 caret 停在 motion 终点，不跳回原点也不消失。
                //
                // Issue #737 评论 5781634285 修复点 2：finished 时直接收口 —
                // 把 caret target 写入 restingCaretRect，清 activeMotion，
                // 同一帧把 drawSnapshotState.motionSample 清成 null。
                // 最终画面直接回到 BasicTextField + resting caret，不保留 completed overlay。
                restingCaretRect = motionSample.caretRect
                activeMotion = null
                drawSnapshotState =
                    drawSnapshotState.copy(
                        motionSample = null,
                        restingCaretRect = restingCaretRect,
                    )
            } else {
                // #708 评论 5723410606 第一节：同步 draw snapshot 的 motionSample + restingCaretRect —
                // draw 层下一帧 drawWithContent 直接读。
                drawSnapshotState =
                    drawSnapshotState.copy(
                        motionSample = motionSample,
                        restingCaretRect = restingCaretRect,
                    )
            }
            return motionSample
        }
        // 无 active motion — 如果有 pending presentation，用它的 sample
        // Issue #737 评论 5784705864 缺口1：pending presentation 的 sample 保持 ownership，
        // 不清成 null（否则 pending ownership 丢失，BasicTextField 裸画）。
        val pending = pendingPresentation
        if (pending != null) {
            drawSnapshotState =
                drawSnapshotState.copy(
                    motionSample = pending.sample,
                    restingCaretRect = restingCaretRect,
                )
            return pending.sample
        }
        // 无 active motion、无 pending presentation — resting
        // Issue #728 评论 5754839786 缺口2：无 active motion 时用 restingCaretRect 填，
        // 保证静止/selection 移动后屏幕 caret 不消失。
        drawSnapshotState =
            drawSnapshotState.copy(
                motionSample = null,
                restingCaretRect = restingCaretRect,
            )
        return null
    }

    /**
     * 是否还有活动动画 — overlay 据此决定是否继续推进帧时钟。
     *
     * Issue #737：只检查 [activeMotion] — 不再有 timeline。
     *
     * @param frameTimeNanos 当前帧时间戳。
     */
    fun hasActiveVisuals(frameTimeNanos: Long): Boolean {
        val motion = activeMotion ?: return false
        return !motion.isFinished(frameTimeNanos)
    }

    /**
     * 清除所有状态 — 章节切换或 detach 时调用。
     */
    fun clear() {
        frameCoordinator.clear()
        pendingPatches.clear()
        _frameRequestVersion.update { 0L }
        _latestLayout.update { null }
        _latestPatch.update { null }
        // Issue #732 评论 5763493968 第3节：重置运行时 policy 切换状态
        currentMotionPolicy = EditorMotionPolicy()
        pendingMotionPolicy = null
        // Issue #737：清空唯一协调 motion
        activeMotion = null
        // Issue #737 评论 5784705864 缺口1：清空 pending presentation
        pendingPresentation = null
        // Issue #737 评论 5784705864 缺口2：清空 prepared motion sequence
        preparedMotionSequence = -1L
        // Issue #728 评论 5754839786 缺口2：清空静止 caret rect
        restingCaretRect = null
        // Issue #728 评论 5755336403 缺口2：清空 pending 纯 selection caret 移动
        pendingSelectionCaretTarget = null
        lastPresentedLayout = null
        // #708 评论 5723410606 第一节/第二节/第三节：重置 draw snapshot / fingerprint
        drawSnapshotState = ComposeEditorDrawSnapshot()
        lastObservedLayoutFingerprint = null
        // Issue #737 评论 5782769758：重置 presentation generation 计数器
        presentationGeneration = 0L
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
     * 旧 patch 会带着原来的 insertedUnits/deletedUnits 再进入 motion 构造，
     * 把文字动画重新启动。本方法只把 effective policy 写进 [pendingMotionPolicy]，
     * 并增加一次 [frameRequestVersion] 唤醒帧循环；
     * [drainPendingPatchesAtFrame] 开头会应用 pending policy（清 motion），
     * 然后再 drain patch、创建 [CoordinatedEditMotion]。
     *
     * 不在 UI effect 里立刻清 motion — policy 切换、patch 消费、motion 创建
     * 全部进同一只 frame clock（[EditorTextFieldDrawLayer] 的 withFrameNanos）。
     *
     * @param policy 新的动画策略 — 内部会先 effective() 收口 reduce-motion。
     */
    fun updateMotionPolicy(policy: EditorMotionPolicy) {
        pendingMotionPolicy = policy.effective()
        _frameRequestVersion.update { it + 1L }
    }
}
