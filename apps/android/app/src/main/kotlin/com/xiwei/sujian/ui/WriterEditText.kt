package com.xiwei.sujian.ui

import android.content.Context
import android.graphics.Canvas
import android.os.Build
import android.text.Editable
import android.text.Layout
import android.text.TextWatcher
import android.util.AttributeSet
import android.view.MotionEvent
import android.view.VelocityTracker
import android.view.View
import android.view.ViewConfiguration
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.widget.OverScroller
import androidx.appcompat.widget.AppCompatEditText
import com.xiwei.sujian.ui.span.SujianInputConnection
import kotlin.math.abs

/**
 * WriterEditText - 素笺写作编辑器
 *
 * 基于 AppCompatEditText，添加打字动画、平滑光标、自动缩进和滚动增强
 *
 * ## 控制器层级
 * - EditorActivity → WriterEditText → EditorAnimationRuntime / TypingAnimationController / AutoIndentController
 *
 * ## 当前阶段
 * - **是**：保留原生文本栈和系统菜单外观
 * - **否**：不接管菜单（通过 EditorViewModel 路由动作）
 *
 * ## 后续扩展
 * - 接管长按菜单内容
 * - 接管选区手柄和命令状态
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
    internal var autoIndentController: AutoIndentController? = null
    internal var sujianInputConnection: SujianInputConnection? = null

    private var lastTypingEnabled: Boolean? = null
    private var lastTypingDuration: Long? = null
    private var lastSmoothEnabled: Boolean? = null
    private var lastSmoothDuration: Long? = null
    private var lastAutoIndentEnabled: Boolean? = null
    private var lastAutoIndentWidth: Float? = null
    private lateinit var contextMenuController: EditorContextMenuController
    private var needsDelayedIndentFullRebuild = false
    private var isEditorScrolling = false
    private val scrollIdleDelayMs = 140L
    private val scrollIdleRunnable = Runnable { setEditorScrolling(false) }
    private val flingScroller = OverScroller(context)
    private val touchConfig = ViewConfiguration.get(context)
    private val touchSlop = touchConfig.scaledTouchSlop
    private val minimumFlingVelocity = touchConfig.scaledMinimumFlingVelocity
    private val maximumFlingVelocity = touchConfig.scaledMaximumFlingVelocity
    private var velocityTracker: VelocityTracker? = null
    private var lastTouchY = 0f
    private var flingDragStarted = false

    fun setTypingAnimationEnabled(enabled: Boolean, durationMs: Long = 100L) {
        if (!controllersReady) return
        lastTypingEnabled = enabled
        lastTypingDuration = durationMs
        typingAnimationController?.setTypingAnimationEnabled(enabled, durationMs)
    }

    /**
     * 设置 Core 动画事件提供者。
     *
     * 调用此方法后，TypingAnimationController 将通过 Core 的 EditorEngine
     * 判断 should_animate 和事件语义，而非本地 count<=3 判断。
     *
     * 通常在 EditorActivity 初始化 Core 连接后调用：
     * ```kotlin
     * writerEditText.setAnimationEventProvider(AnimationEventProvider { old, new, ... ->
     *     service.editorAnimationEvents(old, new, ...)
     * })
     * ```
     */
    fun setAnimationEventProvider(provider: AnimationEventProvider) {
        if (!controllersReady) return
        typingAnimationController?.setAnimationEventProvider(provider)
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

    private fun setEditorScrolling(scrolling: Boolean) {
        removeCallbacks(scrollIdleRunnable)
        if (isEditorScrolling != scrolling) {
            isEditorScrolling = scrolling
            typingAnimationController?.isScrollAnimationsSuppressed = scrolling
            renderLayer?.setScrolling(scrolling)
        }
        if (scrolling) {
            postDelayed(scrollIdleRunnable, scrollIdleDelayMs)
        }
    }

    private fun markEditorScrolling() {
        if (!controllersReady) return
        setEditorScrolling(true)
    }

    private fun maxEditorScrollY(): Int {
        val textLayout = layout ?: return 0
        val viewportHeight = (height - compoundPaddingTop - compoundPaddingBottom).coerceAtLeast(0)
        return (textLayout.height - viewportHeight).coerceAtLeast(0)
    }

    private fun recycleVelocityTracker() {
        velocityTracker?.recycle()
        velocityTracker = null
    }

    private fun startEditorFling(velocityY: Float): Boolean {
        val maxScrollY = maxEditorScrollY()
        if (maxScrollY <= 0) return false
        val startY = scrollY.coerceIn(0, maxScrollY)
        flingScroller.fling(
            0,
            startY,
            0,
            -velocityY.toInt(),
            0,
            0,
            0,
            maxScrollY,
            0,
            height / 2
        )
        markEditorScrolling()
        postInvalidateOnAnimation()
        return true
    }

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
                if (count > 0 && s != null) {
                    val end = (start + count).coerceAtMost(s.length)
                    if (start in 0..end && s.subSequence(start, end).contains('\n')) {
                        needsDelayedIndentFullRebuild = true
                    }
                }
                typingAnimationController?.beforeTextChanged(s, start, count, after)
            }

            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {
                if (!controllersReady) return
                if (isUpdatingSpanWrapper || autoIndentController?.isUpdatingSpan == true) return
                if (count > 0 && s != null) {
                    val end = (start + count).coerceAtMost(s.length)
                    if (start in 0..end && s.subSequence(start, end).contains('\n')) {
                        needsDelayedIndentFullRebuild = true
                    }
                }
                if (before > 1 || count > 1) {
                    needsDelayedIndentFullRebuild = true
                }
                typingAnimationController?.onTextChanged(start, count)
            }

            override fun afterTextChanged(s: Editable?) {
                if (!controllersReady) return
                if (isUpdatingSpanWrapper || autoIndentController?.isUpdatingSpan == true) return
                val editable = s ?: return

                typingAnimationController?.afterTextChanged(editable)

                val composingStart = BaseInputConnection.getComposingSpanStart(editable)
                val composingEnd = BaseInputConnection.getComposingSpanEnd(editable)
                val isComposing = composingStart != -1 && composingEnd != -1

                val composingActive = autoIndentController?.isComposingActive == true
                if (!isComposing && !composingActive) {
                    if (needsDelayedIndentFullRebuild) {
                        autoIndentController?.updateParagraphIndentSpans(editable, isFullRebuild = true)
                    } else {
                        val cursorPos = selectionStart
                        if (cursorPos >= 0) {
                            autoIndentController?.updateParagraphIndentSpans(editable, updateStartPos = cursorPos, isFullRebuild = false)
                        }
                    }
                    if (autoIndentController?.pendingFullRebuildAfterComposition == true) {
                        autoIndentController?.updateParagraphIndentSpans(editable, isFullRebuild = true)
                    }
                } else {
                    if (needsDelayedIndentFullRebuild && !isComposing) {
                        autoIndentController?.requestDelayedFullRebuild()
                    }
                }
                needsDelayedIndentFullRebuild = false
            }
        })

        layer.smoothCursorRenderer.cursorRuntimeReady = true

        viewTreeObserver.addOnGlobalLayoutListener {
            if (layer.smoothCursorRenderer.smoothCursorEnabled && isFocused) {
                layer.smoothCursorRenderer.updateCursorTarget(true)
            }
        }

        typeface = android.graphics.Typeface.create("sans-serif", typeface?.style ?: android.graphics.Typeface.NORMAL)

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
            breakStrategy = Layout.BREAK_STRATEGY_SIMPLE
            hyphenationFrequency = Layout.HYPHENATION_FREQUENCY_NONE
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            justificationMode = Layout.JUSTIFICATION_MODE_NONE
        }
        includeFontPadding = false
        contextMenuController = EditorContextMenuController(this)
        contextMenuController.installActionModeCallbacks()
    }

    fun onEditorResume() {
        if (!controllersReady) return
        post {
            if (!isAttachedToWindow || !isShown) return@post
            animationRuntime?.resumeAfterVisibilityRestored()
            renderLayer?.onEditorResume()
            text?.let { autoIndentController?.requestDelayedFullRebuild() }
        }
    }

    override fun onSelectionChanged(selStart: Int, selEnd: Int) {
        super.onSelectionChanged(selStart, selEnd)
        if (!controllersReady) return
        renderLayer?.onSelectionChanged(selStart, selEnd)
        if (autoIndentController?.pendingFullRebuildAfterComposition == true) {
            text?.let { autoIndentController?.updateParagraphIndentSpans(it, isFullRebuild = true) }
        }
    }

    override fun onScrollChanged(horiz: Int, vert: Int, oldHoriz: Int, oldVert: Int) {
        super.onScrollChanged(horiz, vert, oldHoriz, oldVert)
        if (!controllersReady) return
        if (horiz != oldHoriz || vert != oldVert) {
            markEditorScrolling()
        }
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        if (controllersReady) {
            velocityTracker?.addMovement(event)
            when (event.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    flingScroller.forceFinished(true)
                    recycleVelocityTracker()
                    velocityTracker = VelocityTracker.obtain().also { it.addMovement(event) }
                    lastTouchY = event.y
                    flingDragStarted = false
                }
                MotionEvent.ACTION_MOVE -> {
                    if (abs(event.y - lastTouchY) > touchSlop) {
                        flingDragStarted = true
                        markEditorScrolling()
                    }
                    lastTouchY = event.y
                }
                MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                    if (event.actionMasked == MotionEvent.ACTION_UP && flingDragStarted) {
                        velocityTracker?.let { tracker ->
                            tracker.addMovement(event)
                            tracker.computeCurrentVelocity(1000, maximumFlingVelocity.toFloat())
                            val velocityY = tracker.yVelocity
                            if (abs(velocityY) >= minimumFlingVelocity) {
                                startEditorFling(velocityY)
                            }
                        }
                    }
                    recycleVelocityTracker()
                    flingDragStarted = false
                    if (isEditorScrolling) {
                        removeCallbacks(scrollIdleRunnable)
                        postDelayed(scrollIdleRunnable, scrollIdleDelayMs)
                    }
                }
            }
        }
        return super.onTouchEvent(event)
    }

    override fun onTextContextMenuItem(id: Int): Boolean {
        if (contextMenuController.handleTextContextMenuItem(id)) {
            return true
        }
        return super.onTextContextMenuItem(id)
    }

    fun performCopy() {
        val editable = text ?: return
        val selStart = selectionStart
        val selEnd = selectionEnd
        if (selStart != selEnd && selStart >= 0 && selEnd <= editable.length) {
            val selectedText = editable.subSequence(selStart, selEnd).toString()
            val clipboard = android.content.ClipData.newPlainText("text", selectedText)
            val cm = context.getSystemService(android.content.Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager
            cm.setPrimaryClip(clipboard)
        }
    }

    fun onPerformCut() {
        val editable = text ?: return
        val selStart = selectionStart
        val selEnd = selectionEnd
        if (selStart != selEnd && selStart >= 0 && selEnd <= editable.length) {
            val selectedText = editable.subSequence(selStart, selEnd).toString()
            val clipboard = android.content.ClipData.newPlainText("text", selectedText)
            val cm = context.getSystemService(android.content.Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager
            cm.setPrimaryClip(clipboard)
            // 删除选中文本 - 需要禁用动画，避免 TextWatcher 冲突
            editable.delete(selStart, selEnd)
        }
    }

    fun performPasteFromSystem(id: Int) {
        val editable = text ?: return
        val cm = context.getSystemService(android.content.Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager
        val clip = cm.primaryClip ?: return
        if (clip.itemCount > 0) {
            val pasteText = clip.getItemAt(0)?.text?.toString() ?: return
            val selStart = selectionStart
            val selEnd = selectionEnd
            if (selStart >= 0 && selEnd <= editable.length) {
                editable.replace(selStart, selEnd, pasteText)
            }
        }
    }

    override fun computeScroll() {
        super.computeScroll()
        if (!controllersReady) return
        if (!flingScroller.computeScrollOffset()) return

        val maxScrollY = maxEditorScrollY()
        val nextY = flingScroller.currY.coerceIn(0, maxScrollY)
        if (nextY != flingScroller.currY) {
            flingScroller.forceFinished(true)
        }
        if (nextY != scrollY) {
            scrollTo(scrollX, nextY)
            markEditorScrolling()
        }
        postInvalidateOnAnimation()
    }

    override fun onDetachedFromWindow() {
        super.onDetachedFromWindow()
        if (!controllersReady) return
        removeCallbacks(scrollIdleRunnable)
        flingScroller.forceFinished(true)
        recycleVelocityTracker()
        renderLayer?.onDetachedFromWindow()
        typingAnimationController?.onDetachedFromWindow()
        animationRuntime?.clear()
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        onEditorResume()
    }

    override fun onVisibilityChanged(changedView: View, visibility: Int) {
        super.onVisibilityChanged(changedView, visibility)
        if (visibility == View.VISIBLE) {
            onEditorResume()
        }
    }

    override fun onFocusChanged(focused: Boolean, direction: Int, previouslyFocusedRect: android.graphics.Rect?) {
        super.onFocusChanged(focused, direction, previouslyFocusedRect)
        if (!controllersReady) return
        renderLayer?.onFocusChanged(focused)
    }

    override fun onWindowFocusChanged(hasWindowFocus: Boolean) {
        super.onWindowFocusChanged(hasWindowFocus)
        if (!controllersReady) return
        if (hasWindowFocus) {
            onEditorResume()
        }
    }

    override fun onDraw(canvas: Canvas) {
        if (!controllersReady) {
            super.onDraw(canvas)
            return
        }
        if (isEditorScrolling) {
            if (lastSmoothEnabled == true) {
                isCursorVisible = false
            }
            super.onDraw(canvas)
            return
        }
        renderLayer?.beforeTextDraw()
        super.onDraw(canvas)
        renderLayer?.drawAfterText(canvas)
    }

    override fun onCreateInputConnection(outAttrs: EditorInfo): InputConnection {
        val ic = super.onCreateInputConnection(outAttrs) ?: return super.onCreateInputConnection(outAttrs)
        val wrapped = SujianInputConnection(ic, this)
        sujianInputConnection = wrapped
        return wrapped
    }

    internal fun onInputBeforeCommit(pos: Int, textLen: Int) {
        if (!controllersReady) return
        val layout = layout ?: return
        if (pos < 0) return
        val line = layout.getLineForOffset(pos)
        val x = layout.getPrimaryHorizontal(pos)
        val y = layout.getLineBaseline(line).toFloat()
        typingAnimationController?.recordCursorBeforeChange(x, y)
        renderLayer?.smoothCursorRenderer?.saveOldCursorRect()
    }

    internal fun onInputBeforeDelete(pos: Int, beforeLength: Int) {
        if (!controllersReady) return
        val layout = layout ?: return
        if (pos < 0) return
        renderLayer?.smoothCursorRenderer?.saveOldCursorRect()
    }

    internal fun onInputSetComposingText(text: CharSequence?, newCursorPosition: Int) {
        if (!controllersReady) return
        autoIndentController?.markComposingActive()
    }

    internal fun onInputFinishComposing() {
        if (!controllersReady) return
        autoIndentController?.markComposingFinished()
        val editable = editableText ?: return
        autoIndentController?.updateParagraphIndentSpans(editable, isFullRebuild = true)
    }

}
