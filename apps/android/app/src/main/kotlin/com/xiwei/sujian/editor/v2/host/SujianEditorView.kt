package com.xiwei.sujian.editor.v2.host

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.text.TextPaint
import android.util.AttributeSet
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.View
import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputMethodManager
import com.xiwei.sujian.editor.v2.mirror.EditResult
import com.xiwei.sujian.editor.v2.input.AndroidInputAdapter
import com.xiwei.sujian.editor.v2.pipeline.AndroidEditorPipeline
import com.xiwei.sujian.editor.v2.pipeline.EditorCommandPort
import com.xiwei.sujian.editor.v2.pipeline.PipelineOutput
import com.xiwei.sujian.editor.v2.coordinator.TextEditorProfile
import com.xiwei.sujian.editor.v2.coordinator.NewlinePolicy
import com.xiwei.sujian.editor.v2.coordinator.WindowDisplayFrameClock
import com.xiwei.sujian.R
import uniffi.writer_core.EditorTransactionCauseDto

class SujianEditorView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
    defStyleAttr: Int = 0,
    animationTimeSource: com.xiwei.sujian.editor.v2.visual.AnimationTimeSource = com.xiwei.sujian.editor.v2.visual.ChoreographerAnimationTimeSource(),
    transactionIdSource: com.xiwei.sujian.editor.v2.visual.TransactionIdSource = com.xiwei.sujian.editor.v2.visual.TransactionIdSource()
) : View(context, attrs, defStyleAttr), WindowDisplayFrameClock.FrameListener {

    private val textPaint = TextPaint().apply {
        textSize = 48f
        isAntiAlias = true
    }
    private val timeSource = animationTimeSource
    private val pipeline = AndroidEditorPipeline.create(
        com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror(),
        textPaint,
        animationTimeSource,
        transactionIdSource
    )
    private val inputAdapter = AndroidInputAdapter(pipeline.mirror, pipeline) { pipeline.getCurrentProjection() }

    private var scrollX: Float = 0f
    private var scrollY: Float = 0f
    private var maxScrollY: Float = 0f

    fun getScrollXPos(): Float = scrollX
    fun getScrollYPos(): Float = scrollY

    /**
     * #592 三：窗口重建/重绑定时恢复会话层投影保存的滚动位置。
     * 滚动值先夹到有效范围，布局尚未就绪时由后续 updateMaxScroll 收敛。
     */
    fun setScrollPosition(sx: Float, sy: Float) {
        scrollX = sx.coerceAtLeast(0f)
        scrollY = sy.coerceAtLeast(0f)
        invalidate()
    }
    private var searchHighlights: List<Pair<Int, Int>> = emptyList()
    private var pendingLayoutNeeded: Boolean = false
    private var frameClock: WindowDisplayFrameClock? = null
    private var isRegisteredWithClock: Boolean = false
    @Volatile
    private var pendingFrameTimeNanos: Long = Long.MIN_VALUE

    var kernelBridge: EditorKernelBridge?
        get() = pipeline.kernelBridge
        set(value) { pipeline.kernelBridge = value }

    private var _themeBackgroundColor: Int = Color.WHITE

    fun getThemeBackgroundColor(): Int = _themeBackgroundColor
    private var lastAppliedThemeColors: com.xiwei.sujian.ui.compose.theme.EditorThemeColors? = null

    init {
        inputAdapter.setHostView(this)
        inputAdapter.onPipelineOutput = { output: PipelineOutput -> handlePipelineOutput(output) }
        inputAdapter.onCompositionVisualUpdate = {
            updateMaxScroll()
            scrollY = scrollY.coerceIn(0f, maxScrollY)
            invalidate()
            if (pipeline.hasActiveAnimation()) {
                requestAnimationFrame()
            }
        }
        id = R.id.editor_content
        importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_YES
        isFocusable = true
        isFocusableInTouchMode = true
        val contentInset = (16 * context.resources.displayMetrics.density).toInt()
        setPadding(contentInset, contentInset, contentInset, contentInset)
    }

    fun loadText(text: String, cursorUtf8: Int) {
        val result = pipeline.loadText(text, cursorUtf8)
        if (result is AndroidEditorPipeline.LoadTextResult.Loaded) {
            if (width > 0) {
                updateLayoutConfig()
            } else {
                pendingLayoutNeeded = true
            }
        }
    }

    fun insertText(byteOffset: Int, text: String, cause: EditorTransactionCauseDto = EditorTransactionCauseDto.TYPING) {
        val output = pipeline.insertText(byteOffset, text, cause)
        handlePipelineOutput(output)
    }

    fun deleteRange(byteStart: Int, byteEndExclusive: Int, cause: EditorTransactionCauseDto = EditorTransactionCauseDto.DELETE) {
        val output = pipeline.deleteRange(byteStart, byteEndExclusive, cause)
        handlePipelineOutput(output)
    }

    fun replaceRangeTyped(byteStart: Int, byteEndExclusive: Int, replacementText: String, originalText: String, cause: EditorTransactionCauseDto = EditorTransactionCauseDto.TYPING, beforePatch: (() -> Unit)? = null) {
        val output = pipeline.replaceRangeTyped(byteStart, byteEndExclusive, replacementText, originalText, cause, beforePatch)
        handlePipelineOutput(output)
    }

    fun setSelectionTyped(anchorByteOffset: Int, headByteOffset: Int) {
        val output = pipeline.setSelectionTyped(anchorByteOffset, headByteOffset)
        handlePipelineOutput(output)
    }

    fun performUndo() {
        val output = pipeline.performUndo()
        handlePipelineOutput(output)
    }

    fun performRedo() {
        val output = pipeline.performRedo()
        handlePipelineOutput(output)
    }

    private fun handlePipelineOutput(output: PipelineOutput, suppressContentCallback: Boolean = false) {
        handlePipelineOutputInternal(output, suppressContentCallback)
    }

    fun handlePipelineOutput(output: PipelineOutput) {
        handlePipelineOutputInternal(output, false)
    }

    private fun handlePipelineOutputInternal(output: PipelineOutput, suppressContentCallback: Boolean = false) {
        when (output) {
            is PipelineOutput.Edited -> {
                updateMaxScroll()
                scrollY = scrollY.coerceIn(0f, maxScrollY)
                if (!suppressContentCallback && output.result.displayPatches.isNotEmpty()) {
                    val editText = pipeline.getText()
                    // #595 一：先更新会话层唯一 SessionState（revision/transactionId），
                    // 再通知 ViewModel 保存。确保 WritingPane 的 LaunchedEffect(uiState.content)
                    // 触发时 sessionStateFlow 已是最新，不会误判为外部更新而 reset session。
                    if (output.result.isApplied()) {
                        onLocalEdit?.invoke(editText, output.result.newRevision, output.result.transactionId)
                    }
                    onContentChanged?.invoke(editText)
                }
                invalidate()
                if (pipeline.hasActiveAnimation()) {
                    requestAnimationFrame()
                }
            }
            is PipelineOutput.NeedReload -> {
                reloadFromKernel()
            }
            is PipelineOutput.StaleOrInvalid -> {
                reloadFromKernel()
            }
        }
    }

    private fun reloadFromKernel() {
        if (pipeline.reloadFromKernel()) {
            android.util.Log.w(
                "SujianEditorInput",
                "reloadFromKernel applied; mirror='${pipeline.getText()}' rev=${pipeline.getRevision()} cursor=${pipeline.getCursorUtf8()}"
            )
            updateLayoutConfig()
            val reloadText = pipeline.getText()
            onLocalEdit?.invoke(reloadText, pipeline.getRevision(), 0L)
            onContentChanged?.invoke(reloadText)
        } else {
            android.util.Log.w("SujianEditorInput", "reloadFromKernel FAILED (no session snapshot)")
        }
    }

    fun onCompositionUpdated() {
        pipeline.onCompositionUpdated()
        updateMaxScroll()
        scrollY = scrollY.coerceIn(0f, maxScrollY)
        invalidate()
        if (pipeline.hasActiveAnimation()) {
            requestAnimationFrame()
        }
    }

    fun applyCompositionUpdate(visualIntent: com.xiwei.sujian.editor.v2.mirror.VisualIntent, mirrorUpdate: (() -> Unit)? = null) {
        pipeline.applyCompositionUpdate(visualIntent, mirrorUpdate)
        updateMaxScroll()
        scrollY = scrollY.coerceIn(0f, maxScrollY)
        invalidate()
        if (pipeline.hasActiveAnimation()) {
            requestAnimationFrame()
        }
    }

    fun getText(): String = pipeline.getText()

    fun setFontSize(sizeSp: Float) {
        textPaint.textSize = sizeSp * resources.displayMetrics.scaledDensity
        updateLayoutConfig()
    }

    private var lineSpacingMultiplier: Float = 1.0f

    fun setLineSpacingMultiplier(multiplier: Float) {
        lineSpacingMultiplier = multiplier
        pipeline.setLineSpacingMultiplier(multiplier)
        updateLayoutConfig()
    }

    fun getSelectionStart(): Int = pipeline.getSelectionStartUtf8()
    fun getSelectionEnd(): Int = pipeline.getSelectionEndUtf8()
    fun getSelectionStartUtf16(): Int = pipeline.getSelectionStartUtf16()
    fun getSelectionEndUtf16(): Int = pipeline.getSelectionEndUtf16()

    fun setSelectionRange(start: Int, end: Int) {
        setSelectionTyped(start, end)
    }

    fun scrollToSelection() {
        val cursorUtf16 = pipeline.getDisplayCursorUtf16()
        val layoutTextLen = pipeline.getLengthUtf16()
        if (cursorUtf16 < 0 || cursorUtf16 > layoutTextLen) return
        val line = pipeline.getLayoutLineForOffset(cursorUtf16)
        val lineTop = pipeline.getLayoutLineTop(line).toFloat()
        val lineBottom = pipeline.getLayoutLineBottom(line).toFloat()
        val viewHeight = height.toFloat()
        val contentTop = scrollY - paddingTop
        val contentBottom = contentTop + viewHeight
        if (lineTop < contentTop) {
            scrollY = lineTop + paddingTop
        } else if (lineBottom > contentBottom) {
            scrollY = lineBottom - viewHeight + paddingTop
        }
        scrollY = scrollY.coerceIn(0f, maxScrollY)
        invalidate()
    }

    fun replaceRange(start: Int, end: Int, newText: String) {
        replaceRangeTyped(start, end, newText, "", EditorTransactionCauseDto.PROGRAMMATIC)
    }

    fun replaceAll(searchStr: String, replaceStr: String) {
        val output = pipeline.replaceAll(searchStr, replaceStr)
        if (output != null) {
            handlePipelineOutput(output)
        }
    }

    fun applyCompositionCommit(dto: uniffi.writer_core.EditorEditResultDto, preeditText: String = "") {
        val output = pipeline.applyCompositionCommit(dto, preeditText)
        handlePipelineOutput(output)
    }

    fun setSearchHighlights(highlights: List<Pair<Int, Int>>) {
        searchHighlights = highlights
        invalidate()
    }

    fun clearSearchHighlights() {
        searchHighlights = emptyList()
        onSearchHighlightsCleared?.invoke()
        invalidate()
    }

    var onSearchHighlightsCleared: (() -> Unit)? = null

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        super.onSizeChanged(w, h, oldw, oldh)
        if (w > 0) {
            updateLayoutConfig()
            pendingLayoutNeeded = false
        }
    }

    private fun updateMaxScroll() {
        val layoutOverflow = pipeline.getLayoutMaxScrollY(height)
        maxScrollY = if (layoutOverflow > 0f) {
            layoutOverflow + paddingTop + paddingBottom
        } else {
            0f
        }
    }

    private fun updateLayoutConfig() {
        pipeline.updateLayout((width - paddingLeft - paddingRight).coerceAtLeast(1).toFloat())
        updateMaxScroll()
        scrollY = scrollY.coerceIn(0f, maxScrollY)
        invalidate()
    }

    /**
     * Main rendering loop: draw one frame.
     *
     * Animation-driven invalidation is now handled by [WindowDisplayFrameClock]:
     * the clock calls [onFrame] which triggers [invalidate], creating a
     * self-sustaining frame loop as long as [needsFrame] returns true.
     * When the animation completes, the clock stops naturally.
     */
    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        canvas.save()
        canvas.translate(paddingLeft - scrollX, paddingTop - scrollY)
        val searchHighlightsUtf16 = searchHighlights.map { (startUtf8, endUtf8) ->
            Pair(pipeline.utf8ToUtf16(startUtf8), pipeline.utf8ToUtf16(endUtf8))
        }
        val frameTimeNanos = pendingFrameTimeNanos
        if (frameTimeNanos != Long.MIN_VALUE) {
            pendingFrameTimeNanos = Long.MIN_VALUE
            pipeline.drawFrame(canvas, searchHighlightsUtf16, width, height, scrollX, scrollY, frameTimeNanos)
        } else {
            pipeline.drawFrame(canvas, searchHighlightsUtf16, width, height, scrollX, scrollY)
        }
        canvas.restore()
    }

    // #595 六：暂停时不持续请求 VSync — hasActiveAnimation && !isAnimationPaused。
    // 暂停时停止重投 Choreographer.postFrameCallback()，恢复时显式 requestFrame()。
    override fun needsFrame(): Boolean = pipeline.hasActiveAnimation() && !pipeline.isAnimationPaused()

    override fun onFrame(frameTimeNanos: Long) {
        pendingFrameTimeNanos = frameTimeNanos
        if (timeSource is com.xiwei.sujian.editor.v2.visual.ChoreographerAnimationTimeSource) {
            timeSource.onFrameTimeNanos(frameTimeNanos)
        }
        // Advance timeline state at dispatch time (anchor + completion). The draw below
        // repeats these transitions idempotently, so the animation state does not depend
        // on when the invalidate-driven draw is actually delivered (a delayed vsync must
        // not re-anchor or postpone completion of the animation).
        pipeline.onFrameTick(frameTimeNanos / 1_000_000)
        invalidate()
    }

    fun setFrameClock(clock: WindowDisplayFrameClock?) {
        val oldClock = frameClock
        if (oldClock != null && isRegisteredWithClock) {
            oldClock.removeListener(this)
            isRegisteredWithClock = false
        }
        frameClock = clock
    }

    fun requestAnimationFrame() {
        val clock = frameClock ?: return
        if (!isRegisteredWithClock) {
            clock.addListener(this)
            isRegisteredWithClock = true
        }
        clock.requestFrame()
    }

    override fun onCreateInputConnection(outAttrs: android.view.inputmethod.EditorInfo?): InputConnection? {
        val ic = inputAdapter.onCreateInputConnection(outAttrs)
        com.xiwei.sujian.diagnostics.DiagnosticsEvents.inputConnection(created = ic != null, sessionBound = isSessionBound)
        return ic
    }

    override fun onCheckIsTextEditor(): Boolean = isSessionBound

    override fun onInitializeAccessibilityNodeInfo(info: android.view.accessibility.AccessibilityNodeInfo?) {
        super.onInitializeAccessibilityNodeInfo(info)
        info?.isEditable = true
        info?.text = getDisplayText()
        info?.className = android.widget.EditText::class.java.name
        info?.viewIdResourceName = context.packageName + ":id/editor_content"
        info?.isFocusable = true
        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.JELLY_BEAN_MR2) {
            val selStart = pipeline.getSelectionStartUtf16()
            val selEnd = pipeline.getSelectionEndUtf16()
            if (selStart >= 0 && selEnd >= 0) {
                info?.setTextSelection(selStart, selEnd)
            }
        }
        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.LOLLIPOP) {
            info?.addAction(
                android.view.accessibility.AccessibilityNodeInfo.AccessibilityAction.ACTION_SET_TEXT
            )
        }
    }

    override fun performAccessibilityAction(action: Int, arguments: android.os.Bundle?): Boolean {
        if (action == android.view.accessibility.AccessibilityNodeInfo.ACTION_SET_TEXT) {
            if (!isSessionBound) return false
            if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.LOLLIPOP) {
                val text = arguments?.getCharSequence(
                    android.view.accessibility.AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE
                )?.toString() ?: return false
                val currentText = pipeline.getText()
                val currentByteLen = currentText.toByteArray(Charsets.UTF_8).size
                replaceRangeTyped(0, currentByteLen, text, currentText, EditorTransactionCauseDto.PROGRAMMATIC)
                val endByteOffset = text.toByteArray(Charsets.UTF_8).size
                setSelectionTyped(endByteOffset, endByteOffset)
                if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.JELLY_BEAN_MR2) {
                    val event = android.view.accessibility.AccessibilityEvent.obtain(
                        android.view.accessibility.AccessibilityEvent.TYPE_VIEW_TEXT_CHANGED
                    )
                    event.text.add(text)
                    event.fromIndex = 0
                    event.removedCount = currentText.length
                    event.addedCount = text.length
                    event.className = android.widget.EditText::class.java.name
                    event.packageName = context.packageName
                    event.setSource(this)
                    val accessibilityManager = context.getSystemService(
                        android.content.Context.ACCESSIBILITY_SERVICE
                    ) as? android.view.accessibility.AccessibilityManager
                    if (accessibilityManager?.isEnabled == true) {
                        parent?.requestSendAccessibilityEvent(this, event)
                    } else {
                        event.recycle()
                    }
                }
                return true
            }
            return false
        }
        return super.performAccessibilityAction(action, arguments)
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        if (!isSessionBound) return false
        when (event.action) {
            MotionEvent.ACTION_DOWN -> {
                touchDownX = event.x + scrollX - paddingLeft
                touchDownY = event.y + scrollY - paddingTop
                isDragging = false
                requestFocus()
                return true
            }
            MotionEvent.ACTION_MOVE -> {
                val dx = event.x + scrollX - paddingLeft - touchDownX
                val dy = event.y + scrollY - paddingTop - touchDownY
                if (!isDragging && (Math.abs(dx) > 10 || Math.abs(dy) > 10)) {
                    isDragging = true
                }
                if (isDragging && currentProfile.verticalScroll) {
                    scrollY = (scrollY - dy).coerceIn(0f, maxScrollY)
                    touchDownX = event.x + scrollX - paddingLeft
                    touchDownY = event.y + scrollY - paddingTop
                    invalidate()
                }
                return true
            }
            MotionEvent.ACTION_UP -> {
                if (!isDragging) {
                    handleTap(event.x + scrollX - paddingLeft, event.y + scrollY - paddingTop)
                }
                isDragging = false
                return true
            }
        }
        return super.onTouchEvent(event)
    }

    private var touchDownX: Float = 0f
    private var touchDownY: Float = 0f
    private var isDragging: Boolean = false

    private fun handleTap(x: Float, y: Float) {
        val line = pipeline.getLayoutLineForVertical(y.toInt())
        val offset = pipeline.getLayoutOffsetForHorizontal(line, x)
        val byteOffset = pipeline.utf16ToUtf8(offset)
        setSelectionTyped(byteOffset, byteOffset)
        showSoftInput()
    }

    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean {
        if (!isSessionBound) return super.onKeyDown(keyCode, event)
        when (keyCode) {
            KeyEvent.KEYCODE_DEL -> {
                if (inputAdapter.isComposing()) {
                    inputAdapter.handleCompositionCancel()
                    return true
                }
                val selStart = pipeline.getSelectionStartUtf8()
                val selEnd = pipeline.getSelectionEndUtf8()
                if (selStart != selEnd) {
                    replaceRange(selStart, selEnd, "")
                } else if (selEnd > 0) {
                    val prevGraphemeLen = pipeline.previousGraphemeByteLen(selEnd)
                    if (prevGraphemeLen > 0) {
                        replaceRange(selEnd - prevGraphemeLen, selEnd, "")
                    }
                }
                return true
            }
            KeyEvent.KEYCODE_FORWARD_DEL -> {
                val selStart = pipeline.getSelectionStartUtf8()
                val selEnd = pipeline.getSelectionEndUtf8()
                if (selStart != selEnd) {
                    replaceRange(selStart, selEnd, "")
                } else {
                    val textLen = pipeline.getText().toByteArray(Charsets.UTF_8).size
                    if (selEnd < textLen) {
                        val nextGraphemeLen = pipeline.nextGraphemeByteLen(selEnd)
                        if (nextGraphemeLen > 0) {
                            replaceRange(selEnd, selEnd + nextGraphemeLen, "")
                        }
                    }
                }
                return true
            }
            KeyEvent.KEYCODE_ENTER -> {
                if (currentProfile.newlinePolicy == NewlinePolicy.FORBID) {
                    if (currentProfile.commitOnImeAction) {
                        onCommitRequested?.invoke()
                    }
                    return true
                }
                return super.onKeyDown(keyCode, event)
            }
        }
        return super.onKeyDown(keyCode, event)
    }

    override fun onFocusChanged(gained: Boolean, direction: Int, previouslyFocusedRect: android.graphics.Rect?) {
        super.onFocusChanged(gained, direction, previouslyFocusedRect)
        if (gained) {
            showSoftInput()
        } else if (isSessionBound && commitOnFocusLoss) {
            onCommitRequested?.invoke()
        }
    }

    /**
     * #595 六：窗口焦点变化 — 临时失焦暂停并保存可见帧，不永久取消事务。
     *
     * IME 切换、系统浮层、权限弹窗、导航转场和窗口重建都可能造成短暂失焦。
     * 将所有失焦都解释为"丢弃动画事务"会让输入动画随机中断。
     * 只有业务关闭或永久释放才取消事务并释放 bitmap。
     */
    override fun onWindowFocusChanged(hasWindowFocus: Boolean) {
        super.onWindowFocusChanged(hasWindowFocus)
        com.xiwei.sujian.diagnostics.DiagnosticsEvents.editorFocus(hasWindowFocus)
        val frameTimeMs = timeSource.nowNanos() / 1_000_000
        if (!hasWindowFocus) {
            pipeline.pauseAnimation(frameTimeMs)
        } else {
            pipeline.resumeAnimation(frameTimeMs)
            if (pipeline.hasActiveAnimation()) {
                requestAnimationFrame()
            }
        }
    }

    fun requestNextFrame() {
        invalidate()
    }

    fun getPipeline(): EditorCommandPort = pipeline

    fun getPipelineTextPaintSize(): Float = textPaint.textSize

    fun getPipelineLineSpacingMultiplier(): Float = lineSpacingMultiplier

    fun getPipelineThemeColors(): com.xiwei.sujian.ui.compose.theme.EditorThemeColors? = lastAppliedThemeColors

    var onContentChanged: ((String) -> Unit)? = null

    /**
     * #595 一：类型化本地编辑回调 — 传递 text/revision/transactionId。
     *
     * 由 [EditorWindowHost.installContentCallback] 设置，回调中先调用
     * [EditorSessionCoordinator.applyLocalEdit] 更新唯一 SessionState，
     * 再通知 ViewModel 保存。替代仅传字符串的 [onContentChanged]。
     */
    var onLocalEdit: ((text: String, revision: Long, transactionId: Long) -> Unit)? = null

    fun setText(text: String) {
        loadText(text, 0)
    }

    fun setTypingAnimationEnabled(enabled: Boolean, durationMs: Long) {
        pipeline.kernelBridge?.setAnimationDurationMs(durationMs)
        pipeline.setTypingAnimationDurationMs(durationMs)
        if (!enabled) {
            pipeline.cancelActiveTransaction()
        }
        // #595 四: 不在此切换 kernel animation_enabled — 它在 Rust 同时控制文字动画模式
        // 和 CoordinatedCursor.should_animate。kernel animation_enabled 由
        // setKernelAnimationEnabled(textEnabled || cursorEnabled) 原子设置，保证
        // 仅关闭文字动画时光标语义仍被正确上报。文字切片在平台层通过 animationPolicy
        // (SYSTEM_SUPPRESSED) 抑制，走 CursorOnly 事务路径。
        pipeline.setAnimationPolicy(
            if (enabled) com.xiwei.sujian.editor.v2.visual.TextAnimationPolicy.ENABLED
            else com.xiwei.sujian.editor.v2.visual.TextAnimationPolicy.SYSTEM_SUPPRESSED
        )
    }

    /**
     * #595 四: 原子设置 kernel animation_enabled = textEnabled || cursorEnabled。
     * 当仅关闭文字动画但光标动画开启时，kernel 保持 enabled，使 Rust
     * CoordinatedCursor.should_animate 正确上报光标移动语义。
     */
    fun setKernelAnimationEnabled(enabled: Boolean) {
        pipeline.kernelBridge?.setAnimationEnabled(enabled)
    }

    private var smoothCursorEnabled: Boolean = true
    private var smoothCursorDurationMs: Long = 80

    fun setSmoothCursorEnabled(enabled: Boolean, durationMs: Long) {
        smoothCursorEnabled = enabled
        smoothCursorDurationMs = durationMs
        pipeline.setSmoothCursor(enabled, durationMs)
    }

    fun isSmoothCursorEnabled(): Boolean = smoothCursorEnabled

    fun setAutoIndent(enabled: Boolean, widthSp: Float) {
        pipeline.setAutoIndent(enabled, widthSp)
    }

    fun isAutoIndentEnabled(): Boolean = pipeline.isAutoIndentEnabled()
    fun getAutoIndentWidthSp(): Float = pipeline.getAutoIndentWidthSp()

    private var coordinatedAnimationEnabled: Boolean = true
    private var reduceMotionEnabled: Boolean = false

    /**
     * #595 三/九：协同动画设置 — 真正进入 AndroidEditorPipeline/AndroidTextAnimationEngine。
     */
    fun setCoordinatedAnimationEnabled(enabled: Boolean) {
        coordinatedAnimationEnabled = enabled
        pipeline.setCoordinatedAnimationEnabled(enabled)
    }

    fun isCoordinatedAnimationEnabled(): Boolean = coordinatedAnimationEnabled

    /**
     * #595 三：reduce-motion 设置 — 降级所有动画为静态更新。
     */
    fun setReduceMotion(enabled: Boolean) {
        reduceMotionEnabled = enabled
        pipeline.setReduceMotion(enabled)
    }

    fun isReduceMotionEnabled(): Boolean = reduceMotionEnabled

    /**
     * #595 六：当前动画是否因窗口失焦而暂停 — 供宿主派生 [EditorAttachmentState.Paused]。
     */
    fun isAnimationPaused(): Boolean = pipeline.isAnimationPaused()

    fun applyThemeColorsFromAdapter(colors: com.xiwei.sujian.ui.compose.theme.EditorThemeColors) {
        lastAppliedThemeColors = colors
        _themeBackgroundColor = colors.background
        textPaint.color = colors.text
        pipeline.setRendererThemeColors(
            textColor = colors.text,
            cursorColor = colors.cursor,
            selectionColor = colors.selection,
            preeditColor = colors.composing,
            bgColor = colors.background,
            searchHighlightColor = colors.searchHighlight
        )
        invalidate()
    }

    private fun showSoftInput() {
        val imm = context.getSystemService(Context.INPUT_METHOD_SERVICE) as? InputMethodManager
        imm?.showSoftInput(this, 0)
    }

    fun notifyCursorAnchorInfo() {
        val imm = context.getSystemService(android.content.Context.INPUT_METHOD_SERVICE) as? android.view.inputmethod.InputMethodManager ?: return
        val cursorUtf16 = pipeline.getDisplayCursorUtf16()
        val layoutTextLen = pipeline.getLengthUtf16()
        if (cursorUtf16 < 0 || cursorUtf16 > layoutTextLen) return

        val line = pipeline.getLayoutLineForOffset(cursorUtf16)
        val x = pipeline.getLayoutPrimaryHorizontal(cursorUtf16)
        val lineTop = pipeline.getLayoutLineTop(line)
        val lineBottom = pipeline.getLayoutLineBottom(line)

        val info = android.view.inputmethod.CursorAnchorInfo.Builder()
            .setSelectionRange(cursorUtf16, cursorUtf16)
            .setInsertionMarkerLocation(
                x + paddingLeft - scrollX,
                lineTop.toFloat() + paddingTop - scrollY,
                lineBottom.toFloat() + paddingTop - scrollY,
                lineBottom.toFloat() + paddingTop - scrollY,
                android.view.inputmethod.CursorAnchorInfo.FLAG_HAS_VISIBLE_REGION
            )
            .build()
        imm.updateCursorAnchorInfo(this, info)
    }

    // ── #541: Session lifecycle for shared host ──

    var isSessionBound: Boolean = false
        internal set
    private var currentProfile: TextEditorProfile = TextEditorProfile.DocumentBody
    private val isSecretMode: Boolean
        get() = currentProfile.secretPolicy == com.xiwei.sujian.editor.v2.coordinator.SecretPolicy.MASK_AND_CLEAR_ON_COMMIT

    fun getDisplayText(): String {
        if (isSecretMode && isSessionBound) {
            return pipeline.getCurrentProjection().displayText
        }
        return pipeline.getText()
    }
    private var commitOnFocusLoss: Boolean = true
    var onCommitRequested: (() -> Unit)? = null
    var onCancelRequested: (() -> Unit)? = null

    /**
     * Bind this shared host to a new editing session.
     *
     * If a session is already bound, unbinds it first (with reason "rebind"). Then sets
     * the kernel bridge, applies the profile, loads the initial text, and requests focus.
     *
     * Per #541 lifecycle: bindSession → Editing. After this call, [isSessionBound] is true
     * and the host can create a valid InputConnection.
     */
    fun bindSession(
        sessionBridge: EditorKernelBridge,
        profile: TextEditorProfile,
        initialText: String,
        initialCursorUtf8: Int
    ) {
        bindSessionInternal(sessionBridge, profile)
        if (initialText.isNotEmpty()) {
            applyProfileToPipeline(profile, initialText, initialCursorUtf8)
        } else {
            applyProfileToPipeline(profile)
            pipeline.loadText("", 0)
        }
    }

    /**
     * #592 一：附着既有持久会话 — 与 [bindSession] 的区别是绝不对 Rust 调用
     * textEditSessionLoadText：snapshot 的 text/revision/cursor/selection 直接装入
     * Android mirror/layout，Rust revision 不变、Undo/Redo 保留、composition 不重置。
     * 仅用于窗口重建/重新绑定；新正文载入或外部内容重置仍走 [bindSession]/
     * [loadText]。
     */
    fun attachSession(
        sessionBridge: EditorKernelBridge,
        profile: TextEditorProfile,
        text: String,
        revision: Long,
        cursorUtf8: Int,
        selStartUtf8: Int,
        selEndUtf8: Int,
    ) {
        bindSessionInternal(sessionBridge, profile)
        applyProfileToPipeline(profile)
        pipeline.attachSnapshot(text, revision, cursorUtf8, selStartUtf8, selEndUtf8)
    }

    private fun bindSessionInternal(
        sessionBridge: EditorKernelBridge,
        profile: TextEditorProfile,
    ) {
        if (isSessionBound) {
            onContentChanged = null
            onLocalEdit = null
            onCommitRequested = null
            onCancelRequested = null
            unbindSession("rebind")
        }
        kernelBridge = sessionBridge
        currentProfile = profile
        isSessionBound = true
        requestFocus()
    }

    private fun applyProfileToPipeline(profile: TextEditorProfile, initialText: String? = null, initialCursorUtf8: Int = 0) {
        inputAdapter.applyProfile(profile)
        pipeline.setAutoIndent(
            profile.autoIndentPolicy == com.xiwei.sujian.editor.v2.coordinator.AutoIndentPolicy.INDENT_ON_ENTER,
            2f
        )
        inputAdapter.onPerformEditorAction = { _ ->
            if (profile.commitOnImeAction) {
                onCommitRequested?.invoke()
            }
        }
        commitOnFocusLoss = profile.commitOnFocusLoss
        // #595 四：applyProfileToPipeline 只处理 input type、行数、选择、复制粘贴、换行等
        // profile 内容，不再直接写动画开关。动画开关由全局 EditorMotionPolicy 唯一控制，
        // 通过 setTypingAnimationEnabled/setSmoothCursorEnabled/setCoordinatedAnimationEnabled
        // 一次性传入。profile 的 animationPolicy 仅作为约束（SYSTEM_SUPPRESSED → forceStatic），
        // 由 EditorWindowHost 在计算 effectivePolicy 时应用。
        if (profile.cursorPolicy == com.xiwei.sujian.editor.v2.coordinator.CursorPolicy.HIDDEN) {
            pipeline.setCursorVisible(false)
        } else {
            pipeline.setCursorVisible(true)
        }
        if (profile.selectionPolicy == com.xiwei.sujian.editor.v2.coordinator.SelectionPolicy.CURSOR_ONLY) {
            pipeline.setSelectionAllowed(false)
        } else {
            pipeline.setSelectionAllowed(true)
        }
        pipeline.setMaxLength(profile.maxLength)
        pipeline.setCopyAllowed(profile.copyPolicy != com.xiwei.sujian.editor.v2.coordinator.CopyPolicy.BLOCK)
        pipeline.setPasteAllowed(profile.pastePolicy != com.xiwei.sujian.editor.v2.coordinator.PastePolicy.BLOCK)
        val isSecret = profile.secretPolicy == com.xiwei.sujian.editor.v2.coordinator.SecretPolicy.MASK_AND_CLEAR_ON_COMMIT
        if (isSecret) {
            pipeline.setSecretDisplayMode(true)
        } else {
            pipeline.setSecretDisplayMode(false)
        }
        if (initialText != null) {
            pipeline.loadText(initialText, initialCursorUtf8, applySecret = true)
        }
    }

    fun updateEditorProfile(profile: TextEditorProfile) {
        currentProfile = profile
        if (isSessionBound) {
            applyProfileToPipeline(profile)
        }
    }

    /**
     * Unbind the current editing session.
     *
     * Cancels any active animation, invalidates the composition session, clears callbacks,
     * detaches the kernel bridge, releases focus, and hides the soft keyboard.
     * After this call, [isSessionBound] is false and [onCheckIsTextEditor] returns false,
     * so the system will not offer an InputConnection.
     *
     * Per #541 lifecycle: Editing → Released. The host is idle but retains its pipeline
     * infrastructure for later reuse via [bindSession].
     */
    fun unbindSession(@Suppress("UNUSED_PARAMETER") reason: String) {
        if (!isSessionBound) return
        pipeline.cancelActiveTransaction()
        inputAdapter.invalidateCompositionSession()
        inputAdapter.onPerformEditorAction = null
        onContentChanged = null
        onLocalEdit = null
        onCommitRequested = null
        onCancelRequested = null
        kernelBridge = null
        isSessionBound = false
        clearFocus()
        val imm = context.getSystemService(Context.INPUT_METHOD_SERVICE) as? InputMethodManager
        imm?.hideSoftInputFromWindow(windowToken, 0)
    }

    /**
     * Reset transient state for reuse by a different target (session rebind).
     *
     * Clears scroll position, search highlights, and pending layout flags, then delegates
     * to [AndroidEditorPipeline.resetForReuse] which cancels animations and resets the
     * mirror. The pipeline infrastructure (Layout, Planner, Renderer, ResourceStore) is
     * preserved — only target-specific state is cleared.
     *
     * Per #541: this corresponds to the Coordinator's rebind step, where the shared host
     * switches from one EditableTextTarget to another without recreating the full pipeline.
     */
    fun resetForReuse() {
        scrollX = 0f
        scrollY = 0f
        maxScrollY = 0f
        searchHighlights = emptyList()
        pendingLayoutNeeded = false
        pipeline.resetForReuse()
        invalidate()
    }

    fun updateHostGeometry(width: Float, height: Float) {
        if (width > 0 && height > 0 && (width.toInt() != this.width || height.toInt() != this.height)) {
            requestLayout()
        }
    }

    /**
     * Soft reset for persistent sessions on commit (per #541).
     *
     * Invalidates the composition session, but does NOT close the Rust EditorKernel
     * session or reset the mirror. The Undo/Redo stack and revision history survive
     * across commits — the persistent session remains bound and can continue editing.
     * This contrasts with [unbindSession] (used for draft sessions), which detaches the
     * bridge and closes the Rust session entirely.
     *
     * The active animation is cancelled only while a composition (preedit) is in flight:
     * the preedit overlay is being removed and any in-progress composition animation
     * would reference stale preedit state. For a plain text commit the animation stays
     * visually valid — its final state equals the committed text's layout — so it must
     * not be cancelled: commits can be triggered by incidental focus churn (e.g. IME
     * settle while the user keeps typing), and cancelling the animation there would
     * destroy the transaction mid-flight and snap the display to the static text.
     */
    fun softResetForPersistentCommit() {
        if (inputAdapter.isComposing()) {
            pipeline.cancelActiveTransaction()
        }
        inputAdapter.invalidateCompositionSession()
    }

    /**
     * Final resource release when the host leaves the composition tree permanently.
     *
     * Unbinds the session and releases all Bitmap resources in the VisualResourceStore.
     * After this call the host cannot be reused — a new SujianEditorView must be created.
     *
     * Per #541: corresponds to Compose AndroidView's onRelease lifecycle, as opposed to
     * onReset (which would call [resetForReuse] instead).
     */
    fun release() {
        unbindSession("release")
        pipeline.releaseAnimationResources()
    }
}

