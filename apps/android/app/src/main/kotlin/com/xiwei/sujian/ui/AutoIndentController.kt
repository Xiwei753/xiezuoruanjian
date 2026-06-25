package com.xiwei.sujian.ui

import android.text.Editable
import android.text.Spanned
import android.text.style.LeadingMarginSpan
import android.util.Log
import android.view.inputmethod.BaseInputConnection
import android.widget.EditText
import com.xiwei.sujian.ui.span.EmptyParagraphIndentSpan

class AutoIndentController(private val editText: EditText) {

    private val TAG = "WriterAutoIndent"

    var autoIndentEnabled: Boolean = false
        private set
    var autoIndentPx: Int = 0
        private set

    var isUpdatingSpan = false
        private set

    var isSuppressing = false

    var pendingFullRebuildAfterComposition = false
        private set

    private fun currentEmptyIndentSpans(editable: Editable): List<EmptyParagraphIndentSpan> {
        val spans = mutableListOf<EmptyParagraphIndentSpan>()
        spans.addAll(editable.getSpans(0, editable.length, EmptyParagraphIndentSpan::class.java))
        for (span in editable.getSpans(editable.length, editable.length, EmptyParagraphIndentSpan::class.java)) {
            if (!spans.contains(span)) spans.add(span)
        }
        return spans
    }

    private fun currentNormalIndentSpans(editable: Editable): List<LeadingMarginSpan.Standard> {
        val spans = mutableListOf<LeadingMarginSpan.Standard>()
        spans.addAll(
            editable.getSpans(0, editable.length, LeadingMarginSpan.Standard::class.java)
                .filter { it !is EmptyParagraphIndentSpan }
        )
        for (span in editable.getSpans(editable.length, editable.length, LeadingMarginSpan.Standard::class.java)) {
            if (span !is EmptyParagraphIndentSpan && !spans.contains(span)) spans.add(span)
        }
        return spans
    }

    var isComposingActive = false
        private set

    private val delayedFullRebuildRunnable = Runnable {
        val editable = editText.text ?: return@Runnable
        updateParagraphIndentSpans(editable, isFullRebuild = true)
    }

    fun markComposingActive() {
        isComposingActive = true
        Log.d(TAG, "markComposingActive: composing started")
    }

    fun markComposingFinished() {
        isComposingActive = false
        pendingFullRebuildAfterComposition = true
        Log.d(TAG, "markComposingFinished: composing ended, pending rebuild")
    }

    fun setAutoIndent(enabled: Boolean, widthChars: Float) {
        val oldEnabled = this.autoIndentEnabled
        val oldPx = this.autoIndentPx

        this.autoIndentEnabled = enabled
        if (enabled && widthChars > 0) {
            val emWidth = editText.paint.measureText("中")
            this.autoIndentPx = (emWidth * widthChars).toInt()
        } else {
            this.autoIndentPx = 0
        }

        if (oldEnabled != this.autoIndentEnabled || oldPx != this.autoIndentPx) {
            Log.d(TAG, "setAutoIndent: enabled=$enabled, px=${this.autoIndentPx}, triggering full rebuild")
            val editable = editText.text
            if (editable != null) {
                updateParagraphIndentSpans(editable, isFullRebuild = true)
            }
        }
    }

