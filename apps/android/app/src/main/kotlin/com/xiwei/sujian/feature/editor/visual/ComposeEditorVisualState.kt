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
 * - 收集 Core intent chain（双向汇合）
 * - 在真实屏幕 layout 到达且匹配时生成冻结的 [ComposeVisualTransaction]
 *
 * 本类只负责：
 * - 暴露 `_latestLayout` 供 overlay 读取
 * - 暴露 `_activeTransaction` 供 overlay 读取冻结事务
 * - 暴露 `_hiddenRanges` 供 OutputTransformation 读取
 * - 暴露 `_drawsVisualCursor` 控制系统光标显隐（仅由设置/attach 生命周期决定）
 * - 暴露单 master progress（[reportProgress]）供 coordinator 物化 startFrame
 *
 * smooth cursor 规则：
 * - smooth cursor 开启：编辑器 attach 以后系统光标一直透明，始终由 overlay 画
 * - smooth cursor 关闭：始终由系统画，overlay 永远不接管
 * 光标所有权只能由设置/attach 生命周期决定，不由某一笔事务是否带 cursor 动画决定。
 *
 * #641 评论1 第5节 / 问题3：overlay 只"画"，绝不能再改变 viewport / selection / IME 几何。
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
    }

    /** 帧协调器 — 核心状态机，管理 intent chain + 帧事务生成（双向汇合）。 */
    private val frameCoordinator = ComposeVisualFrameCoordinator(targetId)

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
     * 仅由设置/attach 生命周期决定，不在某笔事务到达时改写。
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
     * 单 master progress — overlay 报告当前动画进度（文字/光标/rebase 共用同一进度）。
     * 下一笔事务物化 startFrame 时由 coordinator 读取此真实进度，不再写死 1f。
     */
    private val _masterProgress = MutableStateFlow(0f)
    val masterProgress: StateFlow<Float> = _masterProgress.asStateFlow()

    /** 最后一次 onVisualIntent 传入的 motionPolicy — 供 onAuthoritativeLayout 使用。 */
    private var lastMotionPolicy: EditorMotionPolicy = EditorMotionPolicy()

    /**
     * Core 视觉意图到达 — 只把 intent 交给 frameCoordinator（附上当前 master progress），
     * 不启动动画、不改 layout。
     *
     * @param intent Core 视觉意图。
     * @param motionPolicy 动画策略。
     */
    fun onVisualIntent(
        intent: EditorVisualIntent,
        motionPolicy: EditorMotionPolicy,
    ) {
        lastMotionPolicy = motionPolicy
        val update = frameCoordinator.onVisualIntent(intent, _masterProgress.value)
        applyFrameUpdate(update)
    }

    /**
     * 系统给出权威布局 — 只记录，不修改输入几何。
     *
     * [BasicTextField] 的 `onTextLayout` 回调调用本方法，
     * 把系统最终 [TextLayoutResult] 记录为权威布局，不反向修改输入。
     *
     * #644 评论 #684：真正生成视觉事务发生在 onLayout —
     * 上一份真正显示过的 layout → 当前真正显示出来的 layout →
     * 中间积累的 Core intent chain → 一个冻结的 [ComposeVisualTransaction]（双向汇合）。
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

        // 让 frameCoordinator 生成冻结事务（附上当前 master progress 供 rebase 物化）。
        val effective = lastMotionPolicy.effective()
        val update = frameCoordinator.onLayout(snapshot, effective, _masterProgress.value)

        applyFrameUpdate(update)
    }

    /**
     * 把帧协调器的更新结果应用到本地状态 —
     * 不论事务是在 [onVisualIntent]（intent 先到、匹配 layout 已在）还是
     * [onAuthoritativeLayout]（layout 后到、匹配 pending 已在）时生成，
     * 都用同一套逻辑把冻结事务、hiddenRanges、activeIntent、cursor snapshot 暴露出去。
     */
    private fun applyFrameUpdate(update: FrameUpdate) {
        when (update) {
            is FrameUpdate.Empty -> {
                // 无新事务 — 首帧、无 pending、或 pending 与 layout 尚未匹配。
            }
            is FrameUpdate.NewTransaction -> {
                _activeTransaction.update { update.transaction }
                _hiddenRanges.update { update.hiddenRanges }

                // 从最后一个 intent 推导 activeIntent。
                val lastIntent = update.transaction.intents.lastOrNull()
                _activeIntent.update { lastIntent }

                // 为 cursor 动画创建 snapshot（仍由事务的 cursorStartRect/cursorEndRect 决定）。
                val cursorSnapshot = buildCursorSnapshot(update.transaction)
                _visualCursorSnapshot.update { cursorSnapshot }

                // 光标所有权只由设置/attach 决定（_drawsVisualCursor 不在此改写）。

                Log.d(
                    TAG,
                    "transaction_started: id=${update.transaction.id} " +
                        "coreTxnIds=${update.transaction.coreTransactionIds} " +
                        "drawsVisualCursor=${_drawsVisualCursor.value}",
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
     * overlay 报告当前动画 master progress — 物化 startFrame（下一笔 rebase）用。
     * 文字/光标/rebase 共用同一进度。
     */
    fun reportProgress(progress: Float) {
        _masterProgress.update { progress.coerceIn(0f, 1f) }
    }

    /**
     * 动画结束 — 通知 coordinator 清 active，再清本地 overlay 状态。
     * 由 overlay 的动画完成回调调用（progress 到达 1f）。
     */
    fun completeActiveTransaction(transactionId: Long) {
        frameCoordinator.completeTransaction(transactionId)
    }

    /**
     * 动画结束 — 清 hiddenRanges，系统正文马上可见。
     * 由 overlay 的动画完成回调调用。光标所有权（_drawsVisualCursor）不在此重置。
     */
    fun clearAnimation() {
        _hiddenRanges.update { emptyList() }
        _activeIntent.update { null }
        _visualCursorSnapshot.update { null }
        _activeTransaction.update { null }
        _masterProgress.update { 0f }
    }

    /**
     * 清除所有状态 — 章节切换或 detach 时调用。
     */
    fun clear() {
        frameCoordinator.clear()
        _latestLayout.update { null }
        _activeTransaction.update { null }
        _hiddenRanges.update { emptyList() }
        // 光标所有权只由设置/attach 决定，clear 不重置 _drawsVisualCursor。
        _activeIntent.update { null }
        _visualCursorSnapshot.update { null }
        _masterProgress.update { 0f }
    }

    /**
     * 设置 smooth cursor 状态 — 由外部设置变更驱动。
     * smooth cursor 开启时，编辑器 attach 以后系统光标一直透明。
     */
    fun setSmoothCursorEnabled(enabled: Boolean) {
        _drawsVisualCursor.update { enabled }
    }
}
