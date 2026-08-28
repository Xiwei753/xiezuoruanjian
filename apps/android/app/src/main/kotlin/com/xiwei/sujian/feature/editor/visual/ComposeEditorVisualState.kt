package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update

/**
 * #641 评论1 第4/5节：Core 返回的视觉意图 — 受影响的 UTF-16 range 和动画类型。
 * 从 Core display patch / VisualIntent 映射，offset 是 UTF-16（已由调用方从
 * UTF-8 byte 转换），不再用 byte 作为 Compose offset。
 */
data class EditorVisualIntent(
    val affectedRanges: List<TextRange>,
    val kind: Kind,
) {
    enum class Kind { Insert, Delete, Move, Cursor }
}

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
     * 视觉光标插值快照 — Cursor intent 时由 [onVisualIntent] 根据当前/上一份
     * [TextLayoutResult] 的 cursor rect 计算并保存，overlay 据此按 progress 插值。
     */
    private val _visualCursorSnapshot = MutableStateFlow<VisualCursorSnapshot?>(null)
    val visualCursorSnapshot: StateFlow<VisualCursorSnapshot?> = _visualCursorSnapshot.asStateFlow()

    /**
     * #641 评论1 第5节：系统给出权威布局 — 只记录，不修改输入几何。
     * 动画层据此算受影响 range，但不 scrollTo、不改 selection、不改 editor height。
     *
     * #641 评论1 第5节：onVisualIntent 时机 — commit 时 currentSnapshot 可能还是旧 layout。
     * 在新 layout 到达后，若当前活跃 intent 是 Cursor，重新构建 cursor snapshot
     * （用新的 currentSnapshot 作为 newCursorRect，旧的 previousSnapshot 作为 oldCursorRect），
     * 避免 commit 时两份 rect 都是旧 layout。
     */
    fun onAuthoritativeLayout(
        result: TextLayoutResult,
        selection: TextRange,
        scrollY: Int,
    ) {
        previousSnapshot = currentSnapshot
        currentSnapshot = ComposeLayoutSnapshot(result, selection, scrollY)
        // 新 layout 到达后，若当前活跃 intent 是 Cursor，重新构建 cursor snapshot。
        if (_activeIntent.value?.kind == EditorVisualIntent.Kind.Cursor) {
            buildCursorSnapshot()?.let { _visualCursorSnapshot.update { it } }
        }
    }

    /**
     * #641 评论1 第5节：Core 给出视觉意图 — 设置受影响 UTF-16 range 和动画类型。
     * overlay 据此画动画过程。range 已是 UTF-16（调用方从 Core UTF-8 转换）。
     *
     * 对 Cursor intent：从 oldResult.getCursorRect(oldSelection.end) 和
     * newResult.getCursorRect(newSelection.end) 取真实 cursor rect，
     * 保存到 [visualCursorSnapshot] 供 overlay 按 progress 插值绘制。
     * 对 Move intent：保存 old/new selection 供 overlay 做坐标位移。
     */
    fun onVisualIntent(intent: EditorVisualIntent) {
        _activeIntent.update { intent }
        _hiddenRanges.update { intent.affectedRanges.filter { it.start < it.end } }

        when (intent.kind) {
            EditorVisualIntent.Kind.Cursor -> {
                _drawsVisualCursor.update { true }
                buildCursorSnapshot()?.let { _visualCursorSnapshot.update { it } }
            }

            EditorVisualIntent.Kind.Move -> {
                _drawsVisualCursor.update { false }
                _visualCursorSnapshot.update { null }
            }

            else -> {
                _drawsVisualCursor.update { false }
                _visualCursorSnapshot.update { null }
            }
        }
    }

    /**
     * 从当前/上一份 [TextLayoutResult] 取真实 cursor rect 构建插值快照。
     * old cursor 来自 previous result + previous selection end；
     * new cursor 来自 current result + current selection end。
     * 任一 layout 缺失时不构建快照（overlay 只画已有的一侧）。
     */
    private fun buildCursorSnapshot(): VisualCursorSnapshot? {
        val prev = previousSnapshot ?: return null
        val curr = currentSnapshot ?: return null
        val oldCursorRect = prev.result.getCursorRect(prev.selection.end)
        val newCursorRect = curr.result.getCursorRect(curr.selection.end)
        return VisualCursorSnapshot(
            oldCursorRect = oldCursorRect,
            newCursorRect = newCursorRect,
            oldSelectionEnd = prev.selection.end,
            newSelectionEnd = curr.selection.end,
        )
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
    }

    /** 当前布局快照 — 供 overlay 读取 bounding box。 */
    fun currentLayout(): ComposeLayoutSnapshot? = currentSnapshot

    /** 上一份布局快照 — 删除文字动画用旧布局画。 */
    fun previousLayout(): ComposeLayoutSnapshot? = previousSnapshot
}
