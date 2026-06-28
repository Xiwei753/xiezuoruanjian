package com.xiwei.sujian.data

import com.xiwei.sujian.model.EditorAnimationEventData
import com.xiwei.sujian.model.EditorAnimationKindData
import uniffi.writer_core.EditorAnimationEventDto
import uniffi.writer_core.EditorAnimationKindDto

/**
 * EditorAnimationBridge — Android editor animation domain bridge.
 *
 * Android consumes typed models from Core through UniFFI.  Do not serialize these DTOs to JSON and
 * parse them back on Android; the JSON property is retained only for the Desktop QML route.
 */
class EditorAnimationBridge internal constructor(private val appService: AppServiceBridge) {
    fun editorAnimationEvents(
        oldText: String,
        newText: String,
        oldCursorIndex: UInt,
        newCursorIndex: UInt,
        cause: String,
        maxAnimatedChars: UInt,
        animationDurationMs: ULong
    ): BridgeResult<List<EditorAnimationEventData>> {
        return when (val result = appService.editorAnimationEventDtos(
            oldText = oldText,
            newText = newText,
            oldCursorIndex = oldCursorIndex,
            newCursorIndex = newCursorIndex,
            cause = cause,
            maxAnimatedChars = maxAnimatedChars,
            animationDurationMs = animationDurationMs
        )) {
            is BridgeResult.Success -> BridgeResult.Success(result.data.map { it.toModel() })
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }
}

private fun EditorAnimationEventDto.toModel(): EditorAnimationEventData = EditorAnimationEventData(
    id = id,
    kind = when (kind) {
        EditorAnimationKindDto.INSERT -> EditorAnimationKindData.Insert
        EditorAnimationKindDto.DELETE -> EditorAnimationKindData.Delete
        EditorAnimationKindDto.CURSOR -> EditorAnimationKindData.Cursor
    },
    rangeStart = rangeStart.toInt(),
    rangeLen = rangeLen.toInt(),
    text = text,
    oldCursorIndex = oldCursorIndex.toInt(),
    newCursorIndex = newCursorIndex.toInt(),
    durationMs = durationMs.toLong()
)
