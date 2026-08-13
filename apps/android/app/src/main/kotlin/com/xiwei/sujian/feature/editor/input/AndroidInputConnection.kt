package com.xiwei.sujian.feature.editor.input

import android.view.View
import android.view.inputmethod.BaseInputConnection
import com.xiwei.sujian.feature.editor.pipeline.InputCommandPort
import com.xiwei.sujian.feature.editor.projection.DisplayTextMirror
import com.xiwei.sujian.feature.editor.projection.DisplayTextProjection
import com.xiwei.sujian.feature.editor.projection.EditResult
import uniffi.writer_core.EditorTransactionCauseDto

class AndroidInputConnection(
    private val adapter: AndroidInputAdapter,
    private val mirror: DisplayTextMirror,
    private val commandPort: InputCommandPort,
    private val hostView: View,
    private val projectionProvider: (() -> DisplayTextProjection)? = null,
) : BaseInputConnection(hostView, true) {
    private fun displayUtf16ToRealUtf8(utf16: Int): Int {
        val projection = projectionProvider?.invoke()
        if (projection != null) {
            return projection.displayUtf16ToRealUtf8(utf16)
        }
        return mirror.getTextOffsetIndex().utf16ToUtf8(utf16)
    }

    private fun realUtf8ToDisplayUtf16(utf8: Int): Int {
        val projection = projectionProvider?.invoke()
        if (projection != null) {
            return projection.realUtf8ToDisplayUtf16(utf8)
        }
        return mirror.getTextOffsetIndex().utf8ToUtf16(utf8)
    }

    override fun performEditorAction(actionCode: Int): Boolean {
        val profile = adapter.getCurrentProfile()
        if (profile.commitOnImeAction) {
            when (actionCode) {
                android.view.inputmethod.EditorInfo.IME_ACTION_DONE,
                android.view.inputmethod.EditorInfo.IME_ACTION_GO,
                android.view.inputmethod.EditorInfo.IME_ACTION_NEXT,
                android.view.inputmethod.EditorInfo.IME_ACTION_SEARCH,
                -> {
                    adapter.onPerformEditorAction?.invoke(actionCode)
                    return true
                }
            }
        }
        adapter.onPerformEditorAction?.invoke(actionCode)
        return true
    }

    override fun commitText(
        text: CharSequence?,
        newCursorPosition: Int,
    ): Boolean {
        val commitStr = text?.toString() ?: ""
        if (adapter.shouldForbidNewline(commitStr)) {
            return true
        }
        if (adapter.wouldExceedMaxLength(commitStr)) {
            return true
        }
        if (adapter.isComposing()) {
            adapter.handleCompositionCommitWithText(commitStr, newCursorPosition)
            notifySelectionChanged()
            return true
        }
        // #624 评论2：非 composition 状态收到 "\n" / "\r\n" 不再走普通
        // sendCommitTextToKernel()，统一调用 insertLineBreak()（CRLF 先规范化为
        // 一个逻辑换行）。连续按 Enter 得到连续 \n，第二个空行不会被吞掉。
        // 输出与其他 send* 路径一致经 onPipelineOutput 回到宿主 — 不能丢弃。
        if (commitStr == "\n" || commitStr == "\r\n") {
            adapter.sendLineBreakToKernel(EditorTransactionCauseDto.TYPING)
            notifySelectionChanged()
            return true
        }
        val selStart = mirror.getCommittedSelectionStartUtf8()
        val selEnd = mirror.getCommittedSelectionEndUtf8()
        val byteStart = selStart
        val byteEnd = selEnd
        val originalText = if (byteStart != byteEnd) mirror.committedSliceUtf8(byteStart, byteEnd) else ""
        val (resultingAnchor, resultingHead) =
            InputCursorMapper.computeResultingSelectionUtf8(
                mirror.getTextOffsetIndex(),
                byteStart,
                byteEnd,
                commitStr,
                newCursorPosition,
            )
        adapter.sendCommitTextToKernel(
            byteStart,
            byteEnd,
            commitStr,
            originalText,
            resultingAnchor,
            resultingHead,
            EditorTransactionCauseDto.TYPING,
        )
        notifySelectionChanged()
        return true
    }

    /**
     * Delete text around the cursor. [beforeLength]/[afterLength] are in UTF-16 code units
     * (Android InputConnection convention), not UTF-8 bytes. They must be converted to
     * UTF-8 byte ranges before sending to the Rust kernel, which operates exclusively in
     * UTF-8 half-open intervals. The conversion goes through [TextOffsetIndex] which
     * snaps to code-point boundaries.
     */
    override fun deleteSurroundingText(
        beforeLength: Int,
        afterLength: Int,
    ): Boolean {
        if (beforeLength == 0 && afterLength == 0) return true
        if (adapter.isComposing()) {
            return handleCompositionDelete(beforeLength, afterLength)
        }
        val selAnchorUtf8 = mirror.getSelectionAnchorUtf8()
        val selHeadUtf8 = mirror.getSelectionHeadUtf8()
        val (selMin, selMax) =
            if (selAnchorUtf8 <= selHeadUtf8) {
                Pair(
                    selAnchorUtf8,
                    selHeadUtf8,
                )
            } else {
                Pair(selHeadUtf8, selAnchorUtf8)
            }

        val selMinUtf16 = realUtf8ToDisplayUtf16(selMin)
        val selMaxUtf16 = realUtf8ToDisplayUtf16(selMax)

        val projection = projectionProvider?.invoke()
        val displayLen = projection?.displayLengthUtf16 ?: mirror.getLengthUtf16()

        val deleteStartUtf16 = (selMinUtf16 - beforeLength).coerceAtLeast(0)
        val deleteEndUtf16 = (selMaxUtf16 + afterLength).coerceAtMost(displayLen)

        if (deleteStartUtf16 >= deleteEndUtf16) return false

        val beforeStartUtf8 = displayUtf16ToRealUtf8(deleteStartUtf16)
        val beforeEndUtf8 = selMin
        val afterStartUtf8 = selMax
        val afterEndUtf8 = displayUtf16ToRealUtf8(deleteEndUtf16)

        val hasBefore = beforeStartUtf8 < beforeEndUtf8
        val hasAfter = afterStartUtf8 < afterEndUtf8

        if (!hasBefore && !hasAfter) return false

        adapter.sendDeleteSurroundingToKernel(
            if (hasBefore) beforeStartUtf8 else 0,
            if (hasBefore) beforeEndUtf8 else 0,
            if (hasAfter) afterStartUtf8 else 0,
            if (hasAfter) afterEndUtf8 else 0,
            EditorTransactionCauseDto.DELETE,
        )
        notifySelectionChanged()
        return true
    }

    /**
     * Delete text around the cursor. [beforeLength]/[afterLength] are in code points
     * (not UTF-16 code units like [deleteSurroundingText]). Each code point may span
     * multiple UTF-16 surrogates; [String.offsetByCodePoints] handles the traversal.
     * The resulting UTF-16 range is then converted to UTF-8 byte ranges for the kernel.
     */
    override fun deleteSurroundingTextInCodePoints(
        beforeLength: Int,
        afterLength: Int,
    ): Boolean {
        if (beforeLength == 0 && afterLength == 0) return true
        if (adapter.isComposing()) {
            return handleCompositionDeleteCodePoints(beforeLength, afterLength)
        }
        val selAnchorUtf8 = mirror.getSelectionAnchorUtf8()
        val selHeadUtf8 = mirror.getSelectionHeadUtf8()
        val (selMin, selMax) =
            if (selAnchorUtf8 <= selHeadUtf8) {
                Pair(
                    selAnchorUtf8,
                    selHeadUtf8,
                )
            } else {
                Pair(selHeadUtf8, selAnchorUtf8)
            }

        val selMinUtf16 = realUtf8ToDisplayUtf16(selMin)
        val selMaxUtf16 = realUtf8ToDisplayUtf16(selMax)
        val displayText: CharSequence = projectionProvider?.invoke()?.displayText ?: mirror.getSpannable()

        var deleteStartUtf16 = selMinUtf16
        var count = beforeLength
        while (count > 0 && deleteStartUtf16 > 0) {
            deleteStartUtf16 = Character.offsetByCodePoints(displayText, deleteStartUtf16, -1)
            count--
        }

        var deleteEndUtf16 = selMaxUtf16
        count = afterLength
        while (count > 0 && deleteEndUtf16 < displayText.length) {
            deleteEndUtf16 = Character.offsetByCodePoints(displayText, deleteEndUtf16, 1)
            count--
        }

        if (deleteStartUtf16 >= deleteEndUtf16) return false

        val beforeStartUtf8 = displayUtf16ToRealUtf8(deleteStartUtf16)
        val beforeEndUtf8 = selMin
        val afterStartUtf8 = selMax
        val afterEndUtf8 = displayUtf16ToRealUtf8(deleteEndUtf16)

        val hasBefore = beforeStartUtf8 < beforeEndUtf8
        val hasAfter = afterStartUtf8 < afterEndUtf8

        if (!hasBefore && !hasAfter) return false

        adapter.sendDeleteSurroundingToKernel(
            if (hasBefore) beforeStartUtf8 else 0,
            if (hasBefore) beforeEndUtf8 else 0,
            if (hasAfter) afterStartUtf8 else 0,
            if (hasAfter) afterEndUtf8 else 0,
            EditorTransactionCauseDto.DELETE,
        )
        notifySelectionChanged()
        return true
    }

    private fun handleCompositionDelete(
        beforeLength: Int,
        afterLength: Int,
    ): Boolean {
        val compositionText = adapter.getCompositionText()
        if (compositionText.isEmpty()) return true
        val cursorInComposition = adapter.getCompositionCursorOffset() ?: compositionText.length

        val deleteStartInComp = (cursorInComposition - beforeLength).coerceAtLeast(0)
        val deleteEndInComp = (cursorInComposition + afterLength).coerceAtMost(compositionText.length)
        if (deleteStartInComp >= deleteEndInComp) return false

        val newPreedit = compositionText.removeRange(deleteStartInComp, deleteEndInComp)
        if (newPreedit.isEmpty()) {
            adapter.handleCompositionCancel()
        } else {
            val newCursor = deleteStartInComp
            adapter.handleCompositionUpdate(newPreedit, newCursor + 1)
        }
        notifySelectionChanged()
        return true
    }

    private fun handleCompositionDeleteCodePoints(
        beforeLength: Int,
        afterLength: Int,
    ): Boolean {
        val compositionText = adapter.getCompositionText()
        if (compositionText.isEmpty()) return true
        val cursorInComposition = adapter.getCompositionCursorOffset() ?: compositionText.length

        var deleteStart = cursorInComposition
        var count = beforeLength
        while (count > 0 && deleteStart > 0) {
            deleteStart = compositionText.offsetByCodePoints(deleteStart, -1)
            count--
        }

        var deleteEnd = cursorInComposition
        count = afterLength
        while (count > 0 && deleteEnd < compositionText.length) {
            deleteEnd = compositionText.offsetByCodePoints(deleteEnd, 1)
            count--
        }

        if (deleteStart >= deleteEnd) return false

        val newPreedit = compositionText.removeRange(deleteStart, deleteEnd)
        if (newPreedit.isEmpty()) {
            adapter.handleCompositionCancel()
        } else {
            adapter.handleCompositionUpdate(newPreedit, deleteStart + 1)
        }
        notifySelectionChanged()
        return true
    }

    override fun setComposingText(
        text: CharSequence?,
        newCursorPosition: Int,
    ): Boolean {
        if (text == null) return true
        if (adapter.shouldForbidNewline(text.toString())) {
            return true
        }
        adapter.handleCompositionUpdate(text.toString(), newCursorPosition)
        notifySelectionChanged()
        return true
    }

    override fun finishComposingText(): Boolean {
        if (adapter.isComposing()) {
            adapter.handleCompositionFinish()
        }
        return true
    }

    /**
     * Enter composing mode on an existing text region (vs. setComposingText which starts
     * from scratch). [start]/[end] are UTF-16 offsets from the InputConnection API;
     * converted to UTF-8 byte offsets for the Rust kernel via [TextOffsetIndex].
     * The selected text becomes the initial preedit, and the mirror's composition overlay
     * is set up immediately so the IME sees the composing region.
     *
     * Contract per Android's InputConnection API:
     * - Negative offsets are rejected (returns false).
     * - A reversed range (start > end) is accepted and normalized to [min, max).
     * - The call does NOT notify InputMethodManager.updateSelection: unlike commitText /
     *   setComposingText / setSelection / deleteSurrounding, the committed text and the
     *   selection do not change here, so an extra updateSelection would only re-enter the
     *   IME recorrection loop (an IME mirrors the selection back as setComposingRegion).
     *
     * Composition validity is governed by the InputConnection lifecycle, the kernel
     * composition session (session id / base revision / generation) and the adapter's
     * composition state machine — no IME enumeration or enabled-IME gate is used.
     */
    override fun setComposingRegion(
        start: Int,
        end: Int,
    ): Boolean {
        if (start < 0 || end < 0) return false
        val normStart = minOf(start, end)
        val normEnd = maxOf(start, end)
        if (adapter.isComposing()) {
            adapter.handleCompositionCancel()
        }
        val byteStart = displayUtf16ToRealUtf8(normStart)
        val byteEnd = displayUtf16ToRealUtf8(normEnd)
        val selectedText = mirror.committedSliceUtf8(byteStart, byteEnd)
        val beginOk = adapter.sendBeginCompositionToKernel(byteStart, byteEnd)
        if (!beginOk) {
            commandPort.reloadFromKernel()
            return false
        }
        adapter.startComposingRegion(byteStart, byteEnd, selectedText)
        mirror.updateComposition(byteStart, byteEnd, selectedText)
        return true
    }

    override fun setSelection(
        start: Int,
        end: Int,
    ): Boolean {
        if (start < 0 || end < 0) return false
        val anchorUtf8 = displayUtf16ToRealUtf8(start)
        val headUtf8 = displayUtf16ToRealUtf8(end)
        if (adapter.isComposing()) {
            mirror.setSelectionInternal(anchorUtf8, headUtf8)
            val (sessionId, _, generation) = adapter.compositionSessionInfo()
            if (sessionId != 0L) {
                val projection = projectionProvider?.invoke()
                val compRangeUtf16 = mirror.getCompositionRangeUtf16()
                val preeditCursorUtf16 =
                    if (compRangeUtf16 != null && projection != null) {
                        val compStartDisplayUtf16 = projection.realUtf16ToDisplayUtf16(compRangeUtf16.first)
                        val compEndDisplayUtf16 = projection.realUtf16ToDisplayUtf16(compRangeUtf16.second)
                        val preeditUtf16Len = adapter.getCompositionText().length
                        when {
                            start < compStartDisplayUtf16 -> 0
                            start > compEndDisplayUtf16 -> preeditUtf16Len
                            else -> start - compStartDisplayUtf16
                        }
                    } else if (compRangeUtf16 != null) {
                        val preeditUtf16Len = adapter.getCompositionText().length
                        when {
                            start < compRangeUtf16.first -> 0
                            start > compRangeUtf16.second -> preeditUtf16Len
                            else -> start - compRangeUtf16.first
                        }
                    } else {
                        0
                    }
                val dto =
                    commandPort.updateComposition(
                        sessionId,
                        generation,
                        adapter.getCompositionText(),
                        preeditCursorUtf16,
                    )
                if (dto != null) {
                    val result = EditResult.fromDto(dto)
                    if (result.isApplied()) {
                        adapter.syncCompositionGeneration()
                    } else {
                        adapter.invalidateCompositionSession()
                        commandPort.reloadFromKernel()
                    }
                } else {
                    adapter.invalidateCompositionSession()
                    commandPort.reloadFromKernel()
                }
            }
        } else {
            adapter.sendSetSelectionToKernel(anchorUtf8, headUtf8)
        }
        notifySelectionChanged()
        return true
    }

    override fun getTextBeforeCursor(
        n: Int,
        flags: Int,
    ): CharSequence {
        val projection = projectionProvider?.invoke()
        if (projection != null) {
            val cursorDisplayUtf16 = projection.realUtf8ToDisplayUtf16(mirror.getCursorUtf8())
            val start = (cursorDisplayUtf16 - n).coerceAtLeast(0)
            return projection.displayText.subSequence(
                start,
                cursorDisplayUtf16.coerceAtMost(projection.displayText.length),
            ).toString()
        }
        val cursorUtf16 = mirror.getCursorUtf16()
        val start = (cursorUtf16 - n).coerceAtLeast(0)
        val spannable = mirror.getSpannable()
        val end = cursorUtf16.coerceAtMost(spannable.length)
        return spannable.subSequence(start, end).toString()
    }

    override fun getTextAfterCursor(
        n: Int,
        flags: Int,
    ): CharSequence {
        val projection = projectionProvider?.invoke()
        if (projection != null) {
            val cursorDisplayUtf16 = projection.realUtf8ToDisplayUtf16(mirror.getCursorUtf8())
            val end = (cursorDisplayUtf16 + n).coerceAtMost(projection.displayText.length)
            return projection.displayText.subSequence(
                cursorDisplayUtf16.coerceAtMost(projection.displayText.length),
                end,
            ).toString()
        }
        val cursorUtf16 = mirror.getCursorUtf16()
        val spannable = mirror.getSpannable()
        val cursor = cursorUtf16.coerceAtMost(spannable.length)
        val end = (cursorUtf16 + n).coerceAtMost(spannable.length)
        return spannable.subSequence(cursor, end).toString()
    }

    override fun getSelectedText(flags: Int): CharSequence? {
        val projection = projectionProvider?.invoke()
        if (projection != null) {
            val selStartDisplayUtf16 = projection.realUtf8ToDisplayUtf16(mirror.getSelectionStartUtf8())
            val selEndDisplayUtf16 = projection.realUtf8ToDisplayUtf16(mirror.getSelectionEndUtf8())
            if (selStartDisplayUtf16 < 0 || selEndDisplayUtf16 < 0 ||
                selStartDisplayUtf16 == selEndDisplayUtf16
            ) {
                return null
            }
            val start = selStartDisplayUtf16.coerceAtMost(projection.displayText.length)
            val end = selEndDisplayUtf16.coerceAtMost(projection.displayText.length)
            return projection.displayText.subSequence(start.coerceAtMost(end), end.coerceAtLeast(start)).toString()
        }
        val selStart = mirror.getSelectionStartUtf16()
        val selEnd = mirror.getSelectionEndUtf16()
        if (selStart < 0 || selEnd < 0 || selStart == selEnd) return null
        val spannable = mirror.getSpannable()
        val len = spannable.length
        val start = selStart.coerceAtMost(len)
        val end = selEnd.coerceAtMost(len)
        return spannable.subSequence(start.coerceAtMost(end), end.coerceAtLeast(start)).toString()
    }

    private fun notifySelectionChanged() {
        val imm =
            hostView.context.getSystemService(android.content.Context.INPUT_METHOD_SERVICE)
                as? android.view.inputmethod.InputMethodManager ?: return
        val projection = projectionProvider?.invoke()
        val selStart: Int
        val selEnd: Int
        if (projection != null) {
            selStart = projection.realUtf8ToDisplayUtf16(mirror.getSelectionStartUtf8())
            selEnd = projection.realUtf8ToDisplayUtf16(mirror.getSelectionEndUtf8())
        } else {
            selStart = mirror.getSelectionStartUtf16()
            selEnd = mirror.getSelectionEndUtf16()
        }
        val compRange = mirror.getCompositionRangeUtf16()
        val candidatesStart: Int
        val candidatesEnd: Int
        if (compRange != null && projection != null) {
            candidatesStart = projection.realUtf16ToDisplayUtf16(compRange.first)
            candidatesEnd = projection.realUtf16ToDisplayUtf16(compRange.second)
        } else {
            candidatesStart = compRange?.first ?: -1
            candidatesEnd = compRange?.second ?: -1
        }
        imm.updateSelection(hostView, selStart, selEnd, candidatesStart, candidatesEnd)
    }
}