    fun updateParagraphIndentSpans(editable: Editable, updateStartPos: Int = -1, isFullRebuild: Boolean = false) {
        if (isSuppressing) return

        val composingStart = BaseInputConnection.getComposingSpanStart(editable)
        val composingEnd = BaseInputConnection.getComposingSpanEnd(editable)
        val isComposing = composingStart != -1 && composingEnd != -1

        if (isComposing || isComposingActive) {
            pendingFullRebuildAfterComposition = true
            Log.d(TAG, "updateParagraphIndentSpans: composing active, deferring")
            return
        }

        if (pendingFullRebuildAfterComposition && !isFullRebuild) {
            updateParagraphIndentSpans(editable, isFullRebuild = true)
            return
        }

        if (isFullRebuild) {
            pendingFullRebuildAfterComposition = false
        }

        if (!autoIndentEnabled || autoIndentPx <= 0) {
            val emptySpans = currentEmptyIndentSpans(editable)
            val normalSpans = currentNormalIndentSpans(editable)
            val allSpans = (emptySpans + normalSpans).distinct()
            if (allSpans.isNotEmpty()) {
                isUpdatingSpan = true
                for (span in allSpans) {
                    if (editable.getSpanStart(span) >= 0) {
                        editable.removeSpan(span)
                    }
                }
                isUpdatingSpan = false
            }
            return
        }

        isUpdatingSpan = true
        try {
            val existingEmptySpans = currentEmptyIndentSpans(editable)
            val existingNormalSpans = currentNormalIndentSpans(editable)

            var paragraphStart = 0

            if (!isFullRebuild && updateStartPos >= 0) {
                val safeStart = updateStartPos.coerceIn(0, editable.length)
                val prevNewline = editable.lastIndexOf('\n', safeStart - 1)
                paragraphStart = if (prevNewline == -1) 0 else prevNewline + 1
            }

            val textLength = editable.length

            val emptySpansToRemove = mutableListOf<EmptyParagraphIndentSpan>()
            for (span in existingEmptySpans) {
                val start = editable.getSpanStart(span)
                if (start >= paragraphStart) {
                    emptySpansToRemove.add(span)
                }
            }

            val normalSpansToRemove = mutableListOf<LeadingMarginSpan.Standard>()
            for (span in existingNormalSpans) {
                val start = editable.getSpanStart(span)
                if (start >= paragraphStart) {
                    normalSpansToRemove.add(span)
                }
            }

            while (paragraphStart <= textLength) {
                var newlinePos = editable.indexOf('\n', paragraphStart)
                val paragraphEnd: Int
                val isTrailingEmptyParagraph: Boolean

                if (newlinePos == -1) {
                    paragraphEnd = textLength
                    isTrailingEmptyParagraph = (paragraphStart == textLength)
                } else {
                    paragraphEnd = newlinePos + 1
                    isTrailingEmptyParagraph = (paragraphStart == newlinePos)
                }

                val isEmptyParagraph = isTrailingEmptyParagraph ||
                    (paragraphEnd - paragraphStart == 0)

                if (isEmptyParagraph) {
                    val existingMarker = existingEmptySpans.firstOrNull {
                        editable.getSpanStart(it) == paragraphStart
                    }
                    if (existingMarker != null) {
                        emptySpansToRemove.remove(existingMarker)
                        normalSpansToRemove.remove(existingMarker)
                        if (editable.getSpanEnd(existingMarker) != paragraphEnd ||
                            existingMarker.getLeadingMargin(true) != autoIndentPx) {
                            editable.removeSpan(existingMarker)
                            val newMarker = EmptyParagraphIndentSpan(autoIndentPx, 0)
                            editable.setSpan(
                                newMarker,
                                paragraphStart, paragraphEnd,
                                Spanned.SPAN_PARAGRAPH
                            )
                            Log.d(TAG, "updateParagraphIndentSpans: updated empty marker span at [$paragraphStart, $paragraphEnd)")
                        }
                    } else {
                        val existingNormal = existingNormalSpans.firstOrNull {
                            editable.getSpanStart(it) == paragraphStart
                        }
                        if (existingNormal != null) {
                            normalSpansToRemove.remove(existingNormal)
                            editable.removeSpan(existingNormal)
                        }
                        val newMarker = EmptyParagraphIndentSpan(autoIndentPx, 0)
                        editable.setSpan(
                            newMarker,
                            paragraphStart, paragraphEnd,
                            Spanned.SPAN_PARAGRAPH
                        )
                        Log.d(TAG, "updateParagraphIndentSpans: set empty marker span at [$paragraphStart, $paragraphEnd)")
                    }
                    if (paragraphEnd >= textLength && !isTrailingEmptyParagraph) break
                    if (isTrailingEmptyParagraph && paragraphEnd >= textLength) {
                        if (paragraphEnd == textLength && textLength > 0 && editable[textLength - 1] == '\n' && paragraphStart < textLength) {
                            paragraphStart = textLength
                            continue
                        }
                        break
                    }
                    paragraphStart = paragraphEnd
                    continue
                }

                val existingMarkerHere = existingEmptySpans.firstOrNull {
                    editable.getSpanStart(it) == paragraphStart
                }
                if (existingMarkerHere != null) {
                    emptySpansToRemove.remove(existingMarkerHere)
                    val spanStart = editable.getSpanStart(existingMarkerHere)
                    if (spanStart >= 0) {
                        editable.removeSpan(existingMarkerHere)
                    }
                }

                val span = existingNormalSpans.firstOrNull {
                    editable.getSpanStart(it) == paragraphStart &&
                        editable.getSpanEnd(it) == paragraphEnd &&
                        it.getLeadingMargin(true) == autoIndentPx
                }

                if (span != null) {
                    normalSpansToRemove.remove(span)
                } else {
                    editable.setSpan(
                        LeadingMarginSpan.Standard(autoIndentPx, 0),
                        paragraphStart, paragraphEnd,
                        Spanned.SPAN_EXCLUSIVE_EXCLUSIVE
                    )
                    Log.d(TAG, "updateParagraphIndentSpans: set span at [$paragraphStart, $paragraphEnd)")
                }

                if (paragraphEnd >= textLength) {
                    if (paragraphEnd == textLength && textLength > 0 && editable[textLength - 1] == '\n' && paragraphStart < textLength) {
                        paragraphStart = textLength
                        continue
                    }
                    break
                }
                paragraphStart = paragraphEnd
            }

            for (span in emptySpansToRemove) {
                val spanStart = editable.getSpanStart(span)
                if (spanStart >= 0) {
                    editable.removeSpan(span)
                }
            }

            for (span in normalSpansToRemove) {
                if (editable.getSpanStart(span) >= 0) {
                    editable.removeSpan(span)
                }
            }
        } finally {
            isUpdatingSpan = false
        }
    }

    fun requestDelayedFullRebuild() {
        if (isSuppressing) return
        editText.removeCallbacks(delayedFullRebuildRunnable)
        editText.post {
            editText.removeCallbacks(delayedFullRebuildRunnable)
            editText.post(delayedFullRebuildRunnable)
        }
    }
}
