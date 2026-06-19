package com.xiwei.sujian.ui

import android.text.Editable
import android.view.inputmethod.BaseInputConnection

/**
 * TypingAnimationController — 打字动画控制器
 *
 * 监听 EditText 的文本变化，并为后续 Core 事件驱动动画保留入口。
 *
 * ## 架构定位
 * - WriterEditText → TypingAnimationController → AndroidEditorAnimationEvent 占位
 *
 * ## 职责边界
 * - **做**：文本变化监听、动画参数管理、记录轻量事件占位
 * - **不做**：动画渲染或正文 span 可见性修改
 * - **禁止**：向正文 Editable 注入透明 ForegroundColorSpan 隐藏文字
 *
 * ## 使用场景
 * - 用户输入/删除字符时记录后续自绘 renderer 可消费的事件占位
 * - 粘贴/大段替换时不记录逐字动画
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
                pendingDeleteStart = -1
                pendingDeleteText = ""
                pendingDeleteStartX = -1f
                pendingDeleteStartY = -1f
                renderLayer.clear()
            }
        }

    private var lastAddedStart = -1
    private var lastAddedCount = 0
    private var cursorBeforeX = -1f
    private var cursorBeforeY = -1f

    private var isPasteOrDelete = false
    var lastEditorAnimationEvent: AndroidEditorAnimationEvent? = null
        private set

    // 删除动画待提交信息：在 beforeTextChanged 记录起始位置，在 afterTextChanged 提交完整动画
    private var pendingDeleteStartX = -1f
    private var pendingDeleteStartY = -1f
    private var pendingDeleteText = ""
    private var pendingDeleteStart = -1

    fun setTypingAnimationEnabled(enabled: Boolean, durationMs: Long = 100L) {
        typingAnimationEnabled = enabled
        typingAnimationDurationMs = durationMs
        if (!enabled) renderLayer.clear()
    }

    fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {
        if (isScrollAnimationsSuppressed) {
            lastAddedStart = -1
            lastAddedCount = 0
            return
        }
        if (isSuppressAnimations) {
            pendingDeleteStart = -1
            pendingDeleteText = ""
            pendingDeleteStartX = -1f
            pendingDeleteStartY = -1f
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

        if (count > 0 && count <= 3 && after == 0 && !matchesComposing && typingAnimationDurationMs > 0 && s != null) {
            val deletedText = s.subSequence(start, start + count).toString()
            if (deletedText.contains('\n') || deletedText.contains('\r')) return
            lastEditorAnimationEvent = AndroidEditorAnimationEvent(
                kind = "delete",
                start = start,
                text = deletedText,
                durationMs = typingAnimationDurationMs
            )

            // 记录删除动画的起始位置（被删字符位置），等 afterTextChanged 提交
            // 不能在 beforeTextChanged 里直接提交，因为此时 layout 还未更新，
            // 删除后光标位置（目标位置）需要在新 layout 中计算
            if (typingAnimationEnabled && editText.layout != null) {
                pendingDeleteStartX = editText.layout.getPrimaryHorizontal(start)
                pendingDeleteStartY = editText.layout.getLineBaseline(editText.layout.getLineForOffset(start)).toFloat()
                pendingDeleteText = deletedText
                pendingDeleteStart = start
            }
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
        if (isSuppressAnimations || isScrollAnimationsSuppressed) {
            pendingDeleteStart = -1
            pendingDeleteText = ""
            pendingDeleteStartX = -1f
            pendingDeleteStartY = -1f
            return
        }


        // 提交待处理的删除动画（在 afterTextChanged 里才能拿到新光标位置作为目标）
        if (pendingDeleteStart >= 0 && typingAnimationEnabled && editText.layout != null) {
            val newCursorOffset = editText.selectionStart
            if (newCursorOffset >= 0) {
                val newLine = editText.layout.getLineForOffset(newCursorOffset)
                val destX = editText.layout.getPrimaryHorizontal(newCursorOffset)
                val destY = editText.layout.getLineBaseline(newLine).toFloat()
                renderLayer.addTypingAnim(OverlayAnim(
                    insertedStart = pendingDeleteStart,
                    insertedText = pendingDeleteText,
                    startX = pendingDeleteStartX,
                    startY = pendingDeleteStartY,
                    endX = destX,
                    endY = destY,
                    durationMs = typingAnimationDurationMs,
                    isDeletion = true
                ))
            }
            pendingDeleteStart = -1
            pendingDeleteStartX = -1f
            pendingDeleteStartY = -1f
            pendingDeleteText = ""
        }

        val composingStart = BaseInputConnection.getComposingSpanStart(editable)
        val composingEnd = BaseInputConnection.getComposingSpanEnd(editable)
        val isComposing = composingStart != -1 && composingEnd != -1

        if (isComposing && lastAddedStart >= composingStart && lastAddedStart + lastAddedCount <= composingEnd) {
             if (DEBUG_ANIM) android.util.Log.d(TAG, "afterTextChanged - skipping animation for composing text.")
             lastAddedStart = -1
             lastAddedCount = 0
             return
        }

        if (lastAddedCount in 1..3 && lastAddedStart >= 0) {
            val start = lastAddedStart
            val end = kotlin.math.min(start + lastAddedCount, editable.length)

            if (end > start && typingAnimationDurationMs > 0) {
                val insertedText = editable.subSequence(start, end).toString()
                if (insertedText.contains('\n') || insertedText.contains('\r')) {
                    lastAddedStart = -1
                    lastAddedCount = 0
                    return
                }

                lastEditorAnimationEvent = AndroidEditorAnimationEvent(
                    kind = "insert",
                    start = start,
                    text = insertedText,
                    durationMs = typingAnimationDurationMs
                )

                // 提交插入动画到 renderLayer
                if (typingAnimationEnabled && editText.layout != null) {
                    val animEnd = kotlin.math.min(start + lastAddedCount, editable.length)
                    if (animEnd > start) {
                        val destX = editText.layout.getPrimaryHorizontal(start)
                        val line = editText.layout.getLineForOffset(start)
                        val destY = editText.layout.getLineBaseline(line).toFloat()
                        renderLayer.addTypingAnim(OverlayAnim(
                            insertedStart = start,
                            insertedText = insertedText,
                            startX = cursorBeforeX,
                            startY = cursorBeforeY,
                            endX = destX,
                            endY = destY,
                            durationMs = typingAnimationDurationMs,
                            isDeletion = false
                        ))
                    }
                }

                if (DEBUG_ANIM) {
                    android.util.Log.d(TAG, "afterTextChanged - recorded event: start=$start, length=${insertedText.length}")
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

data class AndroidEditorAnimationEvent(
    val kind: String,
    val start: Int,
    val text: String,
    val durationMs: Long
)
