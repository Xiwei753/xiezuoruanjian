package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update

/**
 * #641 评论1 第4/5节：Core 返回的视觉意图 — 受影响的 UTF-16 range 和动画类型。
 * 从 Core display patch / VisualIntent 映射，offset 是 UTF-16（已由调用方从
 * UTF-8 byte 转换），不再用 byte 作为 Compose offset。
 *
 * #641 评论 问题2：cursor 拆成和文字动画并列的字段 —
 * [textKind] 描述文字动画类型（Insert/Delete/Move/None），
 * [cursor] 描述光标视觉意图。只要 [cursor] 的 [CursorVisualIntent.animate] 为 true，
 * 不管 [textKind] 是什么，都隐藏系统光标、创建 [VisualCursorSnapshot]、overlay 插值画光标。
 * `CURSOR_ONLY` 只是"没有文字动画"（[textKind] = None），
 * 不是"只有这种事务才允许画视觉光标"。
 *
 * #641 评论 5458283021 问题2a：两阶段 retained reflow —
 * [newTextLength] 用于 [ComposeEditorVisualState.onAuthoritativeLayout] 判断
 * 到达的 [TextLayoutResult] 是否对应这笔事务的 new text。
 * onVisualIntent 时若 currentSnapshot 已就绪且 text 长度匹配，立即创建 transaction；
 * 否则保存 pending，等 onAuthoritativeLayout 到达后再用确定的 old/new layout 生成
 * ComposeVisualTransaction 和 retained moves。
 *
 * #641 评论 5459531909 第1项：layout 关联不能再只看长度。
 * [newTextLength] 只能区分"长度不同"的事务，但 `i → W`、候选等长替换、自动纠错
 * 都可能长度相同而布局不同。新增 [expectedNewText] 保存完整新正文，
 * [ComposeEditorVisualState.canComputeRetainedNow] / [applyPendingRetainedMoves]
 * 改成比较 `result.layoutInput.text.text == expectedNewText` 才认这份 layout。
 * [newTextLength] 保留向后兼容（= expectedNewText.length），但不再作为唯一身份。
 *
 * @param transactionId 事务 ID — 由 [ComposeEditorVisualState.onVisualIntent] 内部分配，
 *   调用方可设为 0L。overlay 据此判断是否需要重新启动动画。
 * @param oldRanges 旧受影响 UTF-16 ranges — 删除动画用（来自 Core oldAffectedByteRanges）。
 * @param newRanges 新受影响 UTF-16 ranges — 插入/移动动画用（来自 Core newAffectedByteRanges）。
 * @param textKind 文字动画类型。
 * @param cursor 光标视觉意图 — null 表示不画视觉光标。
 * @param newTextLength 新正文 UTF-16 长度 — 保留向后兼容，由 [expectedNewText].length 推导。
 * @param expectedNewText #641 评论 5459531909 第1项：完整新正文（UTF-16 String）—
 *   layout 关联判断改用 `result.layoutInput.text.text == expectedNewText`，
 *   不再只比较长度。默认空字符串保持现有测试构造兼容。
 * @param replaceBounds #641 评论 5458880786 问题2a：明确的 replace 边界（UTF-16）—
 *   retained reflow 用它算 prefix/suffix，不再从空 oldRanges/newRanges 猜。
 *   null 表示未提供（向后兼容，fallback 到 oldRanges/newRanges 推断）。
 */
data class EditorVisualIntent(
    val transactionId: Long = 0L,
    val oldRanges: List<TextRange>,
    val newRanges: List<TextRange>,
    val textKind: TextVisualKind,
    val cursor: CursorVisualIntent?,
    val newTextLength: Int = 0,
    val expectedNewText: String = "",
    val replaceBounds: VisualReplaceBounds? = null,
)

/**
 * #641 评论 5458880786 问题2a：明确的 replace 边界（UTF-16）—
 * 供 [ComposeEditorVisualState.computeRetainedMoves] 算共同前缀/后缀。
 *
 * 一次 replace 把 oldText[oldStart..oldEnd) 替换成 newText[newStart..newEnd)，
 * 共同前缀 0..oldStart ↔ 0..newStart，共同后缀 oldEnd..oldText.length ↔ newEnd..newText.length。
 * retained reflow 用确定边界算 suffix 起点，不再从空 oldRanges/newRanges 猜（oldRanges 为空时
 * 旧实现 oldSuffixStart=0 错把整段当前缀）。
 *
 * @param oldStart 旧正文 replace 起点（UTF-16）。
 * @param oldEnd 旧正文 replace 终点（exclusive，UTF-16）。
 * @param newStart 新正文 replace 起点（UTF-16）。
 * @param newEnd 新正文 replace 终点（exclusive，UTF-16）。
 */
data class VisualReplaceBounds(
    val oldStart: Int,
    val oldEnd: Int,
    val newStart: Int,
    val newEnd: Int,
)

/**
 * #641 评论 问题2：文字动画类型 — 与光标动画并列，不再用单一 Kind 枚举。
 *
 * - [Insert]：插入文字 — overlay 从 current layout 淡入 newRanges。
 * - [Delete]：删除文字 — overlay 从 previous layout 淡出 oldRanges。
 * - [Move]：移动/替换文字 — overlay 从 previous layout 淡出 oldRanges，
 *   从 current layout 淡入 newRanges。
 * - [None]：没有文字动画（如 CURSOR_ONLY 事务）。
 */
enum class TextVisualKind { Insert, Delete, Move, None }

/**
 * #641 评论 问题2：光标视觉意图 — 与文字动画并列。
 *
 * 只要 [animate] 为 true，不管 [TextVisualKind] 是什么，
 * 都隐藏系统光标、创建 [VisualCursorSnapshot]、overlay 插值画光标。
 *
 * @param oldEndUtf16 旧光标位置（UTF-16 offset）。
 * @param newEndUtf16 新光标位置（UTF-16 offset）。
 * @param animate 是否动画光标 — 来自 Core [com.xiwei.sujian.feature.editor.projection.CoordinatedCursor.shouldAnimate]。
 */
