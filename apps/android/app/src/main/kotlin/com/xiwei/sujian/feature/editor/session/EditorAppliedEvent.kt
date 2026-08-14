package com.xiwei.sujian.feature.editor.session

import androidx.compose.runtime.Immutable
import com.xiwei.sujian.feature.editor.platform.EditorEditSource
import uniffi.writer_core.EditorContentDeltaDto

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
 * #624 评论8/9：增量字符统计 — Core 计算的本地 delta 真值，不依赖前后整章 String。
 *
 * - [insertedChars]：本次插入的 Unicode scalar（char）数；
 * - [deletedChars]：本次删除的 Unicode scalar（char）数；
 * - [insertedNonWhitespaceChars]：本次插入的非空白字符数；
 * - [deletedNonWhitespaceChars]：本次删除的非空白字符数。
 *
 * 全部字段由 Rust `EditorEditResultDto.contentDelta` 映射而来（对 inserted_text /
 * deleted_text 局部计算），不再是 UTF-8 byte 近似。
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
 * #624 评论8/9：Core delta 真值 → 会话层轻量 [EditorContentDelta]。
 *
 * Core 对本次 inserted_text / deleted_text 局部计算（Unicode scalar 计数），
 * Android 直接消费，不再用 UTF-8 byte 长度冒充 deletedChars、不再全文重算。
 */
fun EditorContentDeltaDto.toSessionDelta(): EditorContentDelta =
    EditorContentDelta(
        insertedChars = insertedChars.toInt(),
        deletedChars = deletedChars.toInt(),
        insertedNonWhitespaceChars = insertedNonWhitespaceChars.toInt(),
        deletedNonWhitespaceChars = deletedNonWhitespaceChars.toInt(),
    )
