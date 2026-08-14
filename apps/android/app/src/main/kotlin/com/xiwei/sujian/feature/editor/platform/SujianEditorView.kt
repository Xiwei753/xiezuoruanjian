package com.xiwei.sujian.feature.editor.platform

import android.annotation.SuppressLint
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
import androidx.core.graphics.withTranslation
import com.xiwei.sujian.R
import com.xiwei.sujian.feature.editor.input.AndroidInputAdapter
import com.xiwei.sujian.feature.editor.interop.EditorKernelBridge
import com.xiwei.sujian.feature.editor.pipeline.AndroidEditorPipeline
import com.xiwei.sujian.feature.editor.pipeline.EditorCommandPort
import com.xiwei.sujian.feature.editor.pipeline.PipelineOutput
import com.xiwei.sujian.feature.editor.session.EditorAppliedEvent
import com.xiwei.sujian.feature.editor.session.EditorContentDelta
import com.xiwei.sujian.feature.editor.session.EditorOperationKind
import com.xiwei.sujian.feature.editor.session.NewlinePolicy
import com.xiwei.sujian.feature.editor.session.TextEditorProfile
import com.xiwei.sujian.feature.editor.session.toEditorOperationKind
import com.xiwei.sujian.feature.editor.session.toSessionDelta
import com.xiwei.sujian.feature.editor.window.WindowDisplayFrameClock
import uniffi.writer_core.EditorTransactionCauseDto

