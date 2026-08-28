package com.xiwei.sujian.feature.editor.session

import androidx.compose.runtime.Immutable
import uniffi.writer_core.EditorContentDeltaDto
import uniffi.writer_core.EditorTransactionCauseDto

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
 * - [cause]：#624 评论10 第5项 — Core VisualIntent.cause 真值（Typing/Paste/Delete/
 *   Undo/Redo/Programmatic/...），统计层按此明确分类，不再靠 source/operationKind 猜；
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
    val cause: EditorTransactionCauseDto,
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

// #624 评论10 第5项：统计 source 字符串常量。
private const val STATS_SOURCE_TYPING = "typing"
private const val STATS_SOURCE_PASTED = "pasted"
private const val STATS_SOURCE_DELETED = "deleted"
private const val STATS_SOURCE_UNDO = "undo"
private const val STATS_SOURCE_REDO = "redo"
private const val STATS_SOURCE_PROGRAMMATIC = "programmatic"

/**
 * #624 评论10 第5项：把 Core [EditorTransactionCauseDto] 映射为统计 source 字符串。
 */
fun writingEventSourceFrom(cause: EditorTransactionCauseDto): String =
    when (cause) {
        EditorTransactionCauseDto.TYPING,
        EditorTransactionCauseDto.TYPING_COMMIT,
        EditorTransactionCauseDto.IME_COMPOSITION,
        -> STATS_SOURCE_TYPING
        EditorTransactionCauseDto.PASTE -> STATS_SOURCE_PASTED
        EditorTransactionCauseDto.DELETE -> STATS_SOURCE_DELETED
        EditorTransactionCauseDto.UNDO -> STATS_SOURCE_UNDO
        EditorTransactionCauseDto.REDO -> STATS_SOURCE_REDO
        EditorTransactionCauseDto.PROGRAMMATIC,
        EditorTransactionCauseDto.LOAD,
        EditorTransactionCauseDto.FORMAT,
        -> STATS_SOURCE_PROGRAMMATIC
    }

/**
 * #624 评论11 第4项：cause → 各分类计数 mapper — 一次事件只应有一个来源的
 * 非零字段，与 Core `WritingInputEvent::new()` 的
 * `net_delta_chars = inserted_chars + pasted_chars + ai_inserted_chars - deleted_chars`
 * 一致。
 *
 * - **Paste**：inserted=0、pasted=delta.inserted、deleted=delta.deleted
 *   （旧实现 inserted 和 pasted 都传 delta.inserted，粘贴 5 个字符记成净增 10）；
 * - **Typing/IME**：inserted=delta.inserted、pasted=0、deleted=delta.deleted；
 * - **Delete**：inserted=0、pasted=0、deleted=delta.deleted；
 * - **Undo/Redo/Programmatic/Load/Format**：按实际 delta 传 inserted/deleted，
 *   source 继续由 [writingEventSourceFrom] 映射为非 HumanTyped。
 */
@Immutable
data class StatsEventCounts(
    val insertedChars: Int = 0,
    val deletedChars: Int = 0,
    val pastedChars: Int = 0,
)

/** #624 评论11 第4项：cause → 各分类计数（不依赖整章 String，不猜测 paste）。 */
fun statsCountsFor(
    cause: EditorTransactionCauseDto,
    delta: EditorContentDelta,
): StatsEventCounts =
    when (cause) {
        EditorTransactionCauseDto.PASTE ->
            StatsEventCounts(
                insertedChars = 0,
                pastedChars = delta.insertedChars,
                deletedChars = delta.deletedChars,
            )
        EditorTransactionCauseDto.DELETE ->
            StatsEventCounts(
                deletedChars = delta.deletedChars,
            )
        EditorTransactionCauseDto.TYPING,
        EditorTransactionCauseDto.TYPING_COMMIT,
        EditorTransactionCauseDto.IME_COMPOSITION,
        EditorTransactionCauseDto.UNDO,
        EditorTransactionCauseDto.REDO,
        EditorTransactionCauseDto.PROGRAMMATIC,
        EditorTransactionCauseDto.LOAD,
        EditorTransactionCauseDto.FORMAT,
        ->
            StatsEventCounts(
                insertedChars = delta.insertedChars,
                deletedChars = delta.deletedChars,
            )
    }

/**
 * #641：Core 视觉意图事件 — presentation/session 层发布的纯数据事件，
 * 不含 Compose/visual 依赖。UI 层（WritingPaneEditorContent）收集后，
 * 用 TextOffsetUtils 把 Core old/new UTF-8 ranges 转成 UTF-16 EditorVisualIntent，
 * 调用 ComposeEditorVisualState.onVisualIntent。
 *
 * 设计目的：解耦 EditorViewModel 与 feature.editor.visual。
 * EditorViewModel 不能直接依赖 ComposeEditorVisualState/EditorVisualIntent，
 * 但 commitToCore 成功后必须把 Core 返回的 visual intent 传到 UI 层。
 *
 * @param targetId 目标章节 ID，供 UI 层按 target 过滤。
 * @param oldText 提交前的完整正文（UTF-8），用于 oldAffectedByteRanges → UTF-16 换算。
 * @param newText 提交后的完整正文（UTF-8），用于 newAffectedByteRanges → UTF-16 换算。
 * @param visualIntent Core 返回的视觉意图（projection 层，纯数据）。
 * @param oldSelectionEndUtf8 提交前光标位置（UTF-8），用于 cursor rect 插值。
 * @param newSelectionEndUtf8 提交后光标位置（UTF-8），用于 cursor rect 插值。
 */
@Immutable
data class CoreVisualIntentEvent(
    val targetId: String,
    val oldText: String,
    val newText: String,
    val visualIntent: com.xiwei.sujian.feature.editor.projection.VisualIntent,
    val oldSelectionEndUtf8: Int,
    val newSelectionEndUtf8: Int,
)
