package com.xiwei.sujian.editor.v2.pipeline

import android.graphics.Color
import android.text.TextPaint
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import com.xiwei.sujian.editor.v2.mirror.EditResult
import com.xiwei.sujian.editor.v2.mirror.VisualIntent
import com.xiwei.sujian.editor.v2.mirror.CoordinatedCursor
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine
import com.xiwei.sujian.editor.v2.visual.AndroidTextAnimationEngine
import com.xiwei.sujian.editor.v2.visual.AndroidVisualPlanner
import com.xiwei.sujian.editor.v2.visual.VisualResourceStore
import com.xiwei.sujian.editor.v2.render.ComposedFrame
import com.xiwei.sujian.editor.v2.render.EditorFrameComposer
import com.xiwei.sujian.editor.v2.render.AndroidTextRenderer
import com.xiwei.sujian.editor.v2.render.AndroidTextAnimationRenderer
import com.xiwei.sujian.editor.v2.input.AndroidInputAdapter
import com.xiwei.sujian.editor.v2.input.AndroidTextIndexMap
import com.xiwei.sujian.editor.v2.host.EditorKernelBridge
import android.view.View
import uniffi.writer_core.EditorTransactionCauseDto

class AndroidEditorPipeline private constructor(
    val mirror: DisplayTextMirror,
    val layoutEngine: AndroidLayoutEngine,
    val visualPlanner: AndroidVisualPlanner,
    val animationEngine: AndroidTextAnimationEngine,
    val textRenderer: AndroidTextRenderer,
    val animationRenderer: AndroidTextAnimationRenderer,
    val frameComposer: EditorFrameComposer,
    var inputAdapter: AndroidInputAdapter?,
    var kernelBridge: EditorKernelBridge?
) {

    companion object {
        fun create(mirror: DisplayTextMirror, textPaint: TextPaint, hostView: View): AndroidEditorPipeline {
            val layoutEngine = AndroidLayoutEngine(mirror, textPaint)
            val visualPlanner = AndroidVisualPlanner()
            val resourceStore = VisualResourceStore()
            val animationEngine = AndroidTextAnimationEngine(visualPlanner, resourceStore)
            val textRenderer = AndroidTextRenderer()
            val animationRenderer = AndroidTextAnimationRenderer()
            val frameComposer = EditorFrameComposer()
            val pipeline = AndroidEditorPipeline(mirror, layoutEngine, visualPlanner, animationEngine, textRenderer, animationRenderer, frameComposer, null, null)
            val inputAdapter = AndroidInputAdapter(mirror, pipeline)
            inputAdapter.setHostView(hostView)
            pipeline.inputAdapter = inputAdapter
            return pipeline
        }
    }

    private var autoIndentEnabled: Boolean = false
    private var autoIndentWidthSp: Float = 2f
    private var maxLength: Int = 0

    fun loadText(text: String, cursorUtf8: Int): LoadTextResult {
        val bridge = kernelBridge ?: return LoadTextResult.Failed
        inputAdapter?.invalidateCompositionSession()
        val dto = bridge.loadText(text, cursorUtf8) ?: return LoadTextResult.Failed
        val result = EditResult.fromDto(dto)
        if (result.isStale()) {
            return LoadTextResult.Failed
        }
        if (result.isApplied() || result.isNoChange()) {
            mirror.loadFromSnapshot(text, result.newSelectionEnd, result.newRevision, result.newSelectionStart, result.newSelectionEnd)
            resetAfterLoad()
            return LoadTextResult.Loaded(result)
        }
        return LoadTextResult.Failed
    }

    fun insertText(byteOffset: Int, text: String, cause: EditorTransactionCauseDto = EditorTransactionCauseDto.TYPING): PipelineOutput {
        val bridge = kernelBridge ?: return PipelineOutput.StaleOrInvalid
        inputAdapter?.invalidateCompositionSession()
        if (autoIndentEnabled && text == "\n") {
            val indentPrefix = computeAutoIndentPrefix()
            val dto = bridge.insertLineBreak(byteOffset, indentPrefix, cause, mirror.getRevision()) ?: return PipelineOutput.StaleOrInvalid
            val result = EditResult.fromDto(dto)
            return applyEditResult(result)
        }
        val dto = bridge.insert(byteOffset, text, cause, mirror.getRevision()) ?: return PipelineOutput.StaleOrInvalid
        val result = EditResult.fromDto(dto)
        return applyEditResult(result)
    }

    fun deleteRange(byteStart: Int, byteEndExclusive: Int, cause: EditorTransactionCauseDto = EditorTransactionCauseDto.DELETE): PipelineOutput {
        val bridge = kernelBridge ?: return PipelineOutput.StaleOrInvalid
        inputAdapter?.invalidateCompositionSession()
        val dto = bridge.delete(byteStart, byteEndExclusive, cause, mirror.getRevision()) ?: return PipelineOutput.StaleOrInvalid
        val result = EditResult.fromDto(dto)
        return applyEditResult(result)
    }

    fun replaceRangeTyped(byteStart: Int, byteEndExclusive: Int, replacementText: String, originalText: String, cause: EditorTransactionCauseDto = EditorTransactionCauseDto.TYPING, beforePatch: (() -> Unit)? = null): PipelineOutput {
        val bridge = kernelBridge ?: return PipelineOutput.StaleOrInvalid
        inputAdapter?.invalidateCompositionSession()
        val dto = bridge.replace(byteStart, byteEndExclusive, replacementText, originalText, cause, mirror.getRevision()) ?: return PipelineOutput.StaleOrInvalid
        val result = EditResult.fromDto(dto)
        return applyEditResult(result, beforePatch)
    }

    fun setSelectionTyped(anchorByteOffset: Int, headByteOffset: Int): PipelineOutput {
        val bridge = kernelBridge ?: return PipelineOutput.StaleOrInvalid
        val dto = bridge.setSelection(anchorByteOffset, headByteOffset, mirror.getRevision()) ?: return PipelineOutput.StaleOrInvalid
        val result = EditResult.fromDto(dto)
        return applyEditResult(result)
    }

    fun performUndo(): PipelineOutput {
        val bridge = kernelBridge ?: return PipelineOutput.StaleOrInvalid
        inputAdapter?.invalidateCompositionSession()
        val dto = bridge.undo(mirror.getRevision()) ?: return PipelineOutput.StaleOrInvalid
        val result = EditResult.fromDto(dto)
        return applyEditResult(result)
    }

    fun performRedo(): PipelineOutput {
        val bridge = kernelBridge ?: return PipelineOutput.StaleOrInvalid
        inputAdapter?.invalidateCompositionSession()
        val dto = bridge.redo(mirror.getRevision()) ?: return PipelineOutput.StaleOrInvalid
        val result = EditResult.fromDto(dto)
        return applyEditResult(result)
    }

    fun replaceAll(searchStr: String, replaceStr: String): PipelineOutput {
        val bridge = kernelBridge ?: return PipelineOutput.StaleOrInvalid
        inputAdapter?.invalidateCompositionSession()
        val dto = bridge.replaceAll(searchStr, replaceStr, mirror.getRevision()) ?: return PipelineOutput.StaleOrInvalid
        val result = EditResult.fromDto(dto)
        return applyEditResult(result)
    }

    /**
     * Apply a composition commit with platform-side VisualIntent override.
     *
     * The Rust kernel returns a VisualIntent tailored for the raw edit, but the platform
     * must adjust it for two reasons:
     *
     * 1. **Visual-same suppression**: when the committed text is identical to the old
     *    preedit (e.g. IME confirms the same candidate), the visual text has not changed,
     *    so all text animation is suppressed (SYSTEM_SUPPRESSED) — only the cursor animates.
     *    This avoids a spurious fade-out + fade-in of unchanged text.
     *
     * 2. **Animation mode re-evaluation**: when Rust returns SYSTEM_SUPPRESSED but the
     *    platform detects actual byte-level changes, the animation mode is re-selected
     *    based on byte count (glyph/cluster/run) so the commit still animates properly.
     *    The operationKind is overridden to COMPOSITION_COMMIT to route into the replace
     *    animation path (which supports old→new matching via fingerprint).
     *
     * All byte ranges use half-open intervals [start, end).
     */
    fun applyCompositionCommit(
        dto: uniffi.writer_core.EditorEditResultDto,
        preeditText: String = ""
    ): PipelineOutput {
        val result = EditResult.fromDto(dto)
        val rustOldAffected = result.visualIntent.oldAffectedByteRanges
        val rustNewAffected = result.visualIntent.newAffectedByteRanges

        val committedText = result.displayPatches.firstOrNull()?.insertedText ?: ""
        val replaceStart = rustOldAffected.firstOrNull()?.first
            ?: rustNewAffected.firstOrNull()?.first
            ?: 0

        val preeditByteLen = preeditText.toByteArray(Charsets.UTF_8).size
        val committedByteLen = committedText.toByteArray(Charsets.UTF_8).size

        val oldAffected = if (preeditByteLen > 0) {
            listOf(Pair(replaceStart, replaceStart + preeditByteLen))
        } else {
            rustOldAffected
        }
        val newAffected = if (committedByteLen > 0) {
            listOf(Pair(replaceStart, replaceStart + committedByteLen))
        } else {
            rustNewAffected
        }

        val isVisualSame = preeditText.isNotEmpty() && preeditText == committedText
        if (isVisualSame) {
            val suppressedIntent = VisualIntent(
                cause = result.visualIntent.cause,
                operationKind = result.visualIntent.operationKind,
                oldAffectedByteRanges = oldAffected,
                newAffectedByteRanges = newAffected,
                animationMode = uniffi.writer_core.AnimationModeDto.SYSTEM_SUPPRESSED,
                durationMs = 0L,
                coordinatedCursor = CoordinatedCursor(
                    result.visualIntent.coordinatedCursor.oldByteOffset,
                    result.visualIntent.coordinatedCursor.newByteOffset,
                    false
                )
            )
            return applyEditResultWithIntent(result, suppressedIntent)
        }
        if (result.visualIntent.animationMode == uniffi.writer_core.AnimationModeDto.SYSTEM_SUPPRESSED &&
            (oldAffected.isNotEmpty() || newAffected.isNotEmpty())) {
            val byteCount = maxOf(
                newAffected.sumOf { it.second - it.first },
                oldAffected.sumOf { it.second - it.first }
            )
            val animationMode = when {
                byteCount == 0 -> uniffi.writer_core.AnimationModeDto.SYSTEM_SUPPRESSED
                byteCount <= 24 -> uniffi.writer_core.AnimationModeDto.GLYPH_ANIMATION
                byteCount <= 96 -> uniffi.writer_core.AnimationModeDto.CLUSTER_ANIMATION
                else -> uniffi.writer_core.AnimationModeDto.RUN_ANIMATION
            }
            if (animationMode != uniffi.writer_core.AnimationModeDto.SYSTEM_SUPPRESSED) {
                val animatedIntent = VisualIntent(
                    cause = result.visualIntent.cause,
                    operationKind = uniffi.writer_core.EditorOperationKindDto.COMPOSITION_COMMIT,
                    oldAffectedByteRanges = oldAffected,
                    newAffectedByteRanges = newAffected,
                    animationMode = animationMode,
                    durationMs = 200L,
                    coordinatedCursor = CoordinatedCursor(0, 0, true)
                )
                return applyEditResultWithIntent(result, animatedIntent)
            }
        }
        return applyEditResultWithIntent(result, VisualIntent(
            cause = result.visualIntent.cause,
            operationKind = uniffi.writer_core.EditorOperationKindDto.COMPOSITION_COMMIT,
            oldAffectedByteRanges = oldAffected,
            newAffectedByteRanges = newAffected,
            animationMode = result.visualIntent.animationMode,
            durationMs = result.visualIntent.durationMs,
            coordinatedCursor = result.visualIntent.coordinatedCursor
        ))
    }

    fun applyEditResult(
        result: EditResult,
        beforePatch: (() -> Unit)? = null
    ): PipelineOutput {
        if (result.isStale()) {
            return PipelineOutput.StaleOrInvalid
        }
        if (result.isInvalid() && result.displayPatches.isEmpty()) {
            return PipelineOutput.StaleOrInvalid
        }
        if (result.displayPatches.isEmpty() && result.baseRevision != result.newRevision) {
            return PipelineOutput.NeedReload
        }
        if (result.displayPatches.isEmpty() && result.baseRevision != mirror.getRevision()) {
            return PipelineOutput.NeedReload
        }

        animationEngine.prepareAndSubmit(
            visualIntent = result.visualIntent,
            layoutEngine = layoutEngine,
            mirrorUpdate = { mirror.applyEditResult(result) },
            beforePatch = beforePatch
        )

        return PipelineOutput.Edited(result)
    }

    private fun applyEditResultWithIntent(
        result: EditResult,
        visualIntent: VisualIntent,
        beforePatch: (() -> Unit)? = null
    ): PipelineOutput {
        if (result.isStale()) {
            return PipelineOutput.StaleOrInvalid
        }
        if (result.isInvalid() && result.displayPatches.isEmpty()) {
            return PipelineOutput.StaleOrInvalid
        }
        if (result.displayPatches.isEmpty() && result.baseRevision != result.newRevision) {
            return PipelineOutput.NeedReload
        }
        if (result.displayPatches.isEmpty() && result.baseRevision != mirror.getRevision()) {
            return PipelineOutput.NeedReload
        }

        animationEngine.prepareAndSubmit(
            visualIntent = visualIntent,
            layoutEngine = layoutEngine,
            mirrorUpdate = { mirror.applyEditResult(result) },
            beforePatch = beforePatch
        )

        return PipelineOutput.Edited(result)
    }

    fun applyCompositionUpdate(
        visualIntent: VisualIntent,
        mirrorUpdate: (() -> Unit)? = null
    ) {
        animationEngine.prepareAndSubmit(
            visualIntent = visualIntent,
            layoutEngine = layoutEngine,
            mirrorUpdate = mirrorUpdate
        )
    }

    /**
     * Apply a composition update with platform-constructed VisualIntent.
     *
     * The platform constructs its own VisualIntent rather than using Rust's because:
     * - The platform knows the exact old/new preedit text and can detect visual-same
     *   (old preedit == new preedit) to suppress unnecessary animation.
     * - Animation mode is selected by grapheme cluster count and content characteristics
     *   (newlines → LineReflow, complex graphemes → Cluster, etc.), not by Rust's
     *   generic byte-count heuristic.
     * - oldAffected/newAffected are computed from the preedit byte ranges, ensuring
     *   only the changed preedit region animates.
     *
     * When [isVisualSame] is true, animation is suppressed (SYSTEM_SUPPRESSED) and only
     * the cursor animates — the preedit text has not visually changed.
     */
    fun applyCompositionUpdateAnimated(
        replaceStartUtf8: Int,
        replaceEndUtf8: Int,
        newPreeditText: String,
        oldPreeditText: String,
        mirrorUpdate: (() -> Unit)? = null
    ) {
        val oldPreeditByteLen = oldPreeditText.toByteArray(Charsets.UTF_8).size
        val newPreeditByteLen = newPreeditText.toByteArray(Charsets.UTF_8).size
        val isVisualSame = oldPreeditText.isNotEmpty() && oldPreeditText == newPreeditText
        val oldAffected = buildList {
            if (oldPreeditByteLen > 0 && !isVisualSame) {
                add(Pair(replaceStartUtf8, replaceStartUtf8 + oldPreeditByteLen))
            } else if (oldPreeditByteLen == 0 && replaceStartUtf8 < replaceEndUtf8) {
                add(Pair(replaceStartUtf8, replaceEndUtf8))
            }
        }
        val newAffected = if (newPreeditText.isEmpty() || isVisualSame) emptyList() else listOf(Pair(replaceStartUtf8, replaceStartUtf8 + newPreeditByteLen))
        val combinedText = oldPreeditText + newPreeditText
        val clusterCount = maxOf(
            newPreeditText.codePointCount(0, newPreeditText.length),
            oldPreeditText.codePointCount(0, oldPreeditText.length)
        )
        val containsNewline = combinedText.any { it == '\n' || it == '\r' }
        val containsComplexGrapheme = combinedText.any { char ->
            val type = Character.getType(char.code)
            type == Character.SURROGATE.toInt() ||
                type == Character.NON_SPACING_MARK.toInt() ||
                type == Character.COMBINING_SPACING_MARK.toInt() ||
                Character.isHighSurrogate(char) || Character.isLowSurrogate(char)
        }
        val animationMode = when {
            isVisualSame || clusterCount == 0 -> uniffi.writer_core.AnimationModeDto.SYSTEM_SUPPRESSED
            containsNewline -> uniffi.writer_core.AnimationModeDto.LINE_REFLOW_ANIMATION
            containsComplexGrapheme -> uniffi.writer_core.AnimationModeDto.CLUSTER_ANIMATION
            clusterCount <= 8 -> uniffi.writer_core.AnimationModeDto.GLYPH_ANIMATION
            else -> uniffi.writer_core.AnimationModeDto.RUN_ANIMATION
        }
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.IME_COMPOSITION,
            operationKind = uniffi.writer_core.EditorOperationKindDto.COMPOSITION_UPDATE,
            oldAffectedByteRanges = oldAffected,
            newAffectedByteRanges = newAffected,
            animationMode = animationMode,
            durationMs = 160L,
            coordinatedCursor = CoordinatedCursor(0, 0, true)
        )
        animationEngine.prepareAndSubmit(
            visualIntent = visualIntent,
            layoutEngine = layoutEngine,
            mirrorUpdate = mirrorUpdate
        )
    }

    fun applyCompositionCancelAnimated(
        replaceStartUtf8: Int,
        replaceEndUtf8: Int,
        oldPreeditText: String,
        mirrorUpdate: (() -> Unit)? = null
    ) {
        val preeditByteLen = oldPreeditText.toByteArray(Charsets.UTF_8).size
        val oldAffected = if (preeditByteLen == 0 && replaceStartUtf8 == replaceEndUtf8) emptyList()
            else if (preeditByteLen > 0) listOf(Pair(replaceStartUtf8, replaceStartUtf8 + preeditByteLen))
            else listOf(Pair(replaceStartUtf8, replaceEndUtf8))
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.IME_COMPOSITION,
            operationKind = uniffi.writer_core.EditorOperationKindDto.COMPOSITION_CANCEL,
            oldAffectedByteRanges = oldAffected,
            newAffectedByteRanges = emptyList(),
            animationMode = uniffi.writer_core.AnimationModeDto.CLUSTER_ANIMATION,
            durationMs = 160L,
            coordinatedCursor = CoordinatedCursor(0, 0, true)
        )
        animationEngine.prepareAndSubmit(
            visualIntent = visualIntent,
            layoutEngine = layoutEngine,
            mirrorUpdate = mirrorUpdate
        )
    }

    fun onCompositionUpdated() {
        layoutEngine.requestLayout()
    }

    fun drawFrame(canvas: android.graphics.Canvas, searchHighlightsUtf16: List<Pair<Int, Int>>, viewportWidth: Int, viewportHeight: Int, scrollX: Float, scrollY: Float) {
        val frameTimeMs = System.nanoTime() / 1_000_000
        val layout = layoutEngine.getLayout()
        if (layout != null) {
            val rev = layoutEngine.getCurrentRevision()
            val effectiveSelStart = if (selectionAllowed) (rev?.selectionStartUtf16 ?: mirror.getSelectionStartUtf16()) else mirror.getCursorUtf16()
            val effectiveSelEnd = if (selectionAllowed) (rev?.selectionEndUtf16 ?: mirror.getSelectionEndUtf16()) else mirror.getCursorUtf16()

            val transaction = animationEngine.getActiveTransaction()
            val progress = animationEngine.getTimelineProgress(frameTimeMs)

            animationEngine.markFirstVisibleFrame(frameTimeMs)

            val composedFrame = frameComposer.compose(
                layout = layout,
                transaction = transaction,
                progress = progress,
                cursorUtf16 = if (cursorVisible) (rev?.cursorUtf16 ?: mirror.getCursorUtf16()) else -1,
                cursorX = rev?.cursorX ?: 0f,
                cursorY = rev?.cursorY ?: 0f,
                cursorHeight = rev?.cursorHeight ?: 0f,
                selectionStartUtf16 = effectiveSelStart,
                selectionEndUtf16 = effectiveSelEnd,
                compositionStartUtf16 = rev?.compositionStartUtf16 ?: -1,
                compositionEndUtf16 = rev?.compositionEndUtf16 ?: -1,
                searchHighlightsUtf16 = searchHighlightsUtf16,
                viewportWidth = viewportWidth,
                viewportHeight = viewportHeight,
                scrollX = scrollX,
                scrollY = scrollY
            )

            renderComposedFrame(canvas, composedFrame)

            animationEngine.completeIfFinished(frameTimeMs)
        }
    }

    private fun renderComposedFrame(canvas: android.graphics.Canvas, frame: ComposedFrame) {
        val layout = frame.layout ?: return
        val transaction = frame.transaction

        textRenderer.drawBackground(canvas)

        if (transaction != null && transaction.animatedSlices.isNotEmpty()) {
            textRenderer.drawSearchHighlights(canvas, layout, frame.searchHighlightsUtf16)
            textRenderer.drawSelectionHighlight(canvas, layout, frame.selectionStartUtf16, frame.selectionEndUtf16)
            val animatedRegions = animationRenderer.computeAnimatedSliceRegions(transaction)
            textRenderer.drawStaticTextWithHoles(canvas, layout, animatedRegions)
            animationRenderer.drawAnimatedSlices(canvas, transaction, frame.progress)
            textRenderer.drawPreeditUnderline(canvas, layout, frame.compositionStartUtf16, frame.compositionEndUtf16)

            val ct = transaction.cursorTransition
            if (ct != null && ct.shouldAnimate) {
                animationRenderer.drawAnimatedCursor(canvas, transaction, frame.progress, textRenderer.getCursorPaint())
            } else {
                textRenderer.drawCursor(canvas, frame.cursorUtf16, frame.cursorX, frame.cursorY, frame.cursorHeight)
            }
        } else {
            textRenderer.drawSearchHighlights(canvas, layout, frame.searchHighlightsUtf16)
            textRenderer.drawSelectionHighlight(canvas, layout, frame.selectionStartUtf16, frame.selectionEndUtf16)
            textRenderer.drawStaticText(canvas, layout)
            textRenderer.drawPreeditUnderline(canvas, layout, frame.compositionStartUtf16, frame.compositionEndUtf16)
            textRenderer.drawCursor(canvas, frame.cursorUtf16, frame.cursorX, frame.cursorY, frame.cursorHeight)
        }
    }

    fun cancelActiveTransaction() {
        animationEngine.cancel()
    }

    fun releaseAllResources() {
        animationEngine.release()
    }

    fun hasActiveAnimation(): Boolean = animationEngine.hasActiveAnimation()

    fun updateLayout(width: Float) {
        layoutEngine.setWidth(width)
        layoutEngine.requestLayout()
    }

    fun resetAfterLoad() {
        animationEngine.cancel()
    }

    fun reloadFromKernel(): Boolean {
        val bridge = kernelBridge ?: return false
        val snapshot = bridge.sessionSnapshot() ?: return false
        val cursorUtf8 = snapshot.cursor.toInt()
        val selAnchorUtf8 = snapshot.selectionAnchor.toInt()
        val selHeadUtf8 = cursorUtf8
        mirror.loadFromSnapshot(
            snapshot.text,
            cursorUtf8,
            snapshot.revision.toLong(),
            selAnchorUtf8,
            selHeadUtf8
        )
        cancelActiveTransaction()
        releaseAllResources()
        return true
    }

    fun getText(): String = mirror.getText()
    fun getRevision(): Long = mirror.getRevision()
    fun getCursorUtf8(): Int = mirror.getCursorUtf8()
    fun getCursorUtf16(): Int = mirror.getCursorUtf16()
    fun getSelectionStartUtf8(): Int = mirror.getSelectionStartUtf8()
    fun getSelectionEndUtf8(): Int = mirror.getSelectionEndUtf8()
    fun getSelectionStartUtf16(): Int = mirror.getSelectionStartUtf16()
    fun getSelectionEndUtf16(): Int = mirror.getSelectionEndUtf16()
    fun getLengthUtf16(): Int = mirror.getLengthUtf16()
    fun getCommittedCursorUtf8(): Int = mirror.getCommittedCursorUtf8()
    fun getCommittedSelectionStartUtf8(): Int = mirror.getCommittedSelectionStartUtf8()
    fun getCommittedSelectionEndUtf8(): Int = mirror.getCommittedSelectionEndUtf8()
    fun getCommittedText(): String = mirror.getCommittedText()

    fun setAutoIndent(enabled: Boolean, widthSp: Float) {
        autoIndentEnabled = enabled
        autoIndentWidthSp = widthSp
    }

    fun isAutoIndentEnabled(): Boolean = autoIndentEnabled
    fun getAutoIndentWidthSp(): Float = autoIndentWidthSp

    private fun computeAutoIndentPrefix(): String {
        if (!autoIndentEnabled) return ""
        val indexMap = AndroidTextIndexMap(mirror)
        val cursorUtf8 = mirror.getCursorUtf8()
        val cursorUtf16 = indexMap.utf8ToUtf16(cursorUtf8)
        val text = mirror.getText()
        val safeCursorUtf16 = cursorUtf16.coerceIn(0, text.length)
        val lineStartUtf16 = if (safeCursorUtf16 > 0) text.lastIndexOf('\n', safeCursorUtf16 - 1) + 1 else 0
        val linePrefix = text.substring(lineStartUtf16, safeCursorUtf16)
        val indent = linePrefix.takeWhile { it == ' ' || it == '\t' }
        return indent
    }

    fun previousGraphemeByteLen(offset: Int): Int {
        val indexMap = AndroidTextIndexMap(mirror)
        val utf16Offset = indexMap.utf8ToUtf16(offset)
        if (utf16Offset <= 0) return 0
        val iter = android.icu.text.BreakIterator.getCharacterInstance()
        iter.setText(mirror.getText())
        val prev = iter.preceding(utf16Offset)
        if (prev == android.icu.text.BreakIterator.DONE) return 0
        val prevUtf8 = indexMap.utf16ToUtf8(prev)
        return offset - prevUtf8
    }

    fun nextGraphemeByteLen(offset: Int): Int {
        val indexMap = AndroidTextIndexMap(mirror)
        val utf16Offset = indexMap.utf8ToUtf16(offset)
        if (utf16Offset >= mirror.getLengthUtf16()) return 0
        val iter = android.icu.text.BreakIterator.getCharacterInstance()
        iter.setText(mirror.getText())
        val next = iter.following(utf16Offset)
        if (next == android.icu.text.BreakIterator.DONE) return 0
        val nextUtf8 = indexMap.utf16ToUtf8(next)
        return nextUtf8 - offset
    }

    fun getLayoutMaxScrollY(viewHeight: Int): Float {
        val layout = layoutEngine.getLayout() ?: return 0f
        return (layout.height - viewHeight).coerceAtLeast(0).toFloat()
    }

    fun getLayoutLineForVertical(y: Int): Int {
        val layout = layoutEngine.getLayout() ?: return 0
        return layout.getLineForVertical(y)
    }

    fun getLayoutOffsetForHorizontal(line: Int, x: Float): Int {
        val layout = layoutEngine.getLayout() ?: return 0
        return layout.getOffsetForHorizontal(line, x)
    }

    fun getLayoutLineTop(line: Int): Int {
        val layout = layoutEngine.getLayout() ?: return 0
        return layout.getLineTop(line)
    }

    fun getLayoutLineBottom(line: Int): Int {
        val layout = layoutEngine.getLayout() ?: return 0
        return layout.getLineBottom(line)
    }

    fun getLayoutLineForOffset(offsetUtf16: Int): Int {
        val layout = layoutEngine.getLayout() ?: return 0
        return layout.getLineForOffset(offsetUtf16)
    }

    fun getLayoutPrimaryHorizontal(offsetUtf16: Int): Float {
        val layout = layoutEngine.getLayout() ?: return 0f
        return layout.getPrimaryHorizontal(offsetUtf16)
    }

    fun setLineSpacingMultiplier(multiplier: Float) {
        layoutEngine.setLineSpacingMultiplier(multiplier)
    }

    fun setThemeColors(textColor: Int, cursorColor: Int, selectionColor: Int, preeditColor: Int, bgColor: Int = Color.WHITE) {
        textRenderer.setThemeColors(textColor, cursorColor, selectionColor, preeditColor, bgColor)
    }

    fun utf16ToUtf8(offsetUtf16: Int): Int {
        val indexMap = AndroidTextIndexMap(mirror)
        return indexMap.utf16ToUtf8(offsetUtf16)
    }

    fun utf8ToUtf16(offsetUtf8: Int): Int {
        val indexMap = AndroidTextIndexMap(mirror)
        return indexMap.utf8ToUtf16(offsetUtf8)
    }

    fun getSpannable(): android.text.SpannableStringBuilder = mirror.getSpannable()

    fun onCreateInputConnection(outAttrs: android.view.inputmethod.EditorInfo?): android.view.inputmethod.InputConnection? {
        return inputAdapter?.onCreateInputConnection(outAttrs)
    }

    fun getInputAdapterHostView(): View? = inputAdapter?.getHostView()

    sealed class PipelineOutput {
        data class Edited(val result: EditResult) : PipelineOutput()
        object NeedReload : PipelineOutput()
        object StaleOrInvalid : PipelineOutput()
    }

    sealed class LoadTextResult {
        data class Loaded(val result: EditResult) : LoadTextResult()
        object Failed : LoadTextResult()
    }

    /**
     * Reset the pipeline for reuse by a different editing target (session rebind).
     *
     * Clears target-specific transient state (active animation, mirror content, layout)
     * but preserves the pipeline infrastructure (LayoutEngine, VisualPlanner, Renderer,
     * ResourceStore, InputAdapter) so the shared host can be rebound without recreating
     * the full pipeline. Per #541, this corresponds to the resetForReuse lifecycle step
     * when the AnimatedTextEditorCoordinator switches between EditableTextTargets.
     */
    fun resetForReuse() {
        animationEngine.cancel()
        mirror.loadFromSnapshot("", 0, 0, 0, 0)
        layoutEngine.requestLayout()
    }

    fun invalidateCompositionSession() {
        inputAdapter?.invalidateCompositionSession()
    }

    private var cursorVisible: Boolean = true
    private var selectionAllowed: Boolean = true

    fun setCursorVisible(visible: Boolean) {
        cursorVisible = visible
    }

    fun isCursorVisible(): Boolean = cursorVisible

    fun setSelectionAllowed(allowed: Boolean) {
        selectionAllowed = allowed
    }

    fun isSelectionAllowed(): Boolean = selectionAllowed

    fun setMaxLength(max: Int) {
        maxLength = max
    }

    fun getMaxLength(): Int = maxLength
}
