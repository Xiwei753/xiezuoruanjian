package com.xiwei.sujian.ui

import android.text.Editable
import android.text.Spanned
import android.text.style.LeadingMarginSpan
import android.view.inputmethod.BaseInputConnection
import android.widget.EditText

/**
 * AutoIndentController — 自动首行缩进控制器
 *
 * 使用 LeadingMarginSpan 实现段落的首行缩进效果。
 *
 * ## 架构定位
 * - WriterEditText → AutoIndentController → LeadingMarginSpan
 *
 * ## 职责边界
 * - **做**：首行缩进的 Span 管理、新段落自动缩进
 * - **不做**：文本内容管理（由 EditText 负责）
 *
 * ## 使用场景
 * - 用户输入时自动为新段落添加首行缩进
 * - 支持动态调整缩进宽度
 */
class AutoIndentController(private val editText: EditText) {

    var autoIndentEnabled: Boolean = false
        private set
    var autoIndentPx: Int = 0
        private set

    var isUpdatingSpan = false
        private set

    var isSuppressing = false

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
            val editable = editText.text
            if (editable != null) {
                updateParagraphIndentSpans(editable, isFullRebuild = true)
            }
        }
    }

    fun updateParagraphIndentSpans(editable: Editable, updateStartPos: Int = -1, isFullRebuild: Boolean = false) {
        if (isSuppressing) return

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

            val composingStart = BaseInputConnection.getComposingSpanStart(editable)
            val composingEnd = BaseInputConnection.getComposingSpanEnd(editable)
            val isComposing = composingStart != -1 && composingEnd != -1

            while (paragraphStart <= textLength) {
                var paragraphEnd = editable.indexOf('\n', paragraphStart)
                if (paragraphEnd == -1) {
                    paragraphEnd = textLength
                } else {
                    paragraphEnd += 1
                }

                val overlapsComposing = isComposing && paragraphEnd > composingStart && paragraphStart < composingEnd
                val span = existingSpans.firstOrNull {
                    editable.getSpanStart(it) == paragraphStart &&
                        editable.getSpanEnd(it) == paragraphEnd &&
                        it.getLeadingMargin(true) == autoIndentPx
                }

                if (overlapsComposing) {
                    if (span != null) spansToRemove.remove(span)
                } else {
                    if (span != null) {
                        spansToRemove.remove(span)
                    } else {
                        editable.setSpan(LeadingMarginSpan.Standard(autoIndentPx, 0), paragraphStart, paragraphEnd, Spanned.SPAN_PARAGRAPH)
                    }
                }

                if (paragraphEnd >= textLength) break
                paragraphStart = paragraphEnd
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