/**
 * Bridge contract between the platform pipeline and the Rust EditorKernel.
 *
 * Per #541: this interface will become session-scoped (TextEditSessionBridge) so that
 * each bound EditableTextTarget carries its own session ID. All commands must implicitly
 * or explicitly carry the current session ID; the bridge must not use a global singleton.
 *
 * Byte offset convention: all offsets are UTF-8 byte offsets using half-open intervals
 * [start, endExclusive). The bridge is responsible for converting to the Rust FFI
 * unsigned integer types (UInt/ULong) at the boundary.
 */
interface EditorKernelBridge {
    fun insert(byteOffset: Int, text: String, cause: uniffi.writer_core.EditorTransactionCauseDto, expectedRevision: Long): uniffi.writer_core.EditorEditResultDto?
    fun delete(byteStart: Int, byteEndExclusive: Int, cause: uniffi.writer_core.EditorTransactionCauseDto, expectedRevision: Long): uniffi.writer_core.EditorEditResultDto?
    fun replace(byteStart: Int, byteEndExclusive: Int, replacementText: String, originalText: String, cause: uniffi.writer_core.EditorTransactionCauseDto, expectedRevision: Long): uniffi.writer_core.EditorEditResultDto?
    fun setSelection(anchorByteOffset: Int, headByteOffset: Int, expectedRevision: Long): uniffi.writer_core.EditorEditResultDto?
    fun undo(expectedRevision: Long): uniffi.writer_core.EditorEditResultDto?
    fun redo(expectedRevision: Long): uniffi.writer_core.EditorEditResultDto?
    fun loadText(text: String, cursorUtf8: Int): uniffi.writer_core.EditorEditResultDto?
    fun commitText(
        byteStart: Int,
        byteEndExclusive: Int,
        replacementText: String,
        resultingSelectionAnchor: Int,
        resultingSelectionHead: Int,
        compositionSessionId: Long,
        compositionBaseRevision: Long,
        compositionGeneration: Long,
        cause: uniffi.writer_core.EditorTransactionCauseDto,
        expectedRevision: Long
    ): uniffi.writer_core.EditorEditResultDto?
    fun deleteSurrounding(
        beforeByteStart: Int,
        beforeByteEndExclusive: Int,
        afterByteStart: Int,
        afterByteEndExclusive: Int,
        cause: uniffi.writer_core.EditorTransactionCauseDto,
        expectedRevision: Long
    ): uniffi.writer_core.EditorEditResultDto?
    fun beginComposition(
        replaceStart: Int,
        replaceEndExclusive: Int,
        expectedRevision: Long
    ): uniffi.writer_core.EditorEditResultDto?
    fun updateComposition(
        compositionSessionId: Long,
        compositionGeneration: Long,
        newPreeditText: String,
        newPreeditCursorOffset: Int,
        expectedRevision: Long
    ): uniffi.writer_core.EditorEditResultDto?
    fun finishComposition(
        compositionSessionId: Long,
        compositionGeneration: Long,
        expectedRevision: Long
    ): uniffi.writer_core.EditorEditResultDto?
    fun cancelComposition(
        compositionSessionId: Long,
        compositionGeneration: Long,
        expectedRevision: Long
    ): uniffi.writer_core.EditorEditResultDto?
    fun compositionUpdateVisualIntent(
        compositionReplaceStart: UInt,
        compositionReplaceEndExclusive: UInt,
        oldPreeditText: String,
        newPreeditText: String
    ): uniffi.writer_core.EditorVisualIntentDto?
    fun setAnimationEnabled(enabled: Boolean)
    fun setAnimationDurationMs(durationMs: Long)
    fun replaceAll(search: String, replacement: String, expectedRevision: Long): uniffi.writer_core.EditorEditResultDto?
    fun insertLineBreak(byteOffset: Int, autoIndentPrefix: String, cause: uniffi.writer_core.EditorTransactionCauseDto, expectedRevision: Long): uniffi.writer_core.EditorEditResultDto?
    fun sessionSnapshot(): uniffi.writer_core.EditorSessionSnapshotDto?
}
