package com.xiwei.sujian.editor.selfrender

data class CompositionSessionId(val value: Long)

data class CompositionSession(
    val sessionId: CompositionSessionId,
    val committedRevisionId: Long,
    val committedTextAtStart: String,
    val replaceStart: Int,
    val replaceEndExclusive: Int,
    var preeditText: String,
    var preeditCursorOffset: Int,
    var lastSubmittedGeneration: Long = 0
) {
    val isActive: Boolean get() = sessionId.value > 0

    fun replaceRange(): IntRange = replaceStart..replaceEndExclusive

    fun replaceRangeLength(): Int = replaceEndExclusive - replaceStart

    fun buildVirtualText(): String {
        val start = replaceStart.coerceIn(0, committedTextAtStart.length)
        val end = replaceEndExclusive.coerceIn(0, committedTextAtStart.length)
        return committedTextAtStart.substring(0, start) + preeditText + committedTextAtStart.substring(end)
    }

    fun preeditRangeInVirtualText(): IntRange {
        return replaceStart..(replaceStart + preeditText.length)
    }

    fun updatePreedit(newPreeditText: String, newCursorOffset: Int): CompositionSession {
        return CompositionSession(
            sessionId = sessionId,
            committedRevisionId = committedRevisionId,
            committedTextAtStart = committedTextAtStart,
            replaceStart = replaceStart,
            replaceEndExclusive = replaceEndExclusive,
            preeditText = newPreeditText,
            preeditCursorOffset = newCursorOffset.coerceIn(0, newPreeditText.length),
            lastSubmittedGeneration = lastSubmittedGeneration + 1
        )
    }

    fun setComposingRegion(newStart: Int, newEnd: Int, newPreeditText: String): CompositionSession {
        val clampedStart = newStart.coerceIn(0, committedTextAtStart.length)
        val clampedEnd = newEnd.coerceIn(0, committedTextAtStart.length)
        val (start, end) = if (clampedStart <= clampedEnd) clampedStart to clampedEnd else clampedEnd to clampedStart
        return CompositionSession(
            sessionId = sessionId,
            committedRevisionId = committedRevisionId,
            committedTextAtStart = committedTextAtStart,
            replaceStart = start,
            replaceEndExclusive = end,
            preeditText = newPreeditText,
            preeditCursorOffset = newPreeditText.length,
            lastSubmittedGeneration = lastSubmittedGeneration + 1
        )
    }

    fun commit(commitText: String): Pair<CompositionSession, String> {
        val committedAfter = committedTextAtStart.substring(0, replaceStart) +
                commitText +
                committedTextAtStart.substring(replaceEndExclusive.coerceIn(0, committedTextAtStart.length))
        val cleared = CompositionSession(
            sessionId = CompositionSessionId(0),
            committedRevisionId = committedRevisionId,
            committedTextAtStart = committedTextAtStart,
            replaceStart = 0,
            replaceEndExclusive = 0,
            preeditText = "",
            preeditCursorOffset = 0
        )
        return Pair(cleared, committedAfter)
    }

    fun cancel(): CompositionSession {
        return CompositionSession(
            sessionId = CompositionSessionId(0),
            committedRevisionId = committedRevisionId,
            committedTextAtStart = committedTextAtStart,
            replaceStart = 0,
            replaceEndExclusive = 0,
            preeditText = "",
            preeditCursorOffset = 0
        )
    }

    companion object {
        private var nextSessionId: Long = 1L

        fun createNew(
            committedRevisionId: Long,
            committedText: String,
            replaceStart: Int,
            replaceEndExclusive: Int,
            preeditText: String,
            preeditCursorOffset: Int
        ): CompositionSession {
            return CompositionSession(
                sessionId = CompositionSessionId(nextSessionId++),
                committedRevisionId = committedRevisionId,
                committedTextAtStart = committedText,
                replaceStart = replaceStart.coerceIn(0, committedText.length),
                replaceEndExclusive = replaceEndExclusive.coerceIn(0, committedText.length),
                preeditText = preeditText,
                preeditCursorOffset = preeditCursorOffset
            )
        }

        val EMPTY = CompositionSession(
            sessionId = CompositionSessionId(0),
            committedRevisionId = 0,
            committedTextAtStart = "",
            replaceStart = 0,
            replaceEndExclusive = 0,
            preeditText = "",
            preeditCursorOffset = 0
        )
    }
}
