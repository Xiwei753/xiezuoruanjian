package com.xiwei.sujian.feature.editor.session

import androidx.compose.runtime.Immutable
import com.xiwei.sujian.feature.editor.platform.EditorEditSource

/**
 * #624 评论9：轻量编辑应用事件 — 热路径（IME/Key → Core EditResult → Android Spannable 局部 patch）
 * 只传 revision/transactionId/selection/contentDelta，不传整章 String。
 *
 * 完整正文只在冷路径（load/snapshot/save/sync/global search/replace-all）经
 * [TargetSnapshot.text] 一次性 materialize；热路径不再每键复制整章。
 *
 * - [revision]/[transactionId]：来自 Rust EditResult；
 * - [operationKind]：来自 VisualIntent 映射；
 * - [source]：编辑来源（NORMAL/UNDO/REDO/PROGRAMMATIC）；
 * - [contentChanged]：本次是否真改了正文（displayPatches 非空）；
 * - [contentDelta]：增量字符统计（不依赖整章 String）；
 * - [selectionAnchorUtf8]/[selectionHeadUtf8]：编辑后真实选区（UTF-8 字节），
 *   -1 表示未携带。
 */
@Immutable
data class EditorAppliedEvent(
    val revision: Long,
    val transactionId: Long,
    val operationKind: EditorOperationKind,
    val source: EditorEditSource,
    val contentChanged: Boolean,
    val contentDelta: EditorContentDelta,
    val selectionAnchorUtf8: Int = -1,
    val selectionHeadUtf8: Int = -1,
)

/**
 * #624 评论9：增量字符统计 — 不需要前后整章 String，只看本次 patch。
 *
 * - [insertedChars]：本次插入的 UTF-16 char 数（Kotlin String.length 精确）；
 * - [deletedChars]：本次删除的 UTF-8 byte 数近似（patch 已 apply 后无法取删除原文，
 *   待 Core EditorContentDelta #624评论8 精确化）；
 * - [insertedNonWhitespaceChars]：本次插入的非空白字符数；
 * - [deletedNonWhitespaceChars]：保守 0（patch 已 apply 后无法取删除原文，
 *   待 Core #624评论8 精确化）。
 */
@Immutable
data class EditorContentDelta(
    val insertedChars: Int = 0,
    val deletedChars: Int = 0,
    val insertedNonWhitespaceChars: Int = 0,
    val deletedNonWhitespaceChars: Int = 0,
) {
    /** 非空白字符净增量 — 用于即时维护 wordCount，避免每键全文 calculateWordCount。 */
    val netNonWhitespace: Int
        get() = insertedNonWhitespaceChars - deletedNonWhitespaceChars
}

/**
 * #624 评论9：从单个 DisplayPatch 推算 [EditorContentDelta]。
 *
 * - insertedChars = insertedText.length（UTF-16 精确）；
 * - deletedChars = (replaceByteEndExclusive - replaceByteStart).coerceAtLeast(0)
 *   （UTF-8 byte 长度近似，patch 已 apply 后无法取删除原文）；
 * - insertedNonWhitespaceChars = insertedText.count { !it.isWhitespace() }；
 * - deletedNonWhitespaceChars = 0（保守，待 Core #624评论8 精确化）。
 */
fun contentDeltaFromPatches(
    insertedText: String,
    replaceByteStart: Int,
    replaceByteEndExclusive: Int,
): EditorContentDelta =
    EditorContentDelta(
        insertedChars = insertedText.length,
        deletedChars = (replaceByteEndExclusive - replaceByteStart).coerceAtLeast(0),
        insertedNonWhitespaceChars = insertedText.count { !it.isWhitespace() },
        deletedNonWhitespaceChars = 0,
    )
