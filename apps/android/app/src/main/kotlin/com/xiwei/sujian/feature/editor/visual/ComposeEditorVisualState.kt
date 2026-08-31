package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update

/**
 * #641 评论1 第4/5节：Compose 显示层视觉状态 — 保存上一份和当前一份
 * [ComposeLayoutSnapshot]，根据 Core 的 [EditorVisualIntent] 算受影响 UTF-16 range。
 *
 * #644 评论 5467821839 第5节剩余子项：本类只负责保存 previous/current layout、
 * pending intent、active transaction、progress，并调用 [ComposeVisualRebase] 里的纯函数。
 * viewport 恢复、session attach、正文写回不进 visual 层。rebase 物化、range subtract/split、
 * retained moves、startFrame 几何计算全部在 [ComposeVisualRebase]。
 *
 * 动画层只"画"，绝不能再改变 viewport / selection / IME 几何。
 * [onAuthoritativeLayout] 由 [BasicTextField] 的 `onTextLayout` 回调调用，
 * 把系统最终 [TextLayoutResult] 记录为权威布局，不反向修改输入。
 *
 * #641 评论 问题3 + 评论 5457777142 问题2：transaction/rebase —
 * 新事务到来时如果旧事务还在跑，先用旧 transaction + 当前 progress 物化
 * [ComposeVisualFrame]，再把它作为新事务的 start_frame。
 *
 * #641 评论 5457777142 问题4：[onVisualIntent] 签名改为接收 [EditorMotionPolicy]
 * 而非 `durationMillis`，overlay 据此决定 text/cursor 两条 timeline。
 */
