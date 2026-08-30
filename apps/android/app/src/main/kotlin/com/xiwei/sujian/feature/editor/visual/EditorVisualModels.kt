package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextRange

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
