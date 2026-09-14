package com.xiwei.sujian.feature.editor.visual

import android.util.Log
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update

/**
 * #641 评论1 第4/5节：Compose 显示层视觉状态 — 保存当前一份 [ComposeLayoutSnapshot]，
 * 根据 Core 的 [EditorVisualIntent] 算受影响 UTF-16 range。
 *
 * #644 评论 #684：删掉旧的"双快照 + 单 pending"状态机。
 * 旧字段 `previousSnapshot`、`currentSnapshot` 作为事务 old/new 配对来源、
 * `PendingVisualIntent`、`pendingVisualIntent`、`applyPendingRetainedMoves()`、
 * `tryActivateVisualCursor()`、`currentLayout()`、`previousLayout()` 全部删除。
 *
 * 改为持有 [ComposeVisualFrameCoordinator]，由它负责：
 * - 收集 Core intent chain
 * - 在真实屏幕 layout 到达时生成冻结的 [ComposeVisualTransaction]
 *
 * 本类只负责：
 * - 暴露 `_latestLayout` 供 overlay 读取
 * - 暴露 `_activeTransaction` 供 overlay 读取冻结事务
 * - 暴露 `_hiddenRanges` 供 OutputTransformation 读取
 * - 暴露 `_drawsVisualCursor` 控制系统光标显隐
 *
 * smooth cursor 规则：
 * - smooth cursor 开启：编辑器 attach 以后系统光标一直透明，始终由 overlay 画
 * - smooth cursor 关闭：始终由系统画，overlay 永远不接管
 *
 * #641 评论1 第5节 / 问题3：overlay 只"画"，绝不能再改变 viewport / selection / IME 几何。
 *
 * @param initialDrawsVisualCursor 初始视觉光标状态 — smooth cursor 开启时从 attach 后一直为 true。
 */
