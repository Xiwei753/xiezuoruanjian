package com.xiwei.sujian.data

import com.xiwei.sujian.model.EditorAnimationKindData
import com.xiwei.sujian.model.EditorVisualTransactionData
import com.xiwei.sujian.model.SujianEditCauseData
import com.xiwei.sujian.model.VisualCoordinateModeData
import uniffi.writer_core.EditorAnimationKindDto
import uniffi.writer_core.EditorVisualTransactionDto
import uniffi.writer_core.EditorTransactionCauseDto
import uniffi.writer_core.VisualCoordinateModeDto

/**
 * EditorAnimationBridge — Android editor animation domain bridge.
 *
 * Android consumes typed models from Core through UniFFI.  Do not serialize these DTOs to JSON and
 * parse them back on Android; the JSON property is retained only for the Desktop QML route.
 */
class EditorAnimationBridge internal constructor(private val appService: AppServiceBridge) {

    /**
     * 调用 Core 的 editor_visual_transaction API。
     *
     * 返回 EditorVisualTransactionData（如果动画需要），或 null（如果不需要动画）。
     * 坐标字段（oldCursorRect, newCursorRect 等）由 Android 层自行填充。
     */
    fun editorVisualTransaction(
        oldText: String,
        newText: String,
        oldCursorIndex: UInt,
        newCursorIndex: UInt,
        cause: String,
        maxAnimatedChars: UInt,
        animationDurationMs: ULong
    ): BridgeResult<EditorVisualTransactionData?> {
        return when (val result = appService.editorVisualTransactionDto(
            oldText = oldText,
            newText = newText,
            oldCursorIndex = oldCursorIndex,
            newCursorIndex = newCursorIndex,
            cause = cause,
            maxAnimatedChars = maxAnimatedChars,
            animationDurationMs = animationDurationMs
        )) {
            is BridgeResult.Success -> BridgeResult.Success(result.data?.toModel())
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }
}

private fun EditorVisualTransactionDto.toModel(): EditorVisualTransactionData = EditorVisualTransactionData(
    id = id,
    kind = when (kind) {
        EditorAnimationKindDto.INSERT -> EditorAnimationKindData.Insert
        EditorAnimationKindDto.DELETE -> EditorAnimationKindData.Delete
        EditorAnimationKindDto.CURSOR -> EditorAnimationKindData.Cursor
    },
    cause = when (cause) {
        EditorTransactionCauseDto.TYPING -> SujianEditCauseData.Typing
        EditorTransactionCauseDto.DELETE -> SujianEditCauseData.Delete
        EditorTransactionCauseDto.IME_COMPOSITION -> SujianEditCauseData.ImeComposition
        EditorTransactionCauseDto.TYPING_COMMIT -> SujianEditCauseData.TypingCommit
        EditorTransactionCauseDto.PASTE -> SujianEditCauseData.Paste
        EditorTransactionCauseDto.UNDO -> SujianEditCauseData.Undo
        EditorTransactionCauseDto.REDO -> SujianEditCauseData.Redo
        EditorTransactionCauseDto.LOAD -> SujianEditCauseData.Load
        EditorTransactionCauseDto.FORMAT -> SujianEditCauseData.Format
        EditorTransactionCauseDto.PROGRAMMATIC -> SujianEditCauseData.Programmatic
    },
    oldText = oldText,
    newText = newText,
    oldSelectionAnchor = oldSelectionAnchor.toInt(),
    oldSelectionHead = oldSelectionHead.toInt(),
    newSelectionAnchor = newSelectionAnchor.toInt(),
    newSelectionHead = newSelectionHead.toInt(),
    insertedRangeStart = insertedRangeStart.toInt(),
    insertedRangeEnd = insertedRangeEnd.toInt(),
    durationMs = durationMs.toLong(),
    coordinateMode = when (coordinateMode) {
        VisualCoordinateModeDto.BASELINE -> VisualCoordinateModeData.Baseline
    }
)
