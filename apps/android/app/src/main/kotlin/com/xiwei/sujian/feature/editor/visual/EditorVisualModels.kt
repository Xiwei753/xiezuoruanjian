package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextRange
import uniffi.writer_core.AnimationModeDto

/**
 * #641 评论1 第4/5节：Core 返回的视觉意图 — 受影响的 UTF-16 range 和动画类型。
 * 从 Core display patch / VisualIntent 映射，offset 是 UTF-16（已由调用方从
 * UTF-8 byte 转换），不再用 byte 作为 Compose offset。
 *
 * #644 评论 #684：Core 事务身份不再丢失 —
 * [coreTransactionId]、[baseRevision]、[newRevision]、[animationMode]、[durationMs]
 * 原样从 Core EditResult 传入，Android 视觉层不再自行生成伪事务 ID。
 *
 * @param coreTransactionId Core 事务 ID — 来自 Rust EditResult，单调递增。
 * @param baseRevision 编辑前正文版本号。
 * @param newRevision 编辑后正文版本号。
 * @param animationMode Core 动画模式 — CLUSTER_ANIMATION / SYSTEM_SUPPRESSED 等。
 * @param durationMs Core 建议动画时长。
 * @param offsetMap Core UTF-8 offset map — 已由调用方转成 UTF-16 [VisualOffsetMap]。
 * @param oldRanges 旧受影响 UTF-16 ranges — 删除动画用（来自 Core oldAffectedByteRanges）。
 * @param newRanges 新受影响 UTF-16 ranges — 插入/移动动画用（来自 Core newAffectedByteRanges）。
 * @param textKind 文字动画类型。
 * @param cursor 光标视觉意图 — null 表示不画视觉光标。
 * @param replaceBounds 明确的 replace 边界（UTF-16）。
 */
data class EditorVisualIntent(
    val coreTransactionId: Long,
    val baseRevision: Long,
    val newRevision: Long,
    val animationMode: AnimationModeDto,
    val durationMs: Long,
    val offsetMap: VisualOffsetMap?,
    val oldRanges: List<TextRange>,
    val newRanges: List<TextRange>,
    val textKind: TextVisualKind,
    val cursor: CursorVisualIntent?,
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
 * #644 评论 #684：Android 视觉层自己的 UTF-16 offset map —
 * 由 [WritingPaneEffects] 在 UI 映射边界从 Core UTF-8 offset map 转换而来，
 * 视觉层只保存 UTF-16，不再拿 Core UTF-8 byte offset 算 Compose 几何。
 *
 * @param entries UTF-16 offset map 条目列表 — 顺序排列，覆盖受影响区域。
 */
data class VisualOffsetMap(
    val entries: List<VisualOffsetMapEntry>,
)

/**
 * #644 评论 #684：UTF-16 offset map 条目 —
 * 表示一段 old UTF-16 range 到 new UTF-16 range 的映射。
 *
 * @param oldStart 旧正文 UTF-16 起始偏移。
 * @param newStart 新正文 UTF-16 起始偏移。
 * @param length 映射长度（UTF-16 code units）。
 * @param kind 映射类型：IDENTITY 表示原文保留（位置可能平移），SHIFTED 表示内容变化。
 */
data class VisualOffsetMapEntry(
    val oldStart: Int,
    val newStart: Int,
    val length: Int,
    val kind: VisualOffsetMapKind,
)

/**
 * #644 评论 #684：offset map 条目类型。
 */
enum class VisualOffsetMapKind {
    /** 原文保留，位置可能因前后增删而平移。 */
    IDENTITY,
    /** 内容被替换/移动。 */
    SHIFTED,
}
