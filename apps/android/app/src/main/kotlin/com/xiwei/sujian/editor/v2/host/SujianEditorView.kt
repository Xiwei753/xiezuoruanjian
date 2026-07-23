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
import uniffi.writer_core.EditorTransactionCauseDto

class SujianEditorView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
    defStyleAttr: Int = 0
) : View(context, attrs, defStyleAttr) {

    private val textPaint = TextPaint().apply {
        textSize = 48f
        isAntiAlias = true
    }
    private val pipeline = AndroidEditorPipeline.create(
        com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror(),
        textPaint
    )
    private val inputAdapter = AndroidInputAdapter(pipeline.mirror, pipeline)

    private var scrollX: Float = 0f
    private var scrollY: Float = 0f
    private var maxScrollY: Float = 0f

    fun getScrollXPos(): Float = scrollX
    fun getScrollYPos(): Float = scrollY
    private var searchHighlights: List<Pair<Int, Int>> = emptyList()
    private var pendingLayoutNeeded: Boolean = false

    var kernelBridge: EditorKernelBridge?
        get() = pipeline.kernelBridge
        set(value) { pipeline.kernelBridge = value }

    private var themeBackgroundColor: Int = Color.WHITE
    private var lastAppliedThemeColors: com.xiwei.sujian.ui.compose.theme.EditorThemeColors? = null

    init {
        inputAdapter.setHostView(this)
        inputAdapter.onPipelineOutput = { output: PipelineOutput -> handlePipelineOutput(output) }
        inputAdapter.onCompositionVisualUpdate = {
            updateMaxScroll()
            scrollY = scrollY.coerceIn(0f, maxScrollY)
            invalidate()
        }
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
                    onContentChanged?.invoke(pipeline.getText())
                }
                invalidate()
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
            updateLayoutConfig()
            onContentChanged?.invoke(pipeline.getText())
        }
    }

    fun onCompositionUpdated() {
        pipeline.onCompositionUpdated()
        updateMaxScroll()
        scrollY = scrollY.coerceIn(0f, maxScrollY)
        invalidate()
    }

    fun applyCompositionUpdate(visualIntent: com.xiwei.sujian.editor.v2.mirror.VisualIntent, mirrorUpdate: (() -> Unit)? = null) {
        pipeline.applyCompositionUpdate(visualIntent, mirrorUpdate)
        updateMaxScroll()
        scrollY = scrollY.coerceIn(0f, maxScrollY)
        invalidate()
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
        if (lineTop < scrollY) {
            scrollY = lineTop
        } else if (lineBottom > scrollY + viewHeight) {
            scrollY = lineBottom - viewHeight
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
        maxScrollY = pipeline.getLayoutMaxScrollY(height)
    }

    private fun updateLayoutConfig() {
        pipeline.updateLayout(width.toFloat())
        updateMaxScroll()
        scrollY = scrollY.coerceIn(0f, maxScrollY)
        invalidate()
    }

    /**
     * Main rendering loop: draw one frame and request the next if animation is still active.
     *
     * The invalidate() at the end triggers a new onDraw on the next vsync, creating a
     * self-sustaining frame loop as long as [hasActiveAnimation] is true. When the
     * animation completes (checked in [AndroidEditorPipeline.drawFrame] via
     * [AndroidTextAnimationEngine.completeIfFinished]), the loop stops naturally.
     */
    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        canvas.save()
        canvas.translate(-scrollX, -scrollY)
        val searchHighlightsUtf16 = searchHighlights.map { (startUtf8, endUtf8) ->
            Pair(pipeline.utf8ToUtf16(startUtf8), pipeline.utf8ToUtf16(endUtf8))
        }
        pipeline.drawFrame(canvas, searchHighlightsUtf16, width, height, scrollX, scrollY)
        canvas.restore()
        if (pipeline.hasActiveAnimation()) {
            invalidate()
        }
    }

    override fun onCreateInputConnection(outAttrs: android.view.inputmethod.EditorInfo?): InputConnection? {
        return inputAdapter.onCreateInputConnection(outAttrs)
    }

    override fun onCheckIsTextEditor(): Boolean = isSessionBound

    override fun onInitializeAccessibilityNodeInfo(info: android.view.accessibility.AccessibilityNodeInfo?) {
        super.onInitializeAccessibilityNodeInfo(info)
        info?.isEditable = true
        info?.text = getDisplayText()
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        if (!isSessionBound) return false
        when (event.action) {
            MotionEvent.ACTION_DOWN -> {
                touchDownX = event.x + scrollX
                touchDownY = event.y + scrollY
                isDragging = false
                requestFocus()
                return true
            }
            MotionEvent.ACTION_MOVE -> {
                val dx = event.x + scrollX - touchDownX
                val dy = event.y + scrollY - touchDownY
                if (!isDragging && (Math.abs(dx) > 10 || Math.abs(dy) > 10)) {
                    isDragging = true
                }
                if (isDragging && currentProfile.verticalScroll) {
                    scrollY = (scrollY - dy).coerceIn(0f, maxScrollY)
                    touchDownX = event.x + scrollX
                    touchDownY = event.y + scrollY
                    invalidate()
                }
                return true
            }
            MotionEvent.ACTION_UP -> {
                if (!isDragging) {
                    handleTap(event.x + scrollX, event.y + scrollY)
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
     * Cancel active animation when the window loses focus.
     *
     * Animation Bitmaps hold references to the View's Canvas; continuing to render after
     * window focus loss can produce stale frames or leak hardware resources. Cancelling
     * here is safe because the user cannot observe the animation while the window is not
     * focused — the next focus gain will render the final static state from the mirror.
     */
    override fun onWindowFocusChanged(hasWindowFocus: Boolean) {
        super.onWindowFocusChanged(hasWindowFocus)
        if (!hasWindowFocus) {
            pipeline.cancelActiveTransaction()
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

    fun setText(text: String) {
        loadText(text, 0)
    }

    fun setTypingAnimationEnabled(enabled: Boolean, durationMs: Long) {
        pipeline.kernelBridge?.setAnimationEnabled(enabled)
        pipeline.kernelBridge?.setAnimationDurationMs(durationMs)
        if (!enabled) {
            pipeline.cancelActiveTransaction()
        }
        pipeline.setAnimationPolicy(
            if (enabled) com.xiwei.sujian.editor.v2.visual.TextAnimationPolicy.ENABLED
            else com.xiwei.sujian.editor.v2.visual.TextAnimationPolicy.SYSTEM_SUPPRESSED
        )
    }

    private var smoothCursorEnabled: Boolean = true
    private var smoothCursorDurationMs: Long = 160

    fun setSmoothCursorEnabled(enabled: Boolean, durationMs: Long) {
        smoothCursorEnabled = enabled
        smoothCursorDurationMs = durationMs
    }

    fun isSmoothCursorEnabled(): Boolean = smoothCursorEnabled

    fun setAutoIndent(enabled: Boolean, widthSp: Float) {
        pipeline.setAutoIndent(enabled, widthSp)
    }

    fun isAutoIndentEnabled(): Boolean = pipeline.isAutoIndentEnabled()
    fun getAutoIndentWidthSp(): Float = pipeline.getAutoIndentWidthSp()

    private var coordinatedAnimationEnabled: Boolean = true

    fun setCoordinatedAnimationEnabled(enabled: Boolean) {
        coordinatedAnimationEnabled = enabled
    }

    fun isCoordinatedAnimationEnabled(): Boolean = coordinatedAnimationEnabled

    fun applyThemeColorsFromAdapter(colors: com.xiwei.sujian.ui.compose.theme.EditorThemeColors) {
        lastAppliedThemeColors = colors
        themeBackgroundColor = colors.background
        textPaint.color = colors.text
        pipeline.setRendererThemeColors(
            textColor = colors.text,
            cursorColor = colors.cursor,
            selectionColor = colors.selection,
            preeditColor = colors.composing,
            bgColor = colors.background
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
            .setInsertionMarkerLocation(x, lineTop.toFloat(), lineBottom.toFloat(), lineBottom.toFloat(), android.view.inputmethod.CursorAnchorInfo.FLAG_HAS_VISIBLE_REGION)
            .build()
        imm.updateCursorAnchorInfo(this, info)
    }

    // ── #541: Session lifecycle for shared host ──

    private var isSessionBound: Boolean = false
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
        if (isSessionBound) {
            onContentChanged = null
            onCommitRequested = null
            onCancelRequested = null
            unbindSession("rebind")
        }
        kernelBridge = sessionBridge
        currentProfile = profile
        isSessionBound = true
        applyProfileToPipeline(profile, initialText, initialCursorUtf8)
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
        if (profile.animationPolicy == com.xiwei.sujian.editor.v2.coordinator.AnimationPolicy.SYSTEM_SUPPRESSED) {
            pipeline.kernelBridge?.setAnimationEnabled(false)
            pipeline.setAnimationPolicy(com.xiwei.sujian.editor.v2.visual.TextAnimationPolicy.SYSTEM_SUPPRESSED)
        } else {
            pipeline.kernelBridge?.setAnimationEnabled(true)
            pipeline.setAnimationPolicy(com.xiwei.sujian.editor.v2.visual.TextAnimationPolicy.INHERIT_GLOBAL)
        }
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
     * Cancels any active animation and invalidates the composition session, but does NOT
     * close the Rust EditorKernel session or reset the mirror. The Undo/Redo stack and
     * revision history survive across commits — the persistent session remains bound and
     * can continue editing. This contrasts with [unbindSession] (used for draft sessions),
     * which detaches the bridge and closes the Rust session entirely.
     *
     * Animation cancellation is necessary because the composition overlay is being removed
     * — any in-progress composition animation would reference stale preedit state.
     */
    fun softResetForPersistentCommit() {
        pipeline.cancelActiveTransaction()
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