data class CursorVisualIntent(
    val oldEndUtf16: Int,
    val newEndUtf16: Int,
    val animate: Boolean,
)

/**
 * #641 评论1 第5节：视觉光标插值快照 — 保存 old/new cursor rect 和 selection，
 * overlay 据此按 progress 插值绘制视觉光标。
 */
data class VisualCursorSnapshot(
    val oldCursorRect: Rect,
    val newCursorRect: Rect,
    val oldSelectionEnd: Int,
    val newSelectionEnd: Int,
)

/**
 * #641 评论1 第4/5节：Compose 显示层视觉状态 — 保存上一份和当前一份
 * [ComposeLayoutSnapshot]，根据 Core 的 [EditorVisualIntent] 算受影响 UTF-16 range。
 *
 * 动画层只"画"，绝不能再改变 viewport / selection / IME 几何。
 * [onAuthoritativeLayout] 由 [BasicTextField] 的 `onTextLayout` 回调调用，
 * 把系统最终 [TextLayoutResult] 记录为权威布局，不反向修改输入。
 *
 * #641 评论 问题3 + 评论 5457777142 问题2：transaction/rebase —
 * 新事务到来时如果旧事务还在跑，先用旧 transaction + 当前 progress 物化
 * [ComposeVisualFrame]，再把它作为新事务的 start_frame。
 * 不能 `progress=0` 生硬重开，也不能直接覆盖旧事务。
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
     * 由 overlay 在每帧绘制后调用 [reportProgress] 更新。
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
     * CURSOR_ONLY 打断文字动画时 textProgress snapTo(1f)，但 rebaseProgress 可能还在中途，
     * 此时 materializeStartFrame 不能仅凭 text/cursor >= 1f 就认为无视觉帧可冻结。
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
     * #641 评论1 第5节：onVisualIntent 时机 — commit 时 currentSnapshot 可能还是旧 layout。
     * 在新 layout 到达后，若当前活跃 intent 的 cursor 要动画，重新构建 cursor snapshot
     * （用新的 currentSnapshot 作为 newCursorRect，旧的 previousSnapshot 作为 oldCursorRect），
     * 避免 commit 时两份 rect 都是旧 layout。
     *
     * #641 评论 问题2：只要 cursor?.animate == true（不管 textKind），重新构建 cursor snapshot。
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
     * #641 评论 5458880786 问题2e：补算 retainedMoves 后同步更新 _hiddenRanges —
     * retained 文字在新位置仍可见，overlay 再画一份会重影。base owned newRanges 由当前
     * activeIntent 的 textKind 决定（Insert/Move 的 newRanges，Delete 为空），加上 retainedMoves 的 newRange。
     * 只在 hasTextAnimation 时才隐藏（reduceMotion 时不隐藏）。
     */
    private fun applyPendingRetainedMoves(result: TextLayoutResult) {
        val pending = pendingVisualIntent ?: return
        // #641 评论 5459531909 第1项：layout 关联改成正文一致才认这份 layout。
        // 旧实现只比较 newTextLength，`i → W`、候选等长替换、自动纠错都会长度相同
        // 但布局不同，导致把上一笔的 layout 错当成这笔的 new layout。
        if (result.layoutInput.text.text != pending.intent.expectedNewText) return
        pendingVisualIntent = null
        val retainedMoves = computeRetainedMoves(pending.intent)
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
            // active.startFrame 就是 onVisualIntent 时传入的 frozenStartFrame，
            // suppressedCurrentRanges 仍是上一事务 new text（= 本次 old text）坐标，需映射到本次 new text。
            val frozenSuppressed =
                active.startFrame?.suppressedCurrentRanges ?: emptyList()
            val mappedSuppressed =
                mapSuppressedRangesThroughReplace(frozenSuppressed, pending.intent.replaceBounds)
            val suppressedNotOverlapping =
                subtractRanges(mappedSuppressed, baseOwnedNewRanges + retainedNewRanges)
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
     * #641 评论 问题2：只要 [intent.cursor]?.animate == true，不管 [intent.textKind]
     * 是什么（Insert/Delete/Move/None），都隐藏系统光标、创建 [VisualCursorSnapshot]、
     * overlay 插值画光标。`CURSOR_ONLY` 只是"没有文字动画"（textKind = None），
     * 不是"只有这种事务才允许画视觉光标"。
     *
     * #641 评论 问题3 + 评论 5457777142 问题2：transaction/rebase —
     * 新事务到来时如果旧事务还在跑，先用旧 transaction + 当前 progress 物化
     * [ComposeVisualFrame]，再把它作为新事务的 start_frame。
     * 不能 `progress=0` 生硬重开，也不能直接覆盖旧事务。
     *
     * #641 评论 5457777142 问题3：Delete 不隐藏新正文 range。
     * [OutputTransformation] 作用的是**新正文**。删除 `abc` 中的 `a`，oldRange=0..1，
     * 新正文 `bc` 的 0..1 是 `b`，把 `b` 设透明是错的。Delete 的离场动画由 overlay
     * 从 previous [TextLayoutResult] 画旧字，不靠 hiddenRanges。
     * 需要隐藏的是当前正文里由 overlay 接管的范围：Insert/Move 的 newRanges、
     * 以及 retained move 的 newRange。Delete 导致后续保留文字位置变化时，
     * 靠 retained move 隐藏新位置并做 old→new 位移。
     *
     * #641 评论 5457777142 问题4：[motionPolicy] 直接放进 transaction，
     * overlay 据此决定 text/cursor 两条 timeline、reduceMotion、textEnabled、cursorEnabled。
     *
     * #641 评论 5458283021 问题2a：两阶段 retained reflow —
     * transaction 立即创建（overlay 能立即开始画文字动画、hiddenRanges 生效），
     * 只是 retainedMoves 初始为 emptyList，等 onAuthoritativeLayout 到达后
     * 用确定的 old/new layout 补算 retainedMoves。
     * 若 currentSnapshot 已就绪且 text 长度匹配 [EditorVisualIntent.newTextLength]，立即算 retainedMoves。
     *
     * #641 评论 5458283021 问题3c：把 policy 提前落实到视觉状态 —
     * 用 [EditorMotionPolicy.effective] 算 hasTextAnimation/hasCursorAnimation，
     * 只在真正有动画时设置 hiddenRanges/drawsVisualCursor。
     * reduceMotion=true / cursorEnabled=false 时不隐藏正文/光标，避免字或光标消失。
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

        // #641 评论 5458880786 问题1d：先冻结上一事务当前帧（用当前 cursorSnapshot / progress），
        // 再切 _activeIntent / 新 cursor snapshot。之前先切 _activeIntent 再 buildCursorSnapshot
        // 再 buildAndActivateTransaction(materializeStartFrame)，materializeStartFrame 读
        // _visualCursorSnapshot.value 时可能已拿到下一事务的 cursor snapshot，物化出错误起点。
        //
        // #641 评论 5460160958 问题2：frozenStartFrame 马上要交给本事务（C）绘制，
        // surviving targetRange 必须是 C 的 new text 坐标，因此用 incoming 的
        // intentWithId.replaceBounds 映射，而不是上一事务（B）自己的 replaceBounds。
        val runningTransaction = _activeTransaction.value
        val frozenStartFrame =
            materializeStartFrame(
                transaction = runningTransaction,
                textProgress = _currentTextProgress.value,
                cursorProgress = _currentCursorProgress.value,
                rebaseProgress = _currentRebaseProgress.value,
                nextReplaceBounds = intentWithId.replaceBounds,
            )

        _activeIntent.update { intentWithId }

        // #641 评论 5458283021 问题3c：把 policy 提前落实到视觉状态。
        // 不要"先隐藏，再让 overlay 决定不画"——reduceMotion 时 overlay 不画，
        // 但 BasicTextField 正文/系统光标已透明，会字或光标消失。
        val effective = motionPolicy.effective()
        val hasTextAnimation =
            effective.textEnabled && intentWithId.textKind != TextVisualKind.None
        val hasCursorAnimation = effective.cursorEnabled && intentWithId.cursor?.animate == true

        // #641 评论 5457777142 问题3 + 评论 5458283021 问题3c：hiddenRanges 修正。
        // Delete 不隐藏新正文 range（OutputTransformation 作用的是新正文，
        // 把新正文里 oldRange 对应位置设透明会错误隐藏现存文字）。
        // 需要隐藏的是当前正文里由 overlay 接管的范围：
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
        // transaction 立即创建（overlay 能立即开始画文字动画、hiddenRanges 生效），
        // 只是 retainedMoves 初始为 emptyList，等 onAuthoritativeLayout 到达后
        // 用确定的 old/new layout 补算 retainedMoves。
        // 若 currentSnapshot 已就绪且 text 长度匹配 newTextLength，立即算 retainedMoves。
        val currentSnapshotLocal = currentSnapshot
        // #641 评论 5459531909 第1项：layout 关联改成正文一致 —
        // 只看长度会被等长替换（i→W、候选、自动纠错）骗过，错把旧 layout 当新 layout。
        val canComputeRetainedNow =
            currentSnapshotLocal != null &&
                currentSnapshotLocal.result.layoutInput.text.text == intentWithId.expectedNewText
        val retainedMoves =
            if (canComputeRetainedNow) {
                computeRetainedMoves(intentWithId)
            } else {
                emptyList()
            }

        val currentRetainedNewRanges =
            retainedMoves.map { it.newRange }.filter { it.start < it.end }
        // #641 评论 5459531909 第2项：frozen startFrame 保留抑制范围。
        // 新事务 _hiddenRanges 不能直接覆盖成自己的 ranges，而应包含
        // currentOwnedNewRanges + currentRetainedNewRanges + frozenStartFrame.suppressedCurrentRanges。
        // suppressedCurrentRanges 先用 replaceBounds 映射到本次 new text，再过滤掉与新 ranges 重叠的，
        // 避免双重隐藏。这样快速输入 a→b 时 a 不会突然恢复 100% 再和 frozen frame 叠一层。
        val mappedSuppressed =
            mapSuppressedRangesThroughReplace(
                frozenStartFrame?.suppressedCurrentRanges ?: emptyList(),
                intentWithId.replaceBounds,
            )
        val suppressedNotOverlapping =
            subtractRanges(
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
     * #641 评论 5458283021 问题1a：rebase 不再慢一笔 —
     * 直接读 [_activeTransaction]（当前正在跑的事务）物化 startFrame，
     * 不再用 previousTransaction 滞后缓存。
     *
     * #641 评论 5458880786 问题1d：startFrame 由 [onVisualIntent] 提前物化传入，
     * 不再在本方法内部调 materializeStartFrame — 避免物化时读到下一事务的 cursor snapshot。
     * 创建 [ComposeVisualTransaction] 时带上 textKind = intent.textKind，
     * 供下一笔事务的 materializeStartFrame 物化 oldRanges/newRanges。
     */
    private fun buildAndActivateTransaction(
        intent: EditorVisualIntent,
        motionPolicy: EditorMotionPolicy,
        retainedMoves: List<RetainedMove>,
        startFrame: ComposeVisualFrame?,
    ) {
        val transaction =
            ComposeVisualTransaction(
                id = intent.transactionId,
                oldLayout = previousSnapshot,
                newLayout = currentSnapshot,
                oldRanges = intent.oldRanges,
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
     * #641 评论 5459754425 + 评论 5459896691：物化当前视觉帧作为新事务的 start_frame。
     *
     * 每次物化都把当前屏幕正在显示的所有内容 flatten 成一层新的扁平 [ComposeVisualFrame]，
     * 每个 [RebasedTextSlice] 携带自己的 sourceLayout。不再形成 startFrame 套 startFrame 的链。
     *
     * #641 评论 5458283021 问题1a：直接接收 [transaction]（当前正在跑的事务）。
     * #641 评论 5458283021 问题1c：分别接收 [textProgress] / [cursorProgress]。
     * #641 评论 5459896691 第1项：增加 [rebaseProgress] — 三条 timeline 都结束才算无视觉帧。
     *
     * #641 评论 5460160958 问题2：增加 [nextReplaceBounds] — frozenStartFrame 马上要交给本事务（C）
     *   绘制，surviving targetRange 必须是 C 的 new text 坐标，因此用 incoming 的 replaceBounds
     *   映射，而不是上一事务（B）自己的 replaceBounds（已删除 ComposeVisualTransaction.startFrameReplaceBounds）。
     * #641 评论 5460160958 问题3：flatten 旧 startFrame 前先按当前 [rebaseProgress] 物化每个 slice
     *   到"这一帧真实状态"，避免从 A 当初冻结的 sourceAlpha/sourceTranslate 重新起跑。
     * #641 评论 5460160958 问题4：targetRange 切成 prefix/suffix 时 sourceRange 成对切分，
     *   不再让两个新 slice 都拿整段 source bounds 当起点。
     *
     * 如果没有旧事务或 text/cursor/rebase 三条 progress 都已到 1f，返回 null。
     */
    private fun materializeStartFrame(
        transaction: ComposeVisualTransaction?,
        textProgress: Float,
        cursorProgress: Float,
        rebaseProgress: Float,
        nextReplaceBounds: VisualReplaceBounds?,
    ): ComposeVisualFrame? {
        val prev = transaction ?: return null
        // #641 评论 5459896691 第1项：三条当前实际存在的 timeline 都结束才算没有视觉帧。
        // CURSOR_ONLY 场景 textProgress=1/cursorProgress=1 但 rebaseProgress 可能还在中途。
        if (textProgress >= 1f && cursorProgress >= 1f && rebaseProgress >= 1f) return null

        val prevStartFrame = prev.startFrame
        // #641 评论 5460160958 问题3：先按当前 rebaseProgress 物化旧 startFrame slice 到"这一帧真实状态"。
        // 多代 flatten 不能直接用 A 当初冻结的 sourceAlpha/sourceTranslate 重新起跑——
        // 若 B 的 rebase 跑到 50%，A 的旧 slice 屏幕上的 alpha/位置已在 source 和 B target 中间，
        // C 到来时新 startFrame 应从 B 当前 50% 真实画面继续。
        val materializedOlder =
            prevStartFrame?.slices?.map { materializeRebasedSlice(it, prev.newLayout, rebaseProgress) } ?: emptyList()

        // currentSlices / retainedSlices 是当前事务按 textProgress 生成的，已是当前状态，不再按 rebaseProgress 物化。
        val currentSlices = collectCurrentSlicesAsRebased(prev, textProgress)
        val retainedSlices = collectRetainedMoveSlicesAsRebased(prev, textProgress)

        // #641 评论 5460160958 问题2+问题4：统一用 nextReplaceBounds 把所有 surviving targetRange
        // 从 prev new text 映射到这次 new text，成对切分 sourceRange + targetRange。
        val allSlices = materializedOlder + currentSlices + retainedSlices
        val mappedSlices =
            if (nextReplaceBounds == null) {
                allSlices
            } else {
                buildList {
                    for (slice in allSlices) {
                        if (slice.targetRange == null) {
                            add(slice)
                        } else {
                            addAll(splitRebasedSliceThroughReplace(slice, nextReplaceBounds))
                        }
                    }
                }
            }

        val cursorRect = materializeCursorRect(prev, cursorProgress)
        val cursorAlpha = if (prev.cursor?.animate == true) 1f else 0f

        return ComposeVisualFrame(
            slices = mappedSlices,
            cursorRect = cursorRect,
            cursorAlpha = cursorAlpha,
            suppressedCurrentRanges = _hiddenRanges.value,
        )
    }

    /**
     * #641 评论 5459896691 第2项 + 评论 5460070064 第3项：
     * 按 [prev.textKind] 物化当前屏幕仍可见的 slice 为 [RebasedTextSlice]。
     *
     * surviving slice（Insert/Move newRanges — 在当前 new text 里仍存在）：
     *   targetRange = range 自身，下一笔 flatten 时通过 replaceBounds 映射到新坐标。
     * fading slice（Delete oldRanges / Move oldRanges — 只属于旧画面）：
     *   targetRange = null，rebase 期间淡出。
     */
    private fun collectCurrentSlicesAsRebased(
        prev: ComposeVisualTransaction,
        textProgress: Float,
    ): List<RebasedTextSlice> =
        when (prev.textKind) {
            TextVisualKind.Delete ->
                // oldRanges 在当前 new text 里不存在 → fading
                rebasedSlices(prev.oldRanges, prev.oldLayout, 1f - textProgress, targetRange = null)
            TextVisualKind.Move ->
                // oldRanges → fading（旧位置消失）；newRanges → surviving（新位置仍在当前正文）
                rebasedSlices(prev.oldRanges, prev.oldLayout, 1f - textProgress, targetRange = null) +
                    survivingRebasedSlices(prev.newRanges, prev.newLayout, textProgress)
            TextVisualKind.Insert ->
                // newRanges 在当前 new text 里存在 → surviving
                survivingRebasedSlices(prev.newRanges, prev.newLayout, textProgress)
            TextVisualKind.None -> emptyList()
        }

    /**
     * 把 [ranges] 里有效段物化成 [RebasedTextSlice]，alpha = [alphaRaw].coerceIn(0,1)。
     * [layout] 为 null 时跳过（无法绘制）。
     * [targetRange] = null 表示只属于旧画面（rebase 期间淡出）。
     */
    private fun rebasedSlices(
        ranges: List<TextRange>,
        layout: ComposeLayoutSnapshot?,
        alphaRaw: Float,
        targetRange: TextRange?,
    ): List<RebasedTextSlice> {
        if (layout == null) return emptyList()
        val alpha = alphaRaw.coerceIn(0f, 1f)
        return ranges
            .filter { it.start < it.end && it.end <= layout.result.layoutInput.text.length }
            .map { range ->
                RebasedTextSlice(
                    sourceLayout = layout,
                    sourceRange = range,
                    sourceTranslate = Offset.Zero,
                    sourceAlpha = alpha,
                    targetRange = targetRange,
                )
            }
    }

    /**
     * #641 评论 5460070064 第3项：surviving slice — targetRange = range 自身（在当前正文里仍存在）。
     * 下一笔 flatten 时通过 replaceBounds 映射到新坐标，若被 replace 删除则变成 null（淡出）。
     */
    private fun survivingRebasedSlices(
        ranges: List<TextRange>,
        layout: ComposeLayoutSnapshot?,
        alphaRaw: Float,
    ): List<RebasedTextSlice> =
        rebasedSlices(ranges, layout, alphaRaw, targetRange = null).map { slice ->
            // targetRange = sourceRange（在当前 new text 坐标中同位置存活）
            slice.copy(targetRange = slice.sourceRange)
        }

    /**
     * 旧事务的 retainedMoves → [RebasedTextSlice]。
     * translate 按 [textProgress] 插值 old→new bounds，alpha=1。
     *
     * #641 评论 5459531909 第4项：translate = delta（相对 source layout 原位置的偏移）。
     * #641 评论 5460070064 第3项：retained 文字在当前 new text 里仍存在 → surviving。
     *   targetRange = move.newRange（在当前正文中的位置），下一笔 flatten 通过 replaceBounds 映射。
     */
    private fun collectRetainedMoveSlicesAsRebased(
        prev: ComposeVisualTransaction,
        textProgress: Float,
    ): List<RebasedTextSlice> {
        val prevLayout = prev.oldLayout
        val currLayout = prev.newLayout
        if (currLayout == null) return emptyList()
        val slices = mutableListOf<RebasedTextSlice>()
        for (move in prev.retainedMoves) {
            val oldBounds = prevLayout?.let { safePathBounds(it.result, move.oldRange) }
            val newBounds = safePathBounds(currLayout.result, move.newRange)
            if (oldBounds == null || newBounds == null) continue
            val currentX = lerpFloat(oldBounds.left, newBounds.left, textProgress)
            val currentY = lerpFloat(oldBounds.top, newBounds.top, textProgress)
            val translate =
                Offset(
                    currentX - newBounds.left,
                    currentY - newBounds.top,
                )
            slices.add(
                RebasedTextSlice(
                    sourceLayout = currLayout,
                    sourceRange = move.newRange,
                    sourceTranslate = translate,
                    sourceAlpha = 1f,
                    // surviving: 在当前正文里仍存在
                    targetRange = move.newRange,
                ),
            )
        }
        return slices
    }

    /**
     * #641 评论 5460160958 问题3：按当前 [rebaseProgress] 物化单个 [RebasedTextSlice] 到"这一帧真实状态"。
     *
     * 多代 flatten 不能直接用 A 当初冻结的 sourceAlpha/sourceTranslate 重新起跑——
     * 若 B 的 rebase 跑到 50%，A 的旧 slice 屏幕上的 alpha/位置已在 source 和 B target 中间，
     * C 到来时新 startFrame 应从 B 当前 50% 真实画面继续。
     *
     * - [slice.targetRange] == null（fading）：alpha = lerp(sourceAlpha, 0f, rebaseProgress)，
     *   其余保持不变（继续从原位置淡出）。
     * - [slice.targetRange] != null（surviving）：alpha = lerp(sourceAlpha, 1f, rebaseProgress)，
     *   位置从 source bounds 插值到 target bounds，重新锚定到 [currentLayout]。
     *   [currentLayout] 为 null 或 bounds 无效时只更新 alpha。
     *
     * @param currentLayout 上一事务（B）的 newLayout — surviving slice 的 target 在这份 layout 里。
     */
    private fun materializeRebasedSlice(
        slice: RebasedTextSlice,
        currentLayout: ComposeLayoutSnapshot?,
        rebaseProgress: Float,
    ): RebasedTextSlice {
        val sourceAlpha = slice.sourceAlpha
        val targetRange = slice.targetRange
        if (targetRange == null) {
            // fading：从原位置继续淡出，alpha 向 0 收敛。
            val currentAlpha = lerpFloat(sourceAlpha, 0f, rebaseProgress)
            return slice.copy(sourceAlpha = currentAlpha)
        }
        // surviving：alpha 向 1 收敛。
        val currentAlpha = lerpFloat(sourceAlpha, 1f, rebaseProgress)
        if (currentLayout == null) {
            return slice.copy(sourceAlpha = currentAlpha)
        }
        val sourceBounds = safePathBounds(slice.sourceLayout.result, slice.sourceRange)
        val targetBounds = safePathBounds(currentLayout.result, targetRange)
        if (sourceBounds == null || targetBounds == null) {
            return slice.copy(sourceAlpha = currentAlpha)
        }
        val currentX = lerpFloat(sourceBounds.left, targetBounds.left, rebaseProgress)
        val currentY = lerpFloat(sourceBounds.top, targetBounds.top, rebaseProgress)
        // 重新锚定到 currentLayout：sourceLayout=currentLayout、sourceRange=targetRange，
        // sourceTranslate = currentPos - targetBounds 原点
        // （绘制时 targetBounds.left/top + translate = currentPos）。
        return RebasedTextSlice(
            sourceLayout = currentLayout,
            sourceRange = targetRange,
            sourceTranslate = Offset(currentX - targetBounds.left, currentY - targetBounds.top),
            sourceAlpha = currentAlpha,
            targetRange = targetRange,
        )
    }

    /**
     * #641 评论 5460160958 问题4：surviving slice 通过下一事务 replace 边界切分时，
     * sourceRange 和 targetRange 成对切分，不再让 prefix/suffix 都拿整段 source bounds 当起点。
     *
     * 前提：surviving slice 表示同一逻辑文本，sourceRange 长度应等于 oldTarget 长度。
     * 若长度不等（不应发生），不静默复制整段——结束该 surviving 映射，按旧画面离场处理：
     * 返回 listOf(slice.copy(targetRange = null))。
     *
     * - prefix 部分（target 在 [0, b.oldStart) 里，位置不变）：
     *   newTarget = [oldTarget.start, prefixEnd)，newSource = [sourceRange.start, sourceRange.start + len)。
     * - suffix 部分（target 在 [b.oldEnd, ...) 里，平移 delta）：
     *   newTarget = [suffixStart + delta, oldTarget.end + delta)，
     *   newSource = [sourceRange.end - len, sourceRange.end)。
     * - 跨越 replace 区域的部分不存活，丢弃。
     */
    private fun splitRebasedSliceThroughReplace(
        slice: RebasedTextSlice,
        b: VisualReplaceBounds,
    ): List<RebasedTextSlice> {
        val oldTarget = slice.targetRange ?: return listOf(slice)
        val sourceRange = slice.sourceRange
        // 前提：surviving slice 表示同一逻辑文本，sourceRange 长度应等于 oldTarget 长度。
        // 若不等，不要静默复制整段——结束该 surviving 映射，按旧画面离场处理。
        if ((sourceRange.end - sourceRange.start) != (oldTarget.end - oldTarget.start)) {
            return listOf(slice.copy(targetRange = null))
        }
        return buildList {
            // prefix 部分（target 在 [0, b.oldStart) 里，位置不变）
            val prefixEnd = minOf(oldTarget.end, b.oldStart)
            if (oldTarget.start < prefixEnd) {
                val len = prefixEnd - oldTarget.start
                val newTarget = TextRange(oldTarget.start, prefixEnd)
                val newSource = TextRange(sourceRange.start, sourceRange.start + len)
                add(slice.copy(sourceRange = newSource, targetRange = newTarget))
            }
            // suffix 部分（target 在 [b.oldEnd, ...) 里，平移 delta）
            val suffixStart = maxOf(oldTarget.start, b.oldEnd)
            if (suffixStart < oldTarget.end) {
                val delta = b.newEnd - b.oldEnd
                val newTarget = TextRange(suffixStart + delta, oldTarget.end + delta)
                val len = oldTarget.end - suffixStart
                val newSource = TextRange(sourceRange.end - len, sourceRange.end)
                add(slice.copy(sourceRange = newSource, targetRange = newTarget))
            }
            // 跨越 replace 区域的部分不存活，丢弃。
        }
    }

    /**
     * cursor rect：按 [cursorProgress] 插值 old→new。无 cursor 动画时返回 null。
     */
    private fun materializeCursorRect(
        prev: ComposeVisualTransaction,
        cursorProgress: Float,
    ): Rect? {
        val cursorSnapshot = _visualCursorSnapshot.value
        if (cursorSnapshot == null || prev.cursor?.animate != true) return null
        val left =
            lerpFloat(
                cursorSnapshot.oldCursorRect.left,
                cursorSnapshot.newCursorRect.left,
                cursorProgress,
            )
        val top =
            lerpFloat(
                cursorSnapshot.oldCursorRect.top,
                cursorSnapshot.newCursorRect.top,
                cursorProgress,
            )
        val right =
            lerpFloat(
                cursorSnapshot.oldCursorRect.right,
                cursorSnapshot.newCursorRect.right,
                cursorProgress,
            )
        val bottom =
            lerpFloat(
                cursorSnapshot.oldCursorRect.bottom,
                cursorSnapshot.newCursorRect.bottom,
                cursorProgress,
            )
        return Rect(left, top, right, bottom)
    }

    /**
     * #641 评论 5459531909 第2项：把上一事务的 suppressedCurrentRanges（在上一事务 new text
     * = 本次 old text 坐标）映射到本次 new text 坐标。
     *
     * 用 [replaceBounds] 的共同前缀/后缀映射：
     * - range 完全在共同前缀 [0, oldStart) 里：位置不变。
     * - range 完全在共同后缀 [oldEnd, ...) 里：newPos = oldPos - oldEnd + newEnd。
     * - range 跨越 replace 区域 [oldStart, oldEnd]：**区间切分** —
     *   把 prefix 部分（[0, oldStart)）保留，suffix 部分（[oldEnd, ...)）映射后保留。
     *   不要整段丢弃。
     *
     * [replaceBounds] 为 null 时不映射（返回空列表，避免错误映射，向后兼容）。
     *
     * #641 评论 5459754425 第2项：改成区间切分，不要整段判断。
     * 例如上一笔 suppressed 是 [0, 5)，下一笔在 [2, 3) 替换。
     * 真正还活着的是：[0, 2) 和原 [3, 5) 映射后的 suffix。
     * 旧实现会把整段 [0,5) 丢掉，导致仍在 startFrame 里画的 surviving 文字会同时从系统正文恢复出来。
     */
    private fun mapSuppressedRangesThroughReplace(
        ranges: List<TextRange>,
        replaceBounds: VisualReplaceBounds?,
    ): List<TextRange> {
        if (replaceBounds == null) return emptyList()
        val delta = replaceBounds.newEnd - replaceBounds.oldEnd
        val result = mutableListOf<TextRange>()
        for (range in ranges) {
            if (range.start >= range.end) continue
            // prefix 部分：完全在共同前缀 [0, oldStart) 里 — 位置不变
            val prefixStart = range.start
            val prefixEnd = minOf(range.end, replaceBounds.oldStart)
            if (prefixStart < prefixEnd) {
                result.add(TextRange(prefixStart, prefixEnd))
            }
            // suffix 部分：完全在共同后缀 [oldEnd, ...) 里 — 平移
            val suffixStart = maxOf(range.start, replaceBounds.oldEnd)
            val suffixEnd = range.end
            if (suffixStart < suffixEnd) {
                result.add(
                    TextRange(
                        suffixStart + delta,
                        suffixEnd + delta,
                    ),
                )
            }
            // 跨越 replace 区域的部分（prefixEnd .. suffixStart）不存活，丢弃
        }
        return result
    }

    /**
     * #641 评论 5459531909 第2项：从 [candidates] 中减去 [blockers] 覆盖的部分，
     * 避免双重隐藏（同一段文字既被新事务 ranges 隐藏，又被 frozen suppressed ranges 隐藏）。
     *
     * 不要整段丢弃 — 改成真正的区间 subtraction：
     * 例如 candidate [0, 5)，blocker [4, 6)，结果应该留下 [0, 4)，不能整段消失。
     *
     * #641 评论 5459754425 第2项：不要 filter whole range，改成真正的区间 subtraction。
     * 比如 candidate [0,5)，blocker [4,6)，结果应该留下 [0,4)，不能整段消失。
     * 这样 hidden ownership 才和实际 surviving slice 一一对应。
     */
    private fun subtractRanges(
        candidates: List<TextRange>,
        blockers: List<TextRange>,
    ): List<TextRange> {
        if (candidates.isEmpty() || blockers.isEmpty()) return candidates
        val result = mutableListOf<TextRange>()
        for (candidate in candidates) {
            if (candidate.start >= candidate.end) continue
            // 收集与 candidate 相关的 blockers，按 start 排序
            val relevantBlockers =
                blockers
                    .filter { it.start < candidate.end && it.end > candidate.start }
                    .sortedBy { it.start }
            if (relevantBlockers.isEmpty()) {
                result.add(candidate)
                continue
            }
            // 从 candidate 中逐段减去 blockers
            var currentStart = candidate.start
            for (blocker in relevantBlockers) {
                // blocker 前面的非重叠部分
                if (blocker.start > currentStart) {
                    result.add(TextRange(currentStart, minOf(blocker.start, candidate.end)))
                }
                // 跳过 blocker 覆盖的部分
                currentStart = maxOf(currentStart, blocker.end)
                if (currentStart >= candidate.end) break
            }
            // 最后一个 blocker 后面的非重叠部分
            if (currentStart < candidate.end) {
                result.add(TextRange(currentStart, candidate.end))
            }
        }
        return result
    }

    /** 线性插值 helper。 */
    private fun lerpFloat(
        a: Float,
        b: Float,
        t: Float,
    ): Float = a + (b - a) * t.coerceIn(0f, 1f)

    /**
     * 从当前/上一份 [TextLayoutResult] 取真实 cursor rect 构建插值快照。
     * old cursor 来自 previous result + previous selection end；
     * new cursor 来自 current result + current selection end。
     * 任一 layout 缺失时不构建快照（overlay 只画已有的一侧）。
     *
     * #641 评论 问题2：old/new selection end 从 [CursorVisualIntent] 读取。
     */
    private fun buildCursorSnapshot(): VisualCursorSnapshot? {
        val prev = previousSnapshot ?: return null
        val curr = currentSnapshot ?: return null
        val intent = _activeIntent.value
        val cursor = intent?.cursor
        val oldSelectionEnd = cursor?.oldEndUtf16 ?: prev.selection.end
        val newSelectionEnd = cursor?.newEndUtf16 ?: curr.selection.end
        val oldCursorRect = prev.result.getCursorRect(oldSelectionEnd)
        val newCursorRect = curr.result.getCursorRect(newSelectionEnd)
        return VisualCursorSnapshot(
            oldCursorRect = oldCursorRect,
            newCursorRect = newCursorRect,
            oldSelectionEnd = oldSelectionEnd,
            newSelectionEnd = newSelectionEnd,
        )
    }

    /**
     * #641 评论 问题3 + 评论 5457777142 问题2 + 评论 5458283021 问题2b：retained move 计算 —
     * 自动折行/手动换行的 retained move 用 old/new [TextLayoutResult]
     * 比较同一逻辑文本范围的位置变化生成。
     *
     * 真实现：根据一次 replace 的 old/new 边界建立 retained text 映射。
     * 共同前缀 offset 不变；共同后缀按 `deltaUtf16 = newInsertedLength - oldRemovedLength`
     * 映射。只检查受影响视觉行附近的 retained range，用前后两份 [TextLayoutResult]
     * 比较 line/Path bounds；坐标变化的连续片段合并成 [RetainedMove]。
     *
     * #641 评论 5458283021 问题2b：按 old/new TextLayoutResult 的视觉行切片，
     * 再把位移向量一致的连续 slice 合并；切点走 code-point 边界，不重新把 surrogate pair 切开。
     * 不再固定每 64 个 UTF-16 char 切一段（64-char range 可能跨多行，前后半段位移不一致）。
     *
     * 位置只从 [TextLayoutResult] 读，动画只负责画。
     * 如果 oldLayout 或 newLayout 缺失，返回空列表。
     */
    @Suppress("CyclomaticComplexMethod", "CognitiveComplexMethod", "NestedBlockDepth")
    private fun computeRetainedMoves(intent: EditorVisualIntent): List<RetainedMove> {
        if (intent.textKind == TextVisualKind.None) return emptyList()
        val prev = previousSnapshot ?: return emptyList()
        val curr = currentSnapshot ?: return emptyList()

        // 用 old/new affected ranges 推断 replace 边界。
        // 共同前缀 = oldRanges.start 之前；共同后缀 = oldRanges.end 之后。
        // #641 评论 5458880786 问题2c：优先用 intent.replaceBounds（明确边界）—
        // 旧实现用 oldRanges.maxOfOrNull 推断，oldRanges 为空时 oldSuffixStart=0 错把整段当前缀。
        // 有 replaceBounds 时用确定边界，没有时才 fallback（向后兼容测试里没传 replaceBounds 的情况）。
        val replaceBounds = intent.replaceBounds
        val oldSuffixStart = replaceBounds?.oldEnd ?: (intent.oldRanges.maxOfOrNull { it.end } ?: 0)
        val newSuffixStart = replaceBounds?.newEnd ?: (intent.newRanges.maxOfOrNull { it.end } ?: 0)

        val oldText = prev.result.layoutInput.text
        val newText = curr.result.layoutInput.text
        val oldTextLen = oldText.length
        val newTextLen = newText.length

        val result = mutableListOf<RetainedMove>()
        if (oldSuffixStart >= oldTextLen || newSuffixStart >= newTextLen) return result

        // #641 评论 5458283021 问题2b：按 old layout 的视觉行切片。
        // 共同后缀字符一一对应：newPos = oldPos - oldSuffixStart + newSuffixStart。
        // 切点走 old layout 的行边界（已是 code-point 边界，Compose 不会把 surrogate pair 拆行）。
        // 再把位移向量一致的连续 slice 合并。
        var oldPos = oldSuffixStart
        var mergedOldStart = -1
        var mergedNewStart = -1
        var mergedDx = 0f
        var mergedDy = 0f
        var merging = false
        while (oldPos < oldTextLen) {
            val oldLine = prev.result.getLineForOffset(oldPos)
            val oldLineEnd = prev.result.getLineEnd(oldLine)
            // 段结束 = min(oldLineEnd, oldTextLen)，确保是 code-point 边界。
            // #641 评论 5458880786 问题2d：只判断"边界左边 high、右边 low" —
            // 旧实现用 codePointBefore + charCount==2 判断，codePointBefore 返回 supplementary、
            // charCount==2 说明 segEnd 在 surrogate pair 后面，本来合法，减 1 反而移进中间。
            // 只有 segEnd-1 是 high surrogate、segEnd 是 low surrogate 时才需要回退到 high 前。
            var segEnd = minOf(oldLineEnd, oldTextLen)
            if (segEnd in 1 until oldTextLen &&
                oldText[segEnd - 1].isHighSurrogate() &&
                oldText[segEnd].isLowSurrogate()
            ) {
                segEnd -= 1
            }
            if (segEnd <= oldPos) segEnd = oldPos + 1

            val newPos = oldPos - oldSuffixStart + newSuffixStart
            val newSegEnd = segEnd - oldSuffixStart + newSuffixStart
            if (newSegEnd > newTextLen) break

            val oldRange = TextRange(oldPos, segEnd)
            val newRange = TextRange(newPos, newSegEnd)
            val oldBounds = safePathBounds(prev.result, oldRange)
            val newBounds = safePathBounds(curr.result, newRange)

            if (oldBounds != null && newBounds != null) {
                val dx = newBounds.left - oldBounds.left
                val dy = newBounds.top - oldBounds.top
                val topChanged = kotlin.math.abs(dy) > 1f
                val leftChanged = kotlin.math.abs(dx) > 1f
                if (topChanged || leftChanged) {
                    // 位移变化的段：尝试与上一段合并（位移向量一致）。
                    if (merging && kotlin.math.abs(dx - mergedDx) <= 1f && kotlin.math.abs(dy - mergedDy) <= 1f) {
                        // 位移向量一致，继续合并（不更新 mergedDx/Dy，保留首段向量）。
                    } else {
                        // 位移向量不一致或首次：先 flush 之前的合并段，再开始新合并。
                        if (merging) {
                            result.add(
                                RetainedMove(
                                    oldRange = TextRange(mergedOldStart, oldPos),
                                    newRange = TextRange(mergedNewStart, newPos),
                                ),
                            )
                        }
                        mergedOldStart = oldPos
                        mergedNewStart = newPos
                        mergedDx = dx
                        mergedDy = dy
                        merging = true
                    }
                } else {
                    // #641 评论 5459531909 第4项：稳定段（!topChanged && !leftChanged）—
                    // 先 flush 当前合并段，再 merging=false，
                    // 不让后面的稳定文字也被并进前一个移动段。
                    if (merging) {
                        result.add(
                            RetainedMove(
                                oldRange = TextRange(mergedOldStart, oldPos),
                                newRange = TextRange(mergedNewStart, newPos),
                            ),
                        )
                        merging = false
                    }
                }
            } else {
                // #641 评论 5459531909 第4项：old/new bounds 无效 —
                // 先 flush 当前合并段，再 merging=false，
                // 不让无效段打断已有合并段的连续性，也不让无效段被并进合并段。
                if (merging) {
                    result.add(
                        RetainedMove(
                            oldRange = TextRange(mergedOldStart, oldPos),
                            newRange = TextRange(mergedNewStart, newPos),
                        ),
                    )
                    merging = false
                }
            }
            oldPos = segEnd
        }
        // flush 最后一个合并段。
        if (merging) {
            val newPos = oldPos - oldSuffixStart + newSuffixStart
            if (newPos <= newTextLen) {
                result.add(
                    RetainedMove(
                        oldRange = TextRange(mergedOldStart, oldPos),
                        newRange = TextRange(mergedNewStart, newPos),
                    ),
                )
            }
        }
        return result
    }

    /** 安全获取 path bounds — range 无效或越界时返回 null。 */
    private fun safePathBounds(
        result: TextLayoutResult,
        range: TextRange,
    ): Rect? {
        if (range.start >= range.end) return null
        if (range.end > result.layoutInput.text.length) return null
        return try {
            result.getPathForRange(range.start, range.end).getBounds()
        } catch (_: Throwable) {
            null
        }
    }

    /**
     * #641 评论 5457777142 问题2 + 评论 5458283021 问题1c：overlay 报告当前动画 progress —
     * 新事务到来时用它物化 [ComposeVisualFrame]。
     *
     * #641 评论 5458283021 问题1c：分别报告 textProgress / cursorProgress，
     * coordinated=false 时物化 cursor 用 cursorProgress 不再错算。
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
