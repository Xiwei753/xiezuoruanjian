package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextRange
import uniffi.writer_core.EditorOperationKindDto
import uniffi.writer_core.EditorTransactionCauseDto

/**
 * Issue #735 评论 5771063665：Android 平台自己的动画模式 —
 * Core 已删除 `AnimationModeDto`，Android 自己决定动画策略。
 *
 * - [CLUSTER_ANIMATION]：按 grapheme cluster 吐字/吞字。
 * - [GLYPH_ANIMATION]：按 glyph 吐字/吞字。
 * - [RUN_ANIMATION]：按 run 吐字/吞字。
 * - [LINE_REFLOW_ANIMATION]：行回流动画。
 * - [SNAPSHOT_ANIMATION]：快照动画。
 * - [SYSTEM_SUPPRESSED]：系统抑制动画（reduce-motion 等）。
 */
enum class AnimationMode {
    CLUSTER_ANIMATION,
    GLYPH_ANIMATION,
    RUN_ANIMATION,
    LINE_REFLOW_ANIMATION,
    SNAPSHOT_ANIMATION,
    SYSTEM_SUPPRESSED,
}

/**
 * Issue #735 评论 5771063665：编辑事实 —
 * 取代已删除的 [EditorVisualIntent]（Core 已不再返回视觉意图）。
 *
 * 所有正文编辑都以已经接受的 `EditorEditResult` 为编辑事实。
 * Android 从 `cause`、`operationKind`、`offsetMap` 推导动画策略，
 * 不再拿 Core 的 Visual DTO。
 *
 * @param coreTransactionId Core 事务 ID — 来自 Rust EditResult，单调递增。
 * @param baseRevision 编辑前正文版本号。
 * @param newRevision 编辑后正文版本号。
 * @param cause 编辑事实：本次事务的原因。
 * @param operationKind 编辑事实：本次操作的语义类别。
 * @param offsetMap Core UTF-8 offset map — 已由调用方转成 UTF-16 [VisualOffsetMap]。
 * @param oldRanges 旧受影响 UTF-16 ranges — 从 offsetMap 补集或 replaceBounds 推导。
 * @param newRanges 新受影响 UTF-16 ranges。
 * @param textKind 文字动画类型 — 从 operationKind 推导。
 * @param replaceBounds 明确的 replace 边界（UTF-16）。
 * @param expectedOldText 旧正文（UTF-16）。
 * @param expectedNewText 新正文（UTF-16）。
 * @param oldAnimationUnits 旧动画单元 UTF-16 ranges — Android 自己按 grapheme cluster 拆分。
 * @param newAnimationUnits 新动画单元 UTF-16 ranges — Android 自己按 grapheme cluster 拆分。
 * @param oldSelectionEndUtf16 旧光标位置（UTF-16 offset）。
 * @param newSelectionEndUtf16 新光标位置（UTF-16 offset）。
 * @param durationMs Core 建议动画时长。
 * @param animationMode Android 自己推导的动画模式。
 */
data class EditorEditFact(
    val coreTransactionId: Long,
    val baseRevision: Long,
    val newRevision: Long,
    val cause: EditorTransactionCauseDto,
    val operationKind: EditorOperationKindDto,
    val offsetMap: VisualOffsetMap?,
    val oldRanges: List<TextRange>,
    val newRanges: List<TextRange>,
    val textKind: TextVisualKind,
    val replaceBounds: VisualReplaceBounds? = null,
    val expectedOldText: String = "",
    val expectedNewText: String = "",
    val oldAnimationUnits: List<TextRange> = emptyList(),
    val newAnimationUnits: List<TextRange> = emptyList(),
    val oldSelectionEndUtf16: Int = -1,
    val newSelectionEndUtf16: Int = -1,
    val durationMs: Long = 0L,
    val animationMode: AnimationMode = AnimationMode.CLUSTER_ANIMATION,
) {
    /**
     * #694 评论第 5 步：判断本 fact 的 cause 是否为本地输入
     * （已由 Android InputTransformation 提供 visual edit，Core 回声只当 ACK）。
     *
     * TYPING / TYPING_COMMIT / IME_COMPOSITION / PASTE / DELETE → true。
     * UNDO / REDO / PROGRAMMATIC / LOAD / FORMAT → false（仍走 Core visual path）。
     */
    fun isLocalInputCause(): Boolean =
        when (cause) {
            EditorTransactionCauseDto.TYPING,
            EditorTransactionCauseDto.TYPING_COMMIT,
            EditorTransactionCauseDto.IME_COMPOSITION,
            EditorTransactionCauseDto.PASTE,
            EditorTransactionCauseDto.DELETE,
            -> true
            EditorTransactionCauseDto.UNDO,
            EditorTransactionCauseDto.REDO,
            EditorTransactionCauseDto.PROGRAMMATIC,
            EditorTransactionCauseDto.LOAD,
            EditorTransactionCauseDto.FORMAT,
            -> false
        }
}

/**
 * #641 评论 5458880786 问题2a：明确的 replace 边界（UTF-16）—
 * 供 [ComposeEditorVisualState] 算共同前缀/后缀。
 *
 * 一次 replace 把 oldText[oldStart..oldEnd) 替换成 newText[newStart..newEnd)，
 * 共同前缀 0..oldStart ↔ 0..newStart，共同后缀 oldEnd..oldText.length ↔ newEnd..newText.length。
 * retained reflow 用确定边界算 suffix 起点，不再从空 oldRanges/newRanges 猜。
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
 * @param kind 映射类型：IDENTITY 表示文本相同且 offset 不变；SHIFTED 表示文本相同但 offset 改变
 *   （被前后增删平移）。内容变化/被编辑/删除的区域根本没有 mapping 条目。
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
    /** 文本相同且 offset 不变（位置不变）。 */
    IDENTITY,

    /** 文本相同但 offset 改变（被前后增删平移）；内容变化/删除区域没有 mapping。 */
    SHIFTED,
}
