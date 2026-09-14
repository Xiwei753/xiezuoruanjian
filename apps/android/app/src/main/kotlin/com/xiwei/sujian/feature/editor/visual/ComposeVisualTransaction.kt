package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy
import uniffi.writer_core.AnimationModeDto

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
 * #644 评论 #684：事务创建后彻底冻结 —
 * 该对象创建以后禁止 `.copy(oldLayout=...)`、`.copy(newLayout=...)` 之类的后补行为。
 * oldLayout/newLayout/retainedMoves/cursorStartRect/cursorEndRect/startFrame
 * 全部在 [ComposeVisualFrameCoordinator.onLayout] 中一次性确定。
 *
 * @param id 事务 ID — [ComposeVisualFrameCoordinator] 内部分配，overlay 据此判断是否需要重新启动动画。
 * @param coreTransactionIds Core 事务 ID 列表 — 一笔 visual 事务可能对应多笔 Core 事务。
 * @param oldLayout 旧布局快照 — 删除/移动文字动画按旧 range 的 bounding box 画旧布局。
 * @param newLayout 新布局快照 — 来自系统 [androidx.compose.foundation.text.BasicTextField]
 *   的最终 [androidx.compose.ui.text.TextLayoutResult]。
 * @param intents 本事务包含的所有 [EditorVisualIntent] — 帧协调链合并。
 * @param oldRanges 旧受影响 UTF-16 ranges — 删除动画用。
 * @param newRanges 新受影响 UTF-16 ranges — 插入/移动动画用。
 * @param retainedMoves 被挤到下一行的"保留文字"的 old/new range。
 * @param cursorStartRect 视觉光标起始矩形 — 事务创建时确定。
 * @param cursorEndRect 视觉光标结束矩形 — 事务创建时确定。
 * @param startFrame 上一事务物化出的视觉帧 — 新事务从该帧对应的 progress 开始。
 * @param durationMs 动画时长（ms）。
 * @param motionPolicy 动画策略 — overlay 据此决定 text/cursor timeline。
 * @param suppressedCurrentRanges #684 评论 5663862982：这一笔动画期间 BasicTextField
 *   必须保持透明的最终正文 ranges。事务生成后冻结。包含三部分：
 *   (1) 本事务自己 owned 的 new ranges（Insert/Move 的 newRanges）；
 *   (2) retained moves 的 newRanges（被挤到下一行的保留文字，overlay 画时系统正文必须透明）；
 *   (3) 上一帧仍由 startFrame 接管、按 composedOffsetMap 映射到当前 new text 后仍存活的 suppressed ranges。
 *   overlay 据此隐藏 BasicTextField 对应区间，避免重影/跳行。
 * @param textKind #684 评论 5664636035 Bug1：屏幕事务自己的文字类型 — 按最终净变化决定，
 *   不再从最后一笔 intent 的 textKind 读。oldChanged/newChanged 都空→None；
 *   oldChanged 空→Insert；newChanged 空→Delete；否则→Move。
 * @param animationMode #684 评论 5665907509 问题1：屏幕事务冻结的 Core 动画模式 —
 *   用 screenSuppressed 收口：整条 chain 里只要有一笔 SYSTEM_SUPPRESSED，当前屏幕事务
 *   的 animationMode 就是 SYSTEM_SUPPRESSED。overlay 据此判断 systemSuppressed
 *   （不再从 _activeIntent 读取），保证 SYSTEM_SUPPRESSED 到来时 overlay 直接落到
 *   系统最终正文，不会让上一笔动画的 suppressed ranges / startFrame 跨过这笔
 *   suppressed 事务继续跑。默认 [AnimationModeDto.CLUSTER_ANIMATION] 兼容不显式传
 *   animationMode 的现有调用。
 * @param textAnimationActive #684 评论 5666730754：屏幕事务创建时一次算死的正文视觉所有权 —
 *   表示这笔事务的正文是否真的被 overlay 接管动画过（不是"事务还挂着"）。
 *   当 screenSuppressed 或 motionPolicy 关闭文字动画或 transactionTextKind==None 时为 false。
 *   materializeStartFrame 据此决定是否物化 prev 自己的 currentSlices/retainedSlices，
 *   overlay 据此决定 textEnabled，无 overlay 工作的事务据此决定是否立即 settle。
 * @param cursorAnimationActive #684 评论 5666730754：屏幕事务创建时一次算死的光标视觉所有权 —
 *   表示这笔事务的光标是否真的被 overlay 接管动画过（不是"事务还挂着"）。
 *   当 screenSuppressed 或 motionPolicy 关闭光标动画或 chain 里没有 cursor.animate==true
 *   或光标位置未变（firstCursor.oldEndUtf16 == lastCursor.newEndUtf16）时为 false。
 *   interruptedCursorRect 据此决定是否从上一笔插值，overlay 据此决定 cursorEnabled。
 */
data class ComposeVisualTransaction(
    val id: Long,
    val coreTransactionIds: List<Long>,
    val oldLayout: ComposeLayoutSnapshot?,
    val newLayout: ComposeLayoutSnapshot?,
    val intents: List<EditorVisualIntent>,
    val oldRanges: List<TextRange>,
    val newRanges: List<TextRange>,
    val retainedMoves: List<RetainedMove>,
    val cursorStartRect: Rect?,
    val cursorEndRect: Rect?,
    val startFrame: ComposeVisualFrame?,
    val durationMs: Long,
    val motionPolicy: EditorMotionPolicy,
    val suppressedCurrentRanges: List<TextRange> = emptyList(),
    val textKind: TextVisualKind = TextVisualKind.None,
    val animationMode: AnimationModeDto = AnimationModeDto.CLUSTER_ANIMATION,
    val textAnimationActive: Boolean = false,
    val cursorAnimationActive: Boolean = false,
)
