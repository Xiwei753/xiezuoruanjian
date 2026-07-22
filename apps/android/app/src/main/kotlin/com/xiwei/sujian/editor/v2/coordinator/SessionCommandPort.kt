package com.xiwei.sujian.editor.v2.coordinator

import com.xiwei.sujian.data.BridgeResult

data class TargetSnapshot(
    val text: String,
    val cursorUtf8: Int,
    val revision: Long,
    val selectionAnchorUtf8: Int,
    val selectionHeadUtf8: Int
)

data class TargetDecorations(
    val searchHighlightsUtf8: List<Pair<Int, Int>> = emptyList(),
    val selectionStartUtf8: Int = -1,
    val selectionEndUtf8: Int = -1
)

sealed class TargetCommand {
    data class Replace(
        val byteStart: Int,
        val byteEndExclusive: Int,
        val replacementText: String,
        val originalText: String
    ) : TargetCommand()

    data class ReplaceAll(
        val searchText: String,
        val replacementText: String
    ) : TargetCommand()

    data class SetSelection(
        val anchorUtf8: Int,
        val headUtf8: Int
    ) : TargetCommand()
}

sealed class TargetCommandResult {
    data class Success(
        val snapshot: TargetSnapshot
    ) : TargetCommandResult()

    data class Failed(
        val reason: String
    ) : TargetCommandResult()
}

interface SessionCommandPort {
    fun queryTargetSnapshot(targetId: String): TargetSnapshot?
    fun applyTargetCommand(targetId: String, command: TargetCommand): TargetCommandResult
    fun setTargetDecorations(targetId: String, decorations: TargetDecorations)
}
