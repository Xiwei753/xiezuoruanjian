package com.xiwei.sujian.ui

import android.animation.Animator
import android.animation.AnimatorListenerAdapter
import android.animation.ValueAnimator
import android.text.Editable
import android.view.inputmethod.BaseInputConnection

/**
 * TypingAnimationController — 打字动画控制器
 *
 * 监听 EditText 的文本变化，触发动画效果并管理动画生命周期。
 *
 * ## 架构定位
 * - WriterEditText → TypingAnimationController → TypingOverlayRenderer
 *
 * ## 职责边界
 * - **做**：文本变化监听、动画触发、动画参数管理
 * - **不做**：动画渲染（由 TypingOverlayRenderer 负责）
 *
 * ## 使用场景
 * - 用户输入字符时触发动画
 * - 粘贴/删除时抑制动画
 */
class TypingAnimationController(
    private val editText: WriterEditText,
    private val renderLayer: EditorRenderLayer
) {
    private val DEBUG_ANIM = false
    private val TAG = "WriterEditorAnim"

    var typingAnimationEnabled = false
        private set
    var typingAnimationDurationMs: Long = 100L
        private set

    var isSuppressAnimations = false
    var isScrollAnimationsSuppressed = false
        set(value) {
            field = value
            if (value) {
                lastAddedStart = -1
                lastAddedCount = 0
                renderLayer.clear()
            }
        }

    private var lastAddedStart = -1
    private var lastAddedCount = 0
    private var cursorBeforeX = -1f
    private var cursorBeforeY = -1f

    private var isPasteOrDelete = false

    fun setTypingAnimationEnabled(enabled: Boolean, durationMs: Long = 100L) {
        typingAnimationEnabled = enabled
        typingAnimationDurationMs = durationMs
    }

    fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {
        if (isScrollAnimationsSuppressed) {
            lastAddedStart = -1
            lastAddedCount = 0
            return
        }
        if (isSuppressAnimations) {
            renderLayer.clear()
            if (DEBUG_ANIM) {
                android.util.Log.d(TAG, "beforeTextChanged - suppressed animation")
            }
            return
        }

        val editable = editText.text
        var matchesComposing = false
        if (editable != null && count > 0) {
            val compStart = BaseInputConnection.getComposingSpanStart(editable)
            val compEnd = BaseInputConnection.getComposingSpanEnd(editable)
            if (compStart != -1 && compEnd != -1) {
                if (start >= compStart && start + count <= compEnd) {
                    matchesComposing = true
                }
            }
        }

        isPasteOrDelete = when {
            after > 3 -> true
            count > 3 -> true
            count > 0 && after > 0 && !matchesComposing -> true
            else -> false
        }

        if ((after > 0 || count > 0) && editText.layout != null) {
            val line = editText.layout.getLineForOffset(start)
            cursorBeforeX = editText.layout.getPrimaryHorizontal(start)
            cursorBeforeY = editText.layout.getLineBaseline(line).toFloat()
        }

        if (count > 0 && count <= 3 && after == 0 && !matchesComposing && typingAnimationEnabled && typingAnimationDurationMs > 0 && s != null) {
            val deletedText = s.subSequence(start, start + count).toString()
            if (deletedText.contains('\n') || deletedText.contains('\r')) return
            val anim = OverlayAnim(
                insertedStart = start,
                insertedText = deletedText,
                startX = cursorBeforeX,
                startY = cursorBeforeY,
                endX = cursorBeforeX,
                endY = cursorBeforeY,
                progress = 0f,
                startTimeNanos = -1L,
                durationMs = typingAnimationDurationMs,
                hiddenSpan = null,
                isDeletion = true
            )
            renderLayer.addTypingAnim(anim)
        }

        if (DEBUG_ANIM) {
            android.util.Log.d(TAG, "beforeTextChanged - replaced: $count, after: $after, cursor: ($cursorBeforeX, $cursorBeforeY)")
        }
    }

    fun onTextChanged(start: Int, count: Int) {
        if (isSuppressAnimations || isScrollAnimationsSuppressed) {
            lastAddedStart = -1
            lastAddedCount = 0
            return
        }

        if (!isPasteOrDelete && count > 0) {
            lastAddedStart = start
            lastAddedCount = count
        } else {
            lastAddedStart = -1
            lastAddedCount = 0
        }
    }

    fun afterTextChanged(editable: Editable?) {
        if (editable == null) return
        if (isSuppressAnimations || isScrollAnimationsSuppressed) return

        val composingStart = BaseInputConnection.getComposingSpanStart(editable)
        val composingEnd = BaseInputConnection.getComposingSpanEnd(editable)
        val isComposing = composingStart != -1 && composingEnd != -1

        if (isComposing && lastAddedStart >= composingStart && lastAddedStart + lastAddedCount <= composingEnd) {
             if (DEBUG_ANIM) android.util.Log.d(TAG, "afterTextChanged - skipping animation for composing text.")
             lastAddedStart = -1
             lastAddedCount = 0
             return
        }

        if (typingAnimationEnabled && lastAddedCount in 1..3 && lastAddedStart >= 0) {
            val start = lastAddedStart
            val end = kotlin.math.min(start + lastAddedCount, editable.length)

            if (end > start && typingAnimationDurationMs > 0) {
                val insertedText = editable.subSequence(start, end).toString()
                if (insertedText.contains('\n') || insertedText.contains('\r')) {
                    lastAddedStart = -1
                    lastAddedCount = 0
                    return
                }

                val hiddenSpan = android.text.style.ForegroundColorSpan(android.graphics.Color.TRANSPARENT)
                editText.isUpdatingSpanWrapper = true
                editable.setSpan(hiddenSpan, start, end, android.text.Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
                editText.isUpdatingSpanWrapper = false

                val anim = OverlayAnim(
                    insertedStart = start,
                    insertedText = insertedText,
                    startX = cursorBeforeX,
                    startY = cursorBeforeY,
                    progress = 0f,
                    startTimeNanos = -1L,
                    durationMs = typingAnimationDurationMs,
                    hiddenSpan = hiddenSpan
                )

                renderLayer.addTypingAnim(anim)

                if (DEBUG_ANIM) {
                    android.util.Log.d(TAG, "afterTextChanged - created anim: insertedStart=${anim.insertedStart}, length=${insertedText.length}")
                }
            }
            lastAddedStart = -1
            lastAddedCount = 0
        }
    }

    fun onDetachedFromWindow() {
        renderLayer.clear()
    }
}