class SujianEditorView
    @JvmOverloads
    constructor(
        context: Context,
        attrs: AttributeSet? = null,
        defStyleAttr: Int = 0,
        animationTimeSource: com.xiwei.sujian.feature.editor.visual.AnimationTimeSource =
            com.xiwei.sujian.feature.editor.visual.ChoreographerAnimationTimeSource(),
        transactionIdSource: com.xiwei.sujian.feature.editor.visual.TransactionIdSource =
            com.xiwei.sujian.feature.editor.visual.TransactionIdSource(),
    ) : View(context, attrs, defStyleAttr), WindowDisplayFrameClock.FrameListener {
        private val textPaint =
            TextPaint().apply {
                textSize = 48f
                isAntiAlias = true
            }
        private val timeSource = animationTimeSource
        private val pipeline =
            AndroidEditorPipeline.create(
                com.xiwei.sujian.feature.editor.projection.DisplayTextMirror(),
                textPaint,
                animationTimeSource,
                transactionIdSource,
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
        fun setScrollPosition(
            sx: Float,
            sy: Float,
        ) {
            scrollX = sx.coerceAtLeast(0f)
            scrollY = sy.coerceAtLeast(0f)
            invalidate()
        }

        private var searchHighlights: List<Pair<Int, Int>> = emptyList()

        // 预分配以避免 onDraw 中每帧分配（DrawAllocation）。
        private val searchHighlightsUtf16Buffer: MutableList<Pair<Int, Int>> = ArrayList()
        private var frameClock: WindowDisplayFrameClock? = null
        private var isRegisteredWithClock: Boolean = false

        @Volatile
        private var pendingFrameTimeNanos: Long = Long.MIN_VALUE

        // #623 评论 1：重新获得窗口焦点时不立即 resume，而是等到下一个真实 VSync 帧
        // （onFrame）再用新的 frameTimeNanos 恢复时间线，避免拿旧缓存帧时间立即 resume。
        @Volatile
        private var pendingResume: Boolean = false

        var kernelBridge: EditorKernelBridge?
            get() = pipeline.kernelBridge
            set(value) {
                pipeline.kernelBridge = value
            }

        private var _themeBackgroundColor: Int = Color.WHITE

        fun getThemeBackgroundColor(): Int = _themeBackgroundColor

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

        fun insertText(
            byteOffset: Int,
            text: String,
            cause: EditorTransactionCauseDto = EditorTransactionCauseDto.TYPING,
        ) {
            val output = pipeline.insertText(byteOffset, text, cause)
            handlePipelineOutput(output)
        }

        fun deleteRange(
            byteStart: Int,
            byteEndExclusive: Int,
            cause: EditorTransactionCauseDto = EditorTransactionCauseDto.DELETE,
        ) {
            val output = pipeline.deleteRange(byteStart, byteEndExclusive, cause)
            handlePipelineOutput(output)
        }

        fun replaceRangeTyped(
            byteStart: Int,
            byteEndExclusive: Int,
            replacementText: String,
            originalText: String,
            cause: EditorTransactionCauseDto = EditorTransactionCauseDto.TYPING,
            beforePatch: (() -> Unit)? = null,
            source: EditorEditSource = EditorEditSource.NORMAL,
        ) {
            val output =
                pipeline.replaceRangeTyped(
                    byteStart,
                    byteEndExclusive,
                    replacementText,
                    originalText,
                    cause,
                    beforePatch,
                    source,
                )
            handlePipelineOutput(output)
        }

        fun setSelectionTyped(
            anchorByteOffset: Int,
            headByteOffset: Int,
            source: EditorEditSource = EditorEditSource.NORMAL,
        ) {
            val output = pipeline.setSelectionTyped(anchorByteOffset, headByteOffset, source)
            handlePipelineOutput(output)
        }

        fun performUndo() {
            // #595 四：来源由 PipelineOutput 天然携带（UNDO），无可变侧信道。
            val output = pipeline.performUndo()
            handlePipelineOutput(output)
        }

        fun performRedo() {
            // #595 四：来源由 PipelineOutput 天然携带（REDO），无可变侧信道。
            val output = pipeline.performRedo()
            handlePipelineOutput(output)
        }

        private fun handlePipelineOutput(
            output: PipelineOutput,
            suppressContentCallback: Boolean = false,
        ) {
            handlePipelineOutputInternal(output, suppressContentCallback)
        }

        fun handlePipelineOutput(output: PipelineOutput) {
            handlePipelineOutputInternal(output, false)
        }

        private fun handlePipelineOutputInternal(
            output: PipelineOutput,
            suppressContentCallback: Boolean = false,
        ) {
            when (output) {
                is PipelineOutput.Edited -> {
                    updateMaxScroll()
                    scrollY = scrollY.coerceIn(0f, maxScrollY)
                    if (!suppressContentCallback && output.result.isApplied()) {
                        // #624 评论8/9：热路径不传整章 String — 只构造轻量 EditorAppliedEvent。
                        // contentChanged = displayPatches 非空；contentDelta 直接消费
                        // Core EditorEditResultDto.contentDelta 真值（Unicode scalar 计数），
                        // 不再从 patch 的 UTF-8 byte 长度推算 deletedChars。
                        val source = output.source
                        val contentChanged = output.result.displayPatches.isNotEmpty()
                        val contentDelta = output.result.contentDelta.toSessionDelta()
                        val event =
                            EditorAppliedEvent(
                                revision = output.result.newRevision,
                                transactionId = output.result.transactionId,
                                operationKind = output.result.visualIntent.operationKind.toEditorOperationKind(),
                                source = source,
                                // #624 评论10 第5项：cause 从 Core VisualIntent.cause 真值填入 —
                                // 统计层按此明确分类，不再靠 source/operationKind 猜。
                                cause = output.result.visualIntent.cause,
                                contentChanged = contentChanged,
                                contentDelta = contentDelta,
                                selectionAnchorUtf8 = pipeline.getSelectionStartUtf8(),
                                selectionHeadUtf8 = pipeline.getSelectionEndUtf8(),
                            )
                        // #595 二/四：根据输出携带的来源分派 — 撤销/恢复/程序化替换走
                        // onExternalEdit，普通输入走 onLocalEdit。来源在命令与
                        // PipelineOutput 本身携带，不再使用 pendingEditSource 可变侧信道。
                        // #595 五：selection-only 操作（CURSOR_ONLY）没有 displayPatches，
                        // 但会话层 selection 必须更新 — 回调不受 displayPatches 门控。
                        if (source != EditorEditSource.NORMAL) {
                            onExternalEdit?.invoke(event)
                        } else {
                            onLocalEdit?.invoke(event)
                        }
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
                    "reloadFromKernel applied; mirror='${pipeline.getText()}' rev=${pipeline.getRevision()} " +
                        "cursor=${pipeline.getCursorUtf8()}",
                )
                updateLayoutConfig()
                // #624 评论9：reloadFromKernel 是冷路径（NeedReload/Stale 后整章重装），
                // 仍用 pipeline.getText() 构造轻量 EditorAppliedEvent（contentChanged=true）。
                val reloadText = pipeline.getText()
                val event =
                    EditorAppliedEvent(
                        revision = pipeline.getRevision(),
                        transactionId = 0L,
                        operationKind = EditorOperationKind.REPLACE,
                        source = EditorEditSource.NORMAL,
                        // #624 评论10 第5项：reloadFromKernel 是冷路径整章重装 —
                        // cause=Load 表示非人工输入（统计层映射为 programmatic）。
                        cause = EditorTransactionCauseDto.LOAD,
                        contentChanged = true,
                        contentDelta =
                            EditorContentDelta(
                                insertedChars = reloadText.length,
                                insertedNonWhitespaceChars = reloadText.count { !it.isWhitespace() },
                            ),
                        selectionAnchorUtf8 = pipeline.getSelectionStartUtf8(),
                        selectionHeadUtf8 = pipeline.getSelectionEndUtf8(),
                    )
                onLocalEdit?.invoke(event)
            } else {
                android.util.Log.w("SujianEditorInput", "reloadFromKernel FAILED (no session snapshot)")
            }
        }

        fun getText(): String = pipeline.getText()

        /**
         * #624 评论3：排版设置原子入口 — 一次更新 TextPaint / runtime config，
         * 最后只推进一次布局。字号、行距、首行缩进（开关 + 字符宽度）由
         * [EditorWindowHost.applyEditorTypography] 持续应用到当前共享 View；
         * 设置变化后当前正文立即重排，不重建编辑 session。
         */
        fun applyLayoutConfig(
            fontSizeSp: Float,
            lineSpacingMultiplier: Float,
            firstLineIndentEnabled: Boolean,
            firstLineIndentWidthChars: Float,
        ) {
            textPaint.textSize = fontSizeSp * resources.displayMetrics.scaledDensity
            this.lineSpacingMultiplier = lineSpacingMultiplier
            pipeline.setLineSpacingMultiplier(lineSpacingMultiplier)
            pipeline.setFirstLineIndent(firstLineIndentEnabled, firstLineIndentWidthChars)
            updateLayoutConfig()
        }

        private var lineSpacingMultiplier: Float = 1.0f

        fun getSelectionStart(): Int = pipeline.getSelectionStartUtf8()

        fun getSelectionEnd(): Int = pipeline.getSelectionEndUtf8()

        fun getSelectionStartUtf16(): Int = pipeline.getSelectionStartUtf16()

        fun getSelectionEndUtf16(): Int = pipeline.getSelectionEndUtf16()

        fun setSelectionRange(
            start: Int,
            end: Int,
        ) {
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

        fun replaceRange(
            start: Int,
            end: Int,
            newText: String,
        ) {
            // #595 四：程序化替换入口显式携带 PROGRAMMATIC 来源 —
            // 其他程序化入口不再误走普通本地输入路径。
            replaceRangeTyped(
                start,
                end,
                newText,
                "",
                EditorTransactionCauseDto.PROGRAMMATIC,
                source = EditorEditSource.PROGRAMMATIC,
            )
        }

        fun replaceAll(
            searchStr: String,
            replaceStr: String,
        ) {
            // #595 四：来源由 PipelineOutput 天然携带（PROGRAMMATIC），无可变侧信道。
            val output = pipeline.replaceAll(searchStr, replaceStr)
            if (output != null) {
                handlePipelineOutput(output)
            }
        }

        fun applyCompositionCommit(dto: uniffi.writer_core.EditorEditResultDto) {
            val output = pipeline.applyCompositionCommit(dto)
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

        override fun onSizeChanged(
            w: Int,
            h: Int,
            oldw: Int,
            oldh: Int,
        ) {
            super.onSizeChanged(w, h, oldw, oldh)
            // #624 评论4：只在真实正文宽度（可绘制宽度）改变时才更新布局配置 —
            // 不要在每次输入时把当前 view width 当成新布局源。
            if (w > 0) {
                val contentWidth = (w - paddingLeft - paddingRight).coerceAtLeast(1)
                if (contentWidth != lastContentWidthPx) {
                    lastContentWidthPx = contentWidth
                    updateLayoutConfig()
                }
            }
        }

        private var lastContentWidthPx: Int = 0

        private fun updateMaxScroll() {
            val layoutOverflow = pipeline.getLayoutMaxScrollY(height)
            maxScrollY =
                if (layoutOverflow > 0f) {
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
        // 已用预分配 ArrayList 减少 onDraw 分配；完全消除需改 drawFrame 接口（List<Pair> → IntArray），
        // 超出 lint 清理范围。搜索高亮通常少量项，剩余 Pair 分配可忽略。

        /**
         * Main rendering loop: draw one frame.
         *
         * Animation-driven invalidation is now handled by [WindowDisplayFrameClock]:
         * the clock calls [onFrame] which triggers [invalidate], creating a
         * self-sustaining frame loop as long as [needsFrame] returns true.
         * When the animation completes, the clock stops naturally.
         */
        @SuppressLint("DrawAllocation")
        override fun onDraw(canvas: Canvas) {
            super.onDraw(canvas)
            canvas.withTranslation(
                (paddingLeft - scrollX).toFloat(),
                (paddingTop - scrollY).toFloat(),
            ) {
                searchHighlightsUtf16Buffer.clear()
                for ((startUtf8, endUtf8) in searchHighlights) {
                    searchHighlightsUtf16Buffer.add(
                        Pair(pipeline.utf8ToUtf16(startUtf8), pipeline.utf8ToUtf16(endUtf8)),
                    )
                }
                val searchHighlightsUtf16 = searchHighlightsUtf16Buffer
                val frameTimeNanos = pendingFrameTimeNanos
                if (frameTimeNanos != Long.MIN_VALUE) {
                    pendingFrameTimeNanos = Long.MIN_VALUE
                    pipeline.drawFrame(canvas, searchHighlightsUtf16, width, height, scrollX, scrollY, frameTimeNanos)
                } else {
                    pipeline.drawFrame(canvas, searchHighlightsUtf16, width, height, scrollX, scrollY)
                }
            }
        }

        // #595 六：暂停时不持续请求 VSync — hasActiveAnimation && !isAnimationPaused。
        // 暂停时停止重投 Choreographer.postFrameCallback()，恢复时显式 requestFrame()。
        override fun needsFrame(): Boolean = pipeline.hasActiveAnimation() && !pipeline.isAnimationPaused()

        override fun onFrame(frameTimeNanos: Long) {
            pendingFrameTimeNanos = frameTimeNanos
            if (timeSource is com.xiwei.sujian.feature.editor.visual.ChoreographerAnimationTimeSource) {
                timeSource.onFrameTimeNanos(frameTimeNanos)
            }
            // 重新获得焦点后在第一个真实 VSync 帧恢复时间线，
            // 不用旧缓存帧时间立即 resume（#623 评论 1）。
            if (pendingResume) {
                pendingResume = false
                pipeline.resumeAnimation(frameTimeNanos / 1_000_000)
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
            // #624 评论5：会话未绑定直接返回 null — 系统拿到的 InputConnection
            // 必须属于当前真实 session。绑定完成并取得焦点后由 attachSession
            // 的 restartInput 让系统重新查询，不再出现 created=true sessionBound=false。
            if (!isSessionBound) return null
            val ic = inputAdapter.onCreateInputConnection(outAttrs)
            com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.inputConnection(
                created = ic != null,
                sessionBound = isSessionBound,
            )
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
            val selStart = pipeline.getSelectionStartUtf16()
            val selEnd = pipeline.getSelectionEndUtf16()
            if (selStart >= 0 && selEnd >= 0) {
                info?.setTextSelection(selStart, selEnd)
            }
            info?.addAction(
                android.view.accessibility.AccessibilityNodeInfo.AccessibilityAction.ACTION_SET_TEXT,
            )
        }

        override fun performAccessibilityAction(
            action: Int,
            arguments: android.os.Bundle?,
        ): Boolean {
            if (action == android.view.accessibility.AccessibilityNodeInfo.ACTION_SET_TEXT) {
                if (!isSessionBound) return false
                val text =
                    arguments?.getCharSequence(
                        android.view.accessibility.AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE,
                    )?.toString() ?: return false
                val currentText = pipeline.getText()
                val currentByteLen = currentText.toByteArray(Charsets.UTF_8).size
                replaceRangeTyped(0, currentByteLen, text, currentText, EditorTransactionCauseDto.PROGRAMMATIC)
                val endByteOffset = text.toByteArray(Charsets.UTF_8).size
                setSelectionTyped(endByteOffset, endByteOffset)
                val event =
                    android.view.accessibility.AccessibilityEvent.obtain(
                        android.view.accessibility.AccessibilityEvent.TYPE_VIEW_TEXT_CHANGED,
                    )
                event.text.add(text)
                event.fromIndex = 0
                event.removedCount = currentText.length
                event.addedCount = text.length
                event.className = android.widget.EditText::class.java.name
                event.packageName = context.packageName
                event.setSource(this)
                val accessibilityManager =
                    context.getSystemService(
                        android.content.Context.ACCESSIBILITY_SERVICE,
                    ) as? android.view.accessibility.AccessibilityManager
                if (accessibilityManager?.isEnabled == true) {
                    parent?.requestSendAccessibilityEvent(this, event)
                } else {
                    event.recycle()
                }
                return true
            }
            return super.performAccessibilityAction(action, arguments)
        }

        override fun performClick(): Boolean {
            super.performClick()
            return true
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
                        performClick()
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

        private fun handleTap(
            x: Float,
            y: Float,
        ) {
            val line = pipeline.getLayoutLineForVertical(y.toInt())
            val offset = pipeline.getLayoutOffsetForHorizontal(line, x)
            val byteOffset = pipeline.utf16ToUtf8(offset)
            setSelectionTyped(byteOffset, byteOffset)
            showSoftInput()
        }

        override fun onKeyDown(
            keyCode: Int,
            event: KeyEvent,
        ): Boolean {
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
                    // #624 评论7：与 KEYCODE_DEL 一致 — 有活动 composition 时先取消，
                    // 不在 overlay 状态下执行前向删除。
                    if (inputAdapter.isComposing()) {
                        inputAdapter.handleCompositionCancel()
                        return true
                    }
                    val selStart = pipeline.getSelectionStartUtf8()
                    val selEnd = pipeline.getSelectionEndUtf8()
                    if (selStart != selEnd) {
                        replaceRange(selStart, selEnd, "")
                    } else {
                        // #624 评论7：用 O(1) committed UTF-8 长度判断光标是否在文末，
                        // 不再 getText().toByteArray() 整章复制。composition 已在上文取消，
                        // display text == committed text，偏移语义一致。
                        val textLen = pipeline.getCommittedTextLengthUtf8()
                        if (selEnd < textLen) {
                            val nextGraphemeLen = pipeline.nextGraphemeByteLen(selEnd)
                            if (nextGraphemeLen > 0) {
                                replaceRange(selEnd, selEnd + nextGraphemeLen, "")
                            }
                        }
                    }
                    return true
                }
                KeyEvent.KEYCODE_ENTER -> handleEnterKey()
            }
            return super.onKeyDown(keyCode, event)
        }

        /**
         * #624 评论2：硬件键盘 Enter 与软键盘 commitText("\n") 共用同一个
         * insertLineBreak()；NewlinePolicy.FORBID 才消费而不写入。
         */
        private fun handleEnterKey(): Boolean {
            if (currentProfile.newlinePolicy == NewlinePolicy.FORBID) {
                if (currentProfile.commitOnImeAction) {
                    onCommitRequested?.invoke()
                }
                return true
            }
            if (inputAdapter.isComposing()) {
                // 先提交未完成的 preedit，再插入换行 — 不能丢弃正在输入的内容。
                inputAdapter.handleCompositionFinish()
            }
            val output = pipeline.insertLineBreak(EditorTransactionCauseDto.TYPING)
            handlePipelineOutput(output)
            return true
        }

        override fun onFocusChanged(
            gained: Boolean,
            direction: Int,
            previouslyFocusedRect: android.graphics.Rect?,
        ) {
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
            com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.editorFocus(hasWindowFocus)
            if (!hasWindowFocus) {
                // 失焦可以用最后一帧暂停 — 保存当前可见帧，不永久取消事务。
                val pauseTimeMs = (timeSource.lastFrameTimeNanos() ?: timeSource.nowNanos()) / 1_000_000
                pipeline.pauseAnimation(pauseTimeMs)
            } else {
                // 重新获得焦点时不要拿旧缓存立即 resume，
                // 先 requestAnimationFrame()，在下一次真实 onFrame 里用新的 frameTimeNanos 恢复时间线。
                pendingResume = true
                if (pipeline.hasActiveAnimation()) {
                    requestAnimationFrame()
                }
            }
        }

        fun requestNextFrame() {
            invalidate()
        }

        fun getPipeline(): EditorCommandPort = pipeline

        /**
         * #595 一 / #624 评论9：类型化本地编辑回调 — 传递轻量 [EditorAppliedEvent]。
         *
         * 由 [EditorWindowHost.installContentCallback] 设置，回调中先调用
         * [EditorSessionCoordinator.applyLocalEdit] 更新唯一 SessionState，
         * 再通知 ViewModel 保存。热路径不传整章 String。
         */
        var onLocalEdit: ((EditorAppliedEvent) -> Unit)? = null

        /**
         * #595 二 / #624 评论9：类型化外部编辑回调 — 撤销/恢复/程序化替换产生
         * EditResult 后调用，传递轻量 [EditorAppliedEvent]。
         *
         * 由 [EditorWindowHost.installContentCallback] 设置，回调中根据 event.source
         * 构造 [EditorDocumentUpdate.UndoRestored] 或 [EditorDocumentUpdate.ProgrammaticReplace]，
         * 调用 [EditorSessionCoordinator.applyExternalUpdate] 更新唯一 SessionState。
         */
        var onExternalEdit: ((EditorAppliedEvent) -> Unit)? = null

        fun setTypingAnimationEnabled(
            enabled: Boolean,
            durationMs: Long,
        ) {
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
                if (enabled) {
                    com.xiwei.sujian.feature.editor.visual.TextAnimationPolicy.ENABLED
                } else {
                    com.xiwei.sujian.feature.editor.visual.TextAnimationPolicy.SYSTEM_SUPPRESSED
                },
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

        fun setSmoothCursorEnabled(
            enabled: Boolean,
            durationMs: Long,
        ) {
            smoothCursorEnabled = enabled
            smoothCursorDurationMs = durationMs
            pipeline.setSmoothCursor(enabled, durationMs)
        }

        fun isSmoothCursorEnabled(): Boolean = smoothCursorEnabled

        // #624 评论3：首行缩进显示样式（开关 + 字符宽度）只经 applyLayoutConfig 原子入口
        // （一次更新 TextPaint / runtime config，最后只推进一次布局）到达 pipeline —
        // 不再保留独立的 setFirstLineIndent 第二入口（零调用方，且会多一次整篇布局推进）。

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

        fun applyThemeColorsFromAdapter(colors: com.xiwei.sujian.feature.editor.ui.theme.EditorThemeColors) {
            _themeBackgroundColor = colors.background
            textPaint.color = colors.text
            pipeline.setRendererThemeColors(
                textColor = colors.text,
                cursorColor = colors.cursor,
                selectionColor = colors.selection,
                preeditColor = colors.composing,
                bgColor = colors.background,
                searchHighlightColor = colors.searchHighlight,
            )
            invalidate()
        }

        private fun showSoftInput() {
            val imm = context.getSystemService(Context.INPUT_METHOD_SERVICE) as? InputMethodManager
            imm?.showSoftInput(this, 0)
        }

        fun notifyCursorAnchorInfo() {
            val imm =
                context.getSystemService(android.content.Context.INPUT_METHOD_SERVICE)
                    as? android.view.inputmethod.InputMethodManager ?: return
            val cursorUtf16 = pipeline.getDisplayCursorUtf16()
            val layoutTextLen = pipeline.getLengthUtf16()
            if (cursorUtf16 < 0 || cursorUtf16 > layoutTextLen) return

            val line = pipeline.getLayoutLineForOffset(cursorUtf16)
            val x = pipeline.getLayoutPrimaryHorizontal(cursorUtf16)
            val lineTop = pipeline.getLayoutLineTop(line)
            val lineBottom = pipeline.getLayoutLineBottom(line)

            val info =
                android.view.inputmethod.CursorAnchorInfo.Builder()
                    .setSelectionRange(cursorUtf16, cursorUtf16)
                    .setInsertionMarkerLocation(
                        x + paddingLeft - scrollX,
                        lineTop.toFloat() + paddingTop - scrollY,
                        lineBottom.toFloat() + paddingTop - scrollY,
                        lineBottom.toFloat() + paddingTop - scrollY,
                        android.view.inputmethod.CursorAnchorInfo.FLAG_HAS_VISIBLE_REGION,
                    )
                    .build()
            imm.updateCursorAnchorInfo(this, info)
        }

        // ── #541: Session lifecycle for shared host ──

        var isSessionBound: Boolean = false
            internal set
        private var currentProfile: TextEditorProfile = TextEditorProfile.DocumentBody
        private val isSecretMode: Boolean
            get() =
                currentProfile.secretPolicy ==
                    com.xiwei.sujian.feature.editor.session.SecretPolicy.MASK_AND_CLEAR_ON_COMMIT

        fun getDisplayText(): String {
            if (isSecretMode && isSessionBound) {
                return pipeline.getCurrentProjection().displayText.toString()
            }
            return pipeline.getText()
        }

        private var commitOnFocusLoss: Boolean = true
        var onCommitRequested: (() -> Unit)? = null
        var onCancelRequested: (() -> Unit)? = null

        /**
         * #592 一：附着既有持久会话 — 绝不对 Rust 调用 textEditSessionLoadText：
         * snapshot 的 text/revision/cursor/selection 直接装入
         * Android mirror/layout，Rust revision 不变、Undo/Redo 保留、composition 不重置。
         * 仅用于窗口重建/重新绑定；#595 二：新建 session 也走本路径（createSession 已把
         * 初始正文装入 kernel，是唯一一次 Core 命令）。
         * 返回 true（本地 mirror 装载不会失败）。
         *
         * #595 三：已删除会触发第二次 Core loadText 的 bindSession 入口 —
         * 本方法是 session 绑定的唯一路径，调用方必须携带真实 snapshot。
         */
        fun attachSession(
            sessionBridge: EditorKernelBridge,
            profile: TextEditorProfile,
            text: String,
            revision: Long,
            cursorUtf8: Int,
            selStartUtf8: Int,
            selEndUtf8: Int,
        ): Boolean {
            bindSessionInternal(sessionBridge, profile)
            applyProfileToPipeline(profile)
            pipeline.attachSnapshot(text, revision, cursorUtf8, selStartUtf8, selEndUtf8)
            // #624 评论5：绑定完成并取得焦点后重启输入连接 — 系统重新查询
            // onCreateInputConnection，保证拿到的 InputConnection 属于当前真实
            // session（未绑定阶段 onCreateInputConnection 已返回 null）。
            post {
                requestFocus()
                if (isSessionBound) {
                    val imm =
                        context.getSystemService(Context.INPUT_METHOD_SERVICE)
                            as? InputMethodManager
                    imm?.restartInput(this)
                }
            }
            return true
        }

        /**
         * #595 二：同一 session 的外部内容重置 — 不解除既有绑定。
         *
         * 与 [attachSession] 的区别：不调用 bindSessionInternal（不解绑/不清回调/
         * 不隐藏 IME/不丢焦点）。Core 侧 reset 已把新正文装入同一 kernel session，
         * 这里只把真实 snapshot 重装到本地 mirror/layout。外部替换发生在用户
         * 正在输入时，解绑重绑会闪 IME 并破坏输入法状态。
         */
        fun attachSnapshotSameSession(
            sessionBridge: EditorKernelBridge,
            profile: TextEditorProfile,
            text: String,
            revision: Long,
            cursorUtf8: Int,
            selStartUtf8: Int,
            selEndUtf8: Int,
        ) {
            kernelBridge = sessionBridge
            currentProfile = profile
            pipeline.attachSnapshot(text, revision, cursorUtf8, selStartUtf8, selEndUtf8)
            updateLayoutConfig()
            invalidate()
        }

        private fun bindSessionInternal(
            sessionBridge: EditorKernelBridge,
            profile: TextEditorProfile,
        ) {
            if (isSessionBound) {
                onLocalEdit = null
                onExternalEdit = null
                onCommitRequested = null
                onCancelRequested = null
                unbindSession("rebind")
            }
            kernelBridge = sessionBridge
            currentProfile = profile
            isSessionBound = true
            requestFocus()
        }

        private fun applyProfileToPipeline(profile: TextEditorProfile): Boolean {
            inputAdapter.applyProfile(profile)
            // #624 评论3：不再硬编码 setAutoIndent(..., 2f) — DocumentBody 只声明
            // “允许首行缩进样式”，实际开关和宽度由设置里的 autoIndentEnabled /
            // autoIndentWidth 经 EditorWindowHost.applyEditorTypography 持续应用。
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
            if (profile.cursorPolicy == com.xiwei.sujian.feature.editor.session.CursorPolicy.HIDDEN) {
                pipeline.setCursorVisible(false)
            } else {
                pipeline.setCursorVisible(true)
            }
            if (profile.selectionPolicy == com.xiwei.sujian.feature.editor.session.SelectionPolicy.CURSOR_ONLY) {
                pipeline.setSelectionAllowed(false)
            } else {
                pipeline.setSelectionAllowed(true)
            }
            pipeline.setMaxLength(profile.maxLength)
            pipeline.setCopyAllowed(profile.copyPolicy != com.xiwei.sujian.feature.editor.session.CopyPolicy.BLOCK)
            pipeline.setPasteAllowed(profile.pastePolicy != com.xiwei.sujian.feature.editor.session.PastePolicy.BLOCK)
            val isSecret =
                profile.secretPolicy ==
                    com.xiwei.sujian.feature.editor.session.SecretPolicy.MASK_AND_CLEAR_ON_COMMIT
            if (isSecret) {
                pipeline.setSecretDisplayMode(true)
            } else {
                pipeline.setSecretDisplayMode(false)
            }
            return true
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
        fun unbindSession(
            @Suppress("UNUSED_PARAMETER") reason: String,
        ) {
            if (!isSessionBound) return
            pipeline.cancelActiveTransaction()
            inputAdapter.invalidateCompositionSession()
            inputAdapter.onPerformEditorAction = null
            onLocalEdit = null
            onExternalEdit = null
            onCommitRequested = null
            onCancelRequested = null
            kernelBridge = null
            isSessionBound = false
            clearFocus()
            val imm = context.getSystemService(Context.INPUT_METHOD_SERVICE) as? InputMethodManager
            imm?.hideSoftInputFromWindow(windowToken, 0)
        }

        /**
         * #624 评论5：进入设置等导航动作前先立刻收 IME — 清焦点 + 隐藏输入法窗口。
         * 由导航套件在把 Settings 放进 back stack 之前调用，不等 AndroidView
         * onRelease → unbindSession 晚一拍才 hide keyboard。
         */
        fun dismissImeForNavigation() {
            clearFocus()
            androidx.core.view.ViewCompat.getWindowInsetsController(this)
                ?.hide(androidx.core.view.WindowInsetsCompat.Type.ime())
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
            pipeline.resetForReuse()
            invalidate()
        }

        fun updateHostGeometry(
            width: Float,
            height: Float,
        ) {
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
 * #595 二/四：编辑来源标记 — 区分普通输入、撤销/恢复、程序化替换。
 * 由 pipeline 命令与 [PipelineOutput.Edited.source] 天然携带，
 * 不再使用 View 上的可变侧信道标记。
 */
enum class EditorEditSource {
    NORMAL,
    UNDO,
    REDO,
    PROGRAMMATIC,
}
