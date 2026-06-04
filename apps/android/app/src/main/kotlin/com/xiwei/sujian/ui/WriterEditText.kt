package com.xiwei.sujian.ui

import android.content.Context
import android.graphics.Canvas
import android.text.Editable
import android.text.TextWatcher
import android.util.AttributeSet
import android.view.MotionEvent
import androidx.appcompat.widget.AppCompatEditText

/**
 * WriterEditText — 自定义写作编辑器
 *
 * 继承 AppCompatEditText，集成平滑光标、打字动画、自动缩进等写作增强功能。
 *
 * ## 架构定位
 * - EditorActivity → WriterEditText → EditorAnimationRuntime / TypingAnimationController / AutoIndentController
 *
 * ## 职责边界
 * - **做**：管理各动画控制器的生命周期、绘制覆盖层
 * - **不做**：业务逻辑（由 EditorViewModel 负责）
 *
 * ## 使用场景
 * - 编辑器页面的文本输入区域
 * - 提供平滑光标和打字动画效果
 */
class WriterEditText @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
    defStyleAttr: Int = android.R.attr.editTextStyle
) : AppCompatEditText(context, attrs, defStyleAttr) {

    internal var isUpdatingSpanWrapper = false
    private var controllersReady = false

    var animationRuntime: EditorAnimationRuntime? = null
        private set

    private var typingAnimationController: TypingAnimationController? = null
    internal var renderLayer: EditorRenderLayer? = null
    private var autoIndentController: AutoIndentController? = null

    private var lastTypingEnabled: Boolean? = null
    private var lastTypingDuration: Long? = null
    private var lastSmoothEnabled: Boolean? = null
    private var lastSmoothDuration: Long? = null
    private var lastAutoIndentEnabled: Boolean? = null
    private var lastAutoIndentWidth: Float? = null

    fun setTypingAnimationEnabled(enabled: Boolean, durationMs: Long = 100L) {
        if (!controllersReady) return
        lastTypingEnabled = enabled
        lastTypingDuration = durationMs
        typingAnimationController?.setTypingAnimationEnabled(enabled, durationMs)
    }

    fun setSmoothCursorEnabled(enabled: Boolean, durationMs: Long = 80L) {
        if (!controllersReady) return
        lastSmoothEnabled = enabled
        lastSmoothDuration = durationMs
        renderLayer?.smoothCursorRenderer?.setSmoothCursorEnabled(enabled, durationMs)
    }

    fun setAutoIndent(enabled: Boolean, widthChars: Float) {
        if (!controllersReady) return
        if (lastAutoIndentEnabled == enabled && lastAutoIndentWidth == widthChars) return
        lastAutoIndentEnabled = enabled
        lastAutoIndentWidth = widthChars
        autoIndentController?.setAutoIndent(enabled, widthChars)
    }

    fun typingAnimationDurationMs(): Long = typingAnimationController?.typingAnimationDurationMs ?: 0L
    fun cursorAnimationDurationMs(): Long = renderLayer?.smoothCursorRenderer?.smoothCursorDurationMs ?: 0L

    fun runWithoutTextAnimations(block: () -> Unit) {
        if (!controllersReady) {
            block()
            return
        }
        val oldAnimEnabled = typingAnimationController?.isSuppressAnimations ?: false
        val oldIndentEnabled = autoIndentController?.isSuppressing ?: false
        typingAnimationController?.isSuppressAnimations = true
        autoIndentController?.isSuppressing = true
        renderLayer?.clear()
        try {
            block()
        } finally {
            typingAnimationController?.isSuppressAnimations = oldAnimEnabled
            autoIndentController?.isSuppressing = oldIndentEnabled
            if (text != null) {
                autoIndentController?.updateParagraphIndentSpans(text!!, isFullRebuild = true)
            }
        }
    }

    init {
        animationRuntime = EditorAnimationRuntime(this)
        val layer = EditorRenderLayer(this)
        renderLayer = layer
        typingAnimationController = TypingAnimationController(this, layer)
        autoIndentController = AutoIndentController(this)
        controllersReady = true

        addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {
                if (!controllersReady) return
                if (isUpdatingSpanWrapper || autoIndentController?.isUpdatingSpan == true) return
                typingAnimationController?.beforeTextChanged(s, start, count, after)
            }

            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {
                if (!controllersReady) return
                if (isUpdatingSpanWrapper || autoIndentController?.isUpdatingSpan == true) return
                typingAnimationController?.onTextChanged(s, start, before, count)
            }

            override fun afterTextChanged(s: Editable?) {
                if (!controllersReady) return
                if (isUpdatingSpanWrapper || autoIndentController?.isUpdatingSpan == true) return
                val editable = s ?: return

                typingAnimationController?.afterTextChanged(editable)

                autoIndentController?.updateParagraphIndentSpans(editable, updateStartPos = selectionStart)
            }
        })

        layer.smoothCursorRenderer.cursorRuntimeReady = true

        viewTreeObserver.addOnGlobalLayoutListener {
            if (layer.smoothCursorRenderer.smoothCursorEnabled && isFocused) {
                layer.smoothCursorRenderer.updateCursorTarget(false)
            }
        }

        typeface = android.graphics.Typeface.create("sans-serif", typeface?.style ?: android.graphics.Typeface.NORMAL)
    }

    override fun onSelectionChanged(selStart: Int, selEnd: Int) {
        super.onSelectionChanged(selStart, selEnd)
        if (!controllersReady) return
        renderLayer?.onSelectionChanged(selStart, selEnd)
    }

    override fun onDetachedFromWindow() {
        super.onDetachedFromWindow()
        if (!controllersReady) return
        renderLayer?.onDetachedFromWindow()
        typingAnimationController?.onDetachedFromWindow()
        animationRuntime?.clear()
    }

    override fun onFocusChanged(focused: Boolean, direction: Int, previouslyFocusedRect: android.graphics.Rect?) {
        super.onFocusChanged(focused, direction, previouslyFocusedRect)
        if (!controllersReady) return
        renderLayer?.onFocusChanged(focused)
    }

    override fun onWindowFocusChanged(hasWindowFocus: Boolean) {
        super.onWindowFocusChanged(hasWindowFocus)
        if (!controllersReady) return
        if (hasWindowFocus && isFocused && renderLayer?.smoothCursorRenderer?.smoothCursorEnabled == true) {
            renderLayer?.smoothCursorRenderer?.updateCursorTarget(false)
            renderLayer?.smoothCursorRenderer?.startCursorBlink()
        }
    }

    override fun onDraw(canvas: Canvas) {
        if (!controllersReady) {
            super.onDraw(canvas)
            return
        }
        renderLayer?.beforeTextDraw()
        super.onDraw(canvas)
        renderLayer?.drawAfterText(canvas)
    }
}
