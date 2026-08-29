package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy

/**
 * #641 评论 问题3 + 评论 5457777142 问题2/问题4：retained move —
 * 自动折行/手动换行时被挤到下一行的"保留文字"。
 *
 * [oldRange] 和 [newRange] 表示同一逻辑文本范围在旧布局和新布局中的位置。
 * 位置只从 [androidx.compose.ui.text.TextLayoutResult] 读，动画只负责画。
 */
data class RetainedMove(
    val oldRange: TextRange,
    val newRange: TextRange,
)

/**
 * #641 评论 问题3 + 评论 5457777142 问题2/问题4：视觉动画事务 — 完整的动画事务状态。
 *
 * 包含 old/new layout 快照、old/new affected ranges、retained moves、
 * cursor intent、startFrame 和 motionPolicy。新事务到来时如果旧事务还在跑，
 * 先把当前视觉帧物化成下一事务起点（[startFrame]），再 rebase —
 * 不能 `progress=0` 生硬重开，也不能直接覆盖旧事务。
 *
 * 动画层只"画"，绝不能再改变 viewport / selection / IME 几何。
 * 位置只从 [androidx.compose.ui.text.TextLayoutResult] 读，动画只负责画。
 *
 * #641 评论 5457777142 问题4：不再用单一 `durationMillis`。
 * 改由 [motionPolicy] 提供 textDurationMillis / cursorDurationMillis 两条 timeline，
 * overlay 根据 [EditorMotionPolicy.coordinated] 决定一条还是两条 timeline。
 * `reduceMotion` / textEnabled / cursorEnabled 也在 overlay 这一层一次性落实。
 *
 * @param id 事务 ID — 单调递增，overlay 据此判断是否需要重新启动动画。
 * @param oldLayout 旧布局快照 — 删除/移动文字动画按旧 range 的 bounding box 画旧布局。
 * @param newLayout 新布局快照 — 来自系统 [androidx.compose.foundation.text.BasicTextField]
 *   的最终 [androidx.compose.ui.text.TextLayoutResult]。
 * @param oldRanges 旧受影响 UTF-16 ranges — 删除动画用。
 * @param newRanges 新受影响 UTF-16 ranges — 插入/移动动画用。
 * @param retainedMoves 被挤到下一行的"保留文字"的 old/new range。
 * @param textKind 文字动画类型 — #641 评论 5458880786 问题1a：[ComposeEditorVisualState.materializeStartFrame]
 *   需要知道上一事务的 textKind 才能物化 oldRanges/newRanges（Delete 淡出 oldRanges、Move 淡出 oldRanges+淡入 newRanges、
 *   Insert 淡入 newRanges）。默认 [TextVisualKind.None] 保持向后兼容。
 * @param cursor 光标视觉意图 — animate=true 时 overlay 插值画视觉光标。
 * @param startFrame 上一事务物化出的视觉帧 — 新事务从该帧对应的 progress 开始，
 *   而不是 `snapTo(0f)`。null 表示从 0 开始。
 * @param motionPolicy 动画策略 — overlay 据此决定 text/cursor timeline、
 *   reduceMotion、textEnabled、cursorEnabled。
 */
data class ComposeVisualTransaction(
    val id: Long,
    val oldLayout: ComposeLayoutSnapshot?,
    val newLayout: ComposeLayoutSnapshot?,
    val oldRanges: List<TextRange>,
    val newRanges: List<TextRange>,
    val retainedMoves: List<RetainedMove>,
    val textKind: TextVisualKind = TextVisualKind.None,
    val cursor: CursorVisualIntent?,
    val startFrame: ComposeVisualFrame?,
    val motionPolicy: EditorMotionPolicy,
)
