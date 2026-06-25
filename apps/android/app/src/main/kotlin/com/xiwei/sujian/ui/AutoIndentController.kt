package com.xiwei.sujian.ui

import android.text.Editable
import android.text.Spanned
import android.text.style.LeadingMarginSpan
import android.util.Log
import android.view.inputmethod.BaseInputConnection
import android.widget.EditText

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

    private var isComposingActive = false
        private set

    private val emptyParagraphMarkerSpans = mutableMapOf<Int, LeadingMarginSpan.Standard>()

    private val delayedFullRebuildRunnable = Runnable {
        val editable = editText.text ?: return@Runnable
        updateParagraphIndentSpans(editable, isFullRebuild = true)
    }

    private fun currentIndentSpans(editable: Editable): List<LeadingMarginSpan.Standard> {
        val spans = mutableListOf<LeadingMarginSpan.Standard>()
        spans.addAll(editable.getSpans(0, editable.length, LeadingMarginSpan.Standard::class.java))
        for (span in editable.getSpans(editable.length, editable.length, LeadingMarginSpan.Standard::class.java)) {
            if (!spans.contains(span)) spans.add(span)
        }
        return spans
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
            val existingSpans = currentIndentSpans(editable)
            if (existingSpans.isNotEmpty()) {
                isUpdatingSpan = true
                for (span in existingSpans) {
                    if (editable.getSpanStart(span) >= 0) {
                        editable.removeSpan(span)
                    }
                }
                isUpdatingSpan = false
            }
            emptyParagraphMarkerSpans.clear()
            return
        }

        isUpdatingSpan = true
        try {
            val existingSpans = currentIndentSpans(editable)
            var paragraphStart = 0

            if (!isFullRebuild && updateStartPos >= 0) {
                val safeStart = updateStartPos.coerceIn(0, editable.length)
                val prevNewline = editable.lastIndexOf('\n', safeStart - 1)
                paragraphStart = if (prevNewline == -1) 0 else prevNewline + 1
            }

            val textLength = editable.length
            val spansToRemove = mutableListOf<LeadingMarginSpan.Standard>()

            for (span in existingSpans) {
                val start = editable.getSpanStart(span)
                if (start >= paragraphStart) {
                    spansToRemove.add(span)
                }
            }

            val markerKeysToRemove = mutableListOf<Int>()
            for ((pos, _) in emptyParagraphMarkerSpans) {
                if (pos >= paragraphStart) {
                    markerKeysToRemove.add(pos)
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
                    val markerSpan = emptyParagraphMarkerSpans[paragraphStart]
                    if (markerSpan != null) {
                        markerKeysToRemove.remove(paragraphStart)
                        spansToRemove.remove(markerSpan)
                        if (editable.getSpanEnd(markerSpan) != paragraphEnd ||
                            markerSpan.getLeadingMargin(true) != autoIndentPx) {
                            editable.removeSpan(markerSpan)
                            val newMarker = LeadingMarginSpan.Standard(autoIndentPx, 0)
                            emptyParagraphMarkerSpans[paragraphStart] = newMarker
                            editable.setSpan(
                                newMarker,
                                paragraphStart, paragraphEnd,
                                Spanned.SPAN_INCLUSIVE_INCLUSIVE
                            )
                            Log.d(TAG, "updateParagraphIndentSpans: updated empty marker span at [$paragraphStart, $paragraphEnd)")
                        }
                    } else {
                        val existingAtEmpty = existingSpans.firstOrNull {
                            editable.getSpanStart(it) == paragraphStart
                        }
                        if (existingAtEmpty != null) {
                            spansToRemove.remove(existingAtEmpty)
                            editable.removeSpan(existingAtEmpty)
                        }
                        val newMarker = LeadingMarginSpan.Standard(autoIndentPx, 0)
                        emptyParagraphMarkerSpans[paragraphStart] = newMarker
                        editable.setSpan(
                            newMarker,
                            paragraphStart, paragraphEnd,
                            Spanned.SPAN_INCLUSIVE_INCLUSIVE
                        )
                        Log.d(TAG, "updateParagraphIndentSpans: set empty marker span at [$paragraphStart, $paragraphEnd)")
                    }
                    if (paragraphEnd >= textLength && !isTrailingEmptyParagraph) break
                    if (isTrailingEmptyParagraph && paragraphEnd >= textLength) break
                    paragraphStart = paragraphEnd
                    continue
                }

                val markerAtPos = emptyParagraphMarkerSpans.remove(paragraphStart)
                if (markerAtPos != null) {
                    val spanStart = editable.getSpanStart(markerAtPos)
                    if (spanStart >= 0) {
                        editable.removeSpan(markerAtPos)
                    }
                }

                val span = existingSpans.firstOrNull {
                    editable.getSpanStart(it) == paragraphStart &&
                        editable.getSpanEnd(it) == paragraphEnd &&
                        it.getLeadingMargin(true) == autoIndentPx
                }

                if (span != null) {
                    spansToRemove.remove(span)
                } else {
                    editable.setSpan(
                        LeadingMarginSpan.Standard(autoIndentPx, 0),
                        paragraphStart, paragraphEnd,
                        Spanned.SPAN_EXCLUSIVE_EXCLUSIVE
                    )
                    Log.d(TAG, "updateParagraphIndentSpans: set span at [$paragraphStart, $paragraphEnd)")
                }

                if (paragraphEnd >= textLength) break
                paragraphStart = paragraphEnd
            }

            for (key in markerKeysToRemove) {
                val marker = emptyParagraphMarkerSpans.remove(key)
                if (marker != null) {
                    val spanStart = editable.getSpanStart(marker)
                    if (spanStart >= 0) {
                        editable.removeSpan(marker)
                    }
                }
            }

            for (span in spansToRemove) {
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