class ComposeEditorVisualState(
    initialDrawsVisualCursor: Boolean = false,
) {
    /** 上一份布局快照 — 删除文字动画按旧 range 的 bounding box 画旧布局。 */
    private var previousSnapshot: ComposeLayoutSnapshot? = null

    /** 当前布局快照 — 来自系统 [BasicTextField] 的最终 [TextLayoutResult]。 */
    private var currentSnapshot: ComposeLayoutSnapshot? = null

    /**
     * 当前正在动画的 UTF-16 range — 这些 range 在 [OutputTransformation] 里被设为透明，
     * overlay 补画动画过程；动画完成立即从该列表删除，系统正文已在最终位置。
     */
    private val _hiddenRanges = MutableStateFlow<List<TextRange>>(emptyList())
    val hiddenRanges: StateFlow<List<TextRange>> = _hiddenRanges.asStateFlow()

    /**
     * 视觉光标是否由 overlay 绘制 — true 时 [BasicTextField] 的 cursorBrush 设为透明，
     * overlay 从 `oldResult.getCursorRect(oldSelection.end)` 插值到
     * `newResult.getCursorRect(newSelection.end)`。
     */
    private val _drawsVisualCursor = MutableStateFlow(initialDrawsVisualCursor)
    val drawsVisualCursor: StateFlow<Boolean> = _drawsVisualCursor.asStateFlow()

    /** 当前活跃的视觉意图 — 供 overlay 读取动画类型。 */
    private val _activeIntent = MutableStateFlow<EditorVisualIntent?>(null)
    val activeIntent: StateFlow<EditorVisualIntent?> = _activeIntent.asStateFlow()

    /**
     * 视觉光标插值快照 — cursor animate=true 时由 [onVisualIntent] 根据当前/上一份
     * [TextLayoutResult] 的 cursor rect 计算并保存，overlay 据此按 progress 插值。
     */
    private val _visualCursorSnapshot = MutableStateFlow<VisualCursorSnapshot?>(null)
    val visualCursorSnapshot: StateFlow<VisualCursorSnapshot?> = _visualCursorSnapshot.asStateFlow()

    /** #641 评论 问题3：当前事务 ID — 单调递增，overlay 据此判断是否需要重新启动动画。 */
    private var currentTransactionId: Long = 0L

    /**
     * #641 评论 问题3：当前活跃的视觉动画事务 — 供 overlay 读取 motionPolicy 和 ranges。
     *
     * #641 评论 5458283021 问题1a：删除 previousTransaction 滞后缓存。
     * rebase 直接读 [_activeTransaction]（当前正在跑的事务），不再慢一笔。
     */
    private val _activeTransaction = MutableStateFlow<ComposeVisualTransaction?>(null)
    val activeTransaction: StateFlow<ComposeVisualTransaction?> = _activeTransaction.asStateFlow()

    /**
     * #641 评论 5457777142 问题2 + 评论 5458283021 问题1c：overlay 报告的当前动画 progress —
     * 新事务到来时用它物化 [ComposeVisualFrame] 作为新事务的 start_frame。
     *
     * #641 评论 5458283021 问题1c：coordinated=false 时 text/cursor 是两条 timeline，
     * 分别报告 textProgress / cursorProgress，物化 cursor 用 cursorProgress 不再错算。
     */
    private val _currentTextProgress = MutableStateFlow(0f)
    val currentTextProgress: StateFlow<Float> = _currentTextProgress.asStateFlow()

    private val _currentCursorProgress = MutableStateFlow(0f)
    val currentCursorProgress: StateFlow<Float> = _currentCursorProgress.asStateFlow()

    /**
     * #641 评论 5459896691 第1项：overlay 报告的 rebaseProgress —
     * 控制 startFrame 文字层淡出的独立 timeline。
     */
    private val _currentRebaseProgress = MutableStateFlow(0f)
    val currentRebaseProgress: StateFlow<Float> = _currentRebaseProgress.asStateFlow()

    /**
     * #641 评论 5458283021 问题2a：pending visual intent — onVisualIntent 保存，
     * 等 onAuthoritativeLayout 到达对应 new layout 后才生成 ComposeVisualTransaction
     * 和 retained moves（两阶段 retained reflow）。
     */
    private data class PendingVisualIntent(
        val intent: EditorVisualIntent,
        val motionPolicy: EditorMotionPolicy,
    )

    private var pendingVisualIntent: PendingVisualIntent? = null

    /**
     * #641 评论1 第5节：系统给出权威布局 — 只记录，不修改输入几何。
     * 动画层据此算受影响 range，但不 scrollTo、不改 selection、不改 editor height。
     *
     * #641 评论 5458283021 问题2a：两阶段 retained reflow —
     * onVisualIntent 只保存 pending visual intent，不提前算 reflow。
     * 新 TextLayoutResult 到达本方法后，若存在 pending 且 text 长度匹配 pending.newTextLength，
     * 用确定的 oldLayout（previousSnapshot）+ newLayout（currentSnapshot）生成
     * ComposeVisualTransaction 和 retained moves，消费 pending。
     */
    fun onAuthoritativeLayout(
        result: TextLayoutResult,
        selection: TextRange,
        scrollY: Int,
    ) {
        previousSnapshot = currentSnapshot
        currentSnapshot = ComposeLayoutSnapshot(result, selection, scrollY)
        // 新 layout 到达后，若当前活跃 intent 的 cursor 要动画，重新构建 cursor snapshot。
        if (_activeIntent.value?.cursor?.animate == true) {
            buildCursorSnapshot()?.let { _visualCursorSnapshot.update { it } }
        }
        // #641 评论 5458283021 问题2a + 评论 5458880786 问题2e：pending visual intent 补算 retainedMoves
        // + 同步 hiddenRanges。抽成 [applyPendingRetainedMoves] 降低 onAuthoritativeLayout 嵌套深度。
        applyPendingRetainedMoves(result)
    }

    /**
     * #641 评论 5458283021 问题2a：两阶段 — pending visual intent 在新 layout 到达后补算 retainedMoves。
     * 不创建新 transaction（transaction 已在 onVisualIntent 立即创建），
     * 只更新当前 activeTransaction 的 retainedMoves + oldLayout/newLayout。
     *
     * #641 评论 5458880786 问题2e：补算 retainedMoves 后同步更新 _hiddenRanges。
     */
    private fun applyPendingRetainedMoves(result: TextLayoutResult) {
        val pending = pendingVisualIntent ?: return
        // #641 评论 5459531909 第1项：layout 关联改成正文一致才认这份 layout。
        if (result.layoutInput.text.text != pending.intent.expectedNewText) return
        pendingVisualIntent = null
        val retainedMoves =
            ComposeVisualRebase.computeRetainedMoves(pending.intent, previousSnapshot, currentSnapshot)
        val active = _activeTransaction.value ?: return
        if (active.id != pending.intent.transactionId) return
        val effective = active.motionPolicy.effective()
        val hasText = effective.textEnabled && pending.intent.textKind != TextVisualKind.None
        if (hasText) {
            val baseOwnedNewRanges =
                (
                    when (pending.intent.textKind) {
                        TextVisualKind.Insert -> pending.intent.newRanges
                        TextVisualKind.Move -> pending.intent.newRanges
                        else -> emptyList()
                    }
                ).filter { it.start < it.end }
            val retainedNewRanges =
                retainedMoves.map { it.newRange }.filter { it.start < it.end }
            // #641 评论 5459531909 第2项：补算 retainedMoves 后仍要保留 frozenStartFrame.suppressedCurrentRanges。
            val frozenSuppressed =
                active.startFrame?.suppressedCurrentRanges ?: emptyList()
            val mappedSuppressed =
                ComposeVisualRebase.mapSuppressedRangesThroughReplace(frozenSuppressed, pending.intent.replaceBounds)
            val suppressedNotOverlapping =
                ComposeVisualRebase.subtractRanges(mappedSuppressed, baseOwnedNewRanges + retainedNewRanges)
            _hiddenRanges.update {
                baseOwnedNewRanges + retainedNewRanges + suppressedNotOverlapping
            }
        }
        _activeTransaction.update {
            it?.copy(
                oldLayout = previousSnapshot,
                newLayout = currentSnapshot,
                retainedMoves = retainedMoves,
            )
        }
    }

    /**
     * #641 评论1 第5节 / 问题2 / 问题3 + 评论 5457777142 问题2/问题3/问题4：
     * Core 给出视觉意图 — 设置受影响 UTF-16 range、动画类型、cursor 和 transaction。
     *
     * #641 评论 5458283021 问题2a：两阶段 retained reflow —
     * transaction 立即创建（overlay 能立即开始画文字动画、hiddenRanges 生效），
     * 只是 retainedMoves 初始为 emptyList，等 onAuthoritativeLayout 到达后
     * 用确定的 old/new layout 补算 retainedMoves。
     *
     * #641 评论 5458283021 问题3c：把 policy 提前落实到视觉状态 —
     * 用 [EditorMotionPolicy.effective] 算 hasTextAnimation/hasCursorAnimation，
     * 只在真正有动画时设置 hiddenRanges/drawsVisualCursor。
     *
     * @param motionPolicy 动画策略 — 调用方传入原始策略，本方法内部调用 [EditorMotionPolicy.effective]。
     */
    fun onVisualIntent(
        intent: EditorVisualIntent,
        motionPolicy: EditorMotionPolicy,
    ) {
        // #641 评论 问题3：分配新事务 ID。
        val newId = currentTransactionId + 1
        currentTransactionId = newId
        val intentWithId = intent.copy(transactionId = newId)

        // #641 评论 5458880786 问题1d：先冻结上一事务当前帧，再切 _activeIntent / 新 cursor snapshot。
        // #641 评论 5460160958 问题2：frozenStartFrame 马上要交给本事务（C）绘制，
        // surviving targetRange 必须是 C 的 new text 坐标，因此用 incoming 的 replaceBounds 映射。
        val runningTransaction = _activeTransaction.value
        val frozenStartFrame =
            ComposeVisualRebase.materializeStartFrame(
                ComposeVisualRebase.MaterializeStartFrameParams(
                    transaction = runningTransaction,
                    textProgress = _currentTextProgress.value,
                    cursorProgress = _currentCursorProgress.value,
                    rebaseProgress = _currentRebaseProgress.value,
                    nextReplaceBounds = intentWithId.replaceBounds,
                    hiddenRanges = _hiddenRanges.value,
                    cursorSnapshot = _visualCursorSnapshot.value,
                ),
            )

        _activeIntent.update { intentWithId }

        // #641 评论 5458283021 问题3c：把 policy 提前落实到视觉状态。
        val effective = motionPolicy.effective()
        val hasTextAnimation =
            effective.textEnabled && intentWithId.textKind != TextVisualKind.None
        val hasCursorAnimation = effective.cursorEnabled && intentWithId.cursor?.animate == true

        // #641 评论 5457777142 问题3 + 评论 5458283021 问题3c：hiddenRanges 修正。
        // Delete 不隐藏新正文 range。需要隐藏的是当前正文里由 overlay 接管的范围：
        // Insert/Move 的 newRanges、retained move 的 newRange。
        // #641 评论 5458283021 问题3c：只在 hasTextAnimation 时设置 hiddenRanges。
        val currentOwnedNewRanges =
            if (hasTextAnimation) {
                (
                    when (intentWithId.textKind) {
                        TextVisualKind.Insert -> intentWithId.newRanges
                        TextVisualKind.Delete -> emptyList()
                        TextVisualKind.Move -> intentWithId.newRanges
                        TextVisualKind.None -> emptyList()
                    }
                ).filter { it.start < it.end }
            } else {
                emptyList()
            }

        // #641 评论 5458283021 问题2a：两阶段 retained reflow。
        val currentSnapshotLocal = currentSnapshot
        // #641 评论 5459531909 第1项：layout 关联改成正文一致。
        val canComputeRetainedNow =
            currentSnapshotLocal != null &&
                currentSnapshotLocal.result.layoutInput.text.text == intentWithId.expectedNewText
        val retainedMoves =
            if (canComputeRetainedNow) {
                ComposeVisualRebase.computeRetainedMoves(intentWithId, previousSnapshot, currentSnapshot)
            } else {
                emptyList()
            }

        val currentRetainedNewRanges =
            retainedMoves.map { it.newRange }.filter { it.start < it.end }
        // #641 评论 5459531909 第2项：frozen startFrame 保留抑制范围。
        val mappedSuppressed =
            ComposeVisualRebase.mapSuppressedRangesThroughReplace(
                frozenStartFrame?.suppressedCurrentRanges ?: emptyList(),
                intentWithId.replaceBounds,
            )
        val suppressedNotOverlapping =
            ComposeVisualRebase.subtractRanges(
                mappedSuppressed,
                currentOwnedNewRanges + currentRetainedNewRanges,
            )
        _hiddenRanges.update {
            currentOwnedNewRanges + currentRetainedNewRanges + suppressedNotOverlapping
        }

        // #641 评论 问题2 + 评论 5458283021 问题3c：只要 cursor?.animate == true 且
        // hasCursorAnimation，就画视觉光标。reduceMotion/cursorEnabled=false 时不画。
        if (hasCursorAnimation) {
            _drawsVisualCursor.update { true }
            buildCursorSnapshot()?.let { _visualCursorSnapshot.update { it } }
        } else {
            _drawsVisualCursor.update { false }
            _visualCursorSnapshot.update { null }
        }

        buildAndActivateTransaction(
            intent = intentWithId,
            motionPolicy = effective,
            retainedMoves = retainedMoves,
            startFrame = frozenStartFrame,
        )
        // 若不能立即算 retainedMoves，保存 pending 等 onAuthoritativeLayout 补算。
        if (!canComputeRetainedNow) {
            pendingVisualIntent = PendingVisualIntent(intent = intentWithId, motionPolicy = effective)
        } else {
            pendingVisualIntent = null
        }
    }

    /**
     * #641 评论 5458283021 问题2a：用确定的 old/new layout 生成 ComposeVisualTransaction，
     * 设为活跃事务。retainedMoves 由调用方决定（两阶段：初始 emptyList，onAuthoritativeLayout 后补算）。
     *
     * #641 评论 5458880786 问题1d：startFrame 由 [onVisualIntent] 提前物化传入，
     * 不再在本方法内部调 materializeStartFrame — 避免物化时读到下一事务的 cursor snapshot。
     *
     * #641 评论 5460373035 问题2：用 effectiveOldRanges = subtractRanges(intent.oldRanges, startFrame.ownedOldRanges)。
     * startFrame 已接管的 old range 从本事务 oldRanges 减掉 — 同一段旧文字永远只由一条绘制路径拥有。
     */
    private fun buildAndActivateTransaction(
        intent: EditorVisualIntent,
        motionPolicy: EditorMotionPolicy,
        retainedMoves: List<RetainedMove>,
        startFrame: ComposeVisualFrame?,
    ) {
        val effectiveOldRanges =
            ComposeVisualRebase.subtractRanges(
                intent.oldRanges,
                startFrame?.ownedOldRanges.orEmpty(),
            )
        val transaction =
            ComposeVisualTransaction(
                id = intent.transactionId,
                oldLayout = previousSnapshot,
                newLayout = currentSnapshot,
                oldRanges = effectiveOldRanges,
                newRanges = intent.newRanges,
                retainedMoves = retainedMoves,
                textKind = intent.textKind,
                cursor = intent.cursor,
                startFrame = startFrame,
                motionPolicy = motionPolicy,
            )
        _activeTransaction.update { transaction }
    }

    /**
     * 从当前/上一份 [TextLayoutResult] 取真实 cursor rect 构建插值快照。
     * 任一 layout 缺失时不构建快照（overlay 只画已有的一侧）。
     * 转发到 [ComposeVisualRebase.buildCursorSnapshot] 纯函数。
     */
    private fun buildCursorSnapshot(): VisualCursorSnapshot? =
        ComposeVisualRebase.buildCursorSnapshot(
            previousSnapshot = previousSnapshot,
            currentSnapshot = currentSnapshot,
            intent = _activeIntent.value,
        )

    /**
     * #641 评论 5457777142 问题2 + 评论 5458283021 问题1c：overlay 报告当前动画 progress —
     * 新事务到来时用它物化 [ComposeVisualFrame]。
     */
    fun reportProgress(
        textProgress: Float,
        cursorProgress: Float,
        rebaseProgress: Float,
    ) {
        _currentTextProgress.update { textProgress }
        _currentCursorProgress.update { cursorProgress }
        _currentRebaseProgress.update { rebaseProgress }
    }

    /**
     * #641 评论1 第5节：动画结束 — 清 hiddenRanges，系统正文马上可见。
     * 由 overlay 的动画完成回调调用。
     */
    fun clearAnimation() {
        _hiddenRanges.update { emptyList() }
        _activeIntent.update { null }
        _drawsVisualCursor.update { false }
        _visualCursorSnapshot.update { null }
        _activeTransaction.update { null }
        _currentTextProgress.update { 0f }
        _currentCursorProgress.update { 0f }
        _currentRebaseProgress.update { 0f }
        pendingVisualIntent = null
    }

    /** 当前布局快照 — 供 overlay 读取 bounding box。 */
    fun currentLayout(): ComposeLayoutSnapshot? = currentSnapshot

    /** 上一份布局快照 — 删除文字动画用旧布局画。 */
    fun previousLayout(): ComposeLayoutSnapshot? = previousSnapshot
}