class ComposeEditorVisualState(
    initialDrawsVisualCursor: Boolean = false,
) {
    companion object {
        private const val TAG = "EditorVisualState"
    }

    /** 帧协调器 — 核心状态机，管理 intent chain + 帧事务生成。 */
    private val frameCoordinator = ComposeVisualFrameCoordinator()

    /** 最新 layout 快照 — 供 overlay 读取 bounding box。 */
    private val _latestLayout = MutableStateFlow<ComposeLayoutSnapshot?>(null)
    val latestLayout: StateFlow<ComposeLayoutSnapshot?> = _latestLayout.asStateFlow()

    /** 当前活跃的视觉动画事务（冻结的）— overlay 只读此事务。 */
    private val _activeTransaction = MutableStateFlow<ComposeVisualTransaction?>(null)
    val activeTransaction: StateFlow<ComposeVisualTransaction?> = _activeTransaction.asStateFlow()

    /**
     * 当前正在动画的 UTF-16 range — 这些 range 在 [OutputTransformation] 里被设为透明，
     * overlay 补画动画过程；动画完成立即从该列表删除，系统正文已在最终位置。
     */
    private val _hiddenRanges = MutableStateFlow<List<TextRange>>(emptyList())
    val hiddenRanges: StateFlow<List<TextRange>> = _hiddenRanges.asStateFlow()

    /**
     * 视觉光标是否由 overlay 绘制 —
     * smooth cursor 开启：编辑器 attach 以后一直为 true（系统光标透明）。
     * smooth cursor 关闭：一直为 false（系统光标正常画）。
     */
    private val _drawsVisualCursor = MutableStateFlow(initialDrawsVisualCursor)
    val drawsVisualCursor: StateFlow<Boolean> = _drawsVisualCursor.asStateFlow()

    /**
     * 当前活跃的视觉意图 — 供 overlay 读取动画类型。
     * 从活跃事务的最后一个 intent 推导。
     */
    private val _activeIntent = MutableStateFlow<EditorVisualIntent?>(null)
    val activeIntent: StateFlow<EditorVisualIntent?> = _activeIntent.asStateFlow()

    /**
     * 视觉光标插值快照 — 供 overlay 按 progress 插值绘制。
     */
    private val _visualCursorSnapshot = MutableStateFlow<VisualCursorSnapshot?>(null)
    val visualCursorSnapshot: StateFlow<VisualCursorSnapshot?> = _visualCursorSnapshot.asStateFlow()

    /**
     * overlay 报告的当前动画 progress — 物化 startFrame 用。
     */
    private val _currentTextProgress = MutableStateFlow(0f)
    val currentTextProgress: StateFlow<Float> = _currentTextProgress.asStateFlow()

    private val _currentCursorProgress = MutableStateFlow(0f)
    val currentCursorProgress: StateFlow<Float> = _currentCursorProgress.asStateFlow()

    private val _currentRebaseProgress = MutableStateFlow(0f)
    val currentRebaseProgress: StateFlow<Float> = _currentRebaseProgress.asStateFlow()

    /** 最后一次 onVisualIntent 传入的 motionPolicy — 供 onAuthoritativeLayout 使用。 */
    private var lastMotionPolicy: EditorMotionPolicy = EditorMotionPolicy()

    /**
     * Core 视觉意图到达 — 只把 intent 交给 frameCoordinator，不启动动画、不改 layout。
     *
     * @param intent Core 视觉意图。
     * @param motionPolicy 动画策略。
     */
    fun onVisualIntent(
        intent: EditorVisualIntent,
        motionPolicy: EditorMotionPolicy,
    ) {
        lastMotionPolicy = motionPolicy
        frameCoordinator.onVisualIntent(intent)
        Log.d(TAG, "intent_queued: coreTxn=${intent.coreTransactionId} kind=${intent.textKind}")
    }

    /**
     * 系统给出权威布局 — 只记录，不修改输入几何。
     *
     * [BasicTextField] 的 `onTextLayout` 回调调用本方法，
     * 把系统最终 [TextLayoutResult] 记录为权威布局，不反向修改输入。
     *
     * #644 评论 #684：真正生成视觉事务发生在 onLayout —
     * 上一份真正显示过的 layout → 当前真正显示出来的 layout →
     * 中间积累的 Core intent chain → 一个冻结的 [ComposeVisualTransaction]。
     *
     * 新事务生成后，里面的 oldLayout/newLayout/retainedMoves/cursorStartRect/cursorEndRect/startFrame
     * 全部不可再被后续 `onTextLayout` 修改。
     */
    fun onAuthoritativeLayout(
        result: TextLayoutResult,
        selection: TextRange,
        scrollY: Int,
    ) {
        val snapshot = ComposeLayoutSnapshot(result, selection, scrollY)
        _latestLayout.update { snapshot }

        // 让 frameCoordinator 生成冻结事务。
        val effective = lastMotionPolicy.effective()
        val update = frameCoordinator.onLayout(snapshot, effective)

        when (update) {
            is FrameUpdate.Empty -> {
                // 无新事务 — 首帧或无 pending intent。
            }
            is FrameUpdate.NewTransaction -> {
                _activeTransaction.update { update.transaction }
                _hiddenRanges.update { update.hiddenRanges }

                // 从最后一个 intent 推导 activeIntent。
                val lastIntent = update.transaction.intents.lastOrNull()
                _activeIntent.update { lastIntent }

                // 为 cursor 动画创建 snapshot。
                val cursorSnapshot = buildCursorSnapshot(update.transaction)
                if (cursorSnapshot != null) {
                    _visualCursorSnapshot.update { cursorSnapshot }
                } else {
                    _visualCursorSnapshot.update { null }
                }

                // smooth cursor 规则：只要事务有 cursor 动画就启用视觉光标。
                val hasCursorAnimation =
                    effective.cursorEnabled && lastIntent?.cursor?.animate == true
                if (hasCursorAnimation) {
                    _drawsVisualCursor.update { true }
                }
                // smooth cursor 关闭时保持 _drawsVisualCursor = false（初始值）。

                Log.d(
                    TAG,
                    "transaction_started: id=${update.transaction.id} " +
                        "coreTxnIds=${update.transaction.coreTransactionIds}",
                )
            }
        }
    }

    /**
     * 从冻结事务构建 cursor snapshot — 用事务的 cursorStartRect/cursorEndRect。
     */
    private fun buildCursorSnapshot(transaction: ComposeVisualTransaction): VisualCursorSnapshot? {
        val startRect = transaction.cursorStartRect ?: return null
        val endRect = transaction.cursorEndRect ?: return null
        val lastIntent = transaction.intents.lastOrNull() ?: return null
        val cursor = lastIntent.cursor ?: return null
        return VisualCursorSnapshot(
            oldCursorRect = startRect,
            newCursorRect = endRect,
            oldSelectionEnd = cursor.oldEndUtf16,
            newSelectionEnd = cursor.newEndUtf16,
        )
    }

    /**
     * overlay 报告当前动画 progress — 物化 startFrame 用。
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
     * 动画结束 — 清 hiddenRanges，系统正文马上可见。
     * 由 overlay 的动画完成回调调用。
     */
    fun clearAnimation() {
        _hiddenRanges.update { emptyList() }
        _activeIntent.update { null }
        _visualCursorSnapshot.update { null }
        _activeTransaction.update { null }
        _currentTextProgress.update { 0f }
        _currentCursorProgress.update { 0f }
        _currentRebaseProgress.update { 0f }
        // smooth cursor 关闭时 _drawsVisualCursor 保持 false。
        // smooth cursor 开启时动画结束后仍保持 true（系统光标一直透明）。
    }

    /**
     * 清除所有状态 — 章节切换或 detach 时调用。
     */
    fun clear() {
        frameCoordinator.clear()
        _latestLayout.update { null }
        _activeTransaction.update { null }
        _hiddenRanges.update { emptyList() }
        _drawsVisualCursor.update { false }
        _activeIntent.update { null }
        _visualCursorSnapshot.update { null }
        _currentTextProgress.update { 0f }
        _currentCursorProgress.update { 0f }
        _currentRebaseProgress.update { 0f }
    }

    /**
     * 设置 smooth cursor 状态 — 由外部设置变更驱动。
     * smooth cursor 开启时，编辑器 attach 以后系统光标一直透明。
     */
    fun setSmoothCursorEnabled(enabled: Boolean) {
        _drawsVisualCursor.update { enabled }
    }
}
