package com.xiwei.sujian.editor.v2.pipeline

import android.content.Context
import android.graphics.Color
import android.text.TextPaint
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import com.xiwei.sujian.editor.v2.mirror.EditResult
import com.xiwei.sujian.editor.v2.mirror.VisualIntent
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutRevision
import com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot
import com.xiwei.sujian.editor.v2.visual.AndroidVisualPlanner
import com.xiwei.sujian.editor.v2.visual.VisualResourceStore
import com.xiwei.sujian.editor.v2.visual.VisualTransactionCoordinator
import com.xiwei.sujian.editor.v2.visual.PreparedVisualTransaction
import com.xiwei.sujian.editor.v2.render.AndroidRenderFrame
import com.xiwei.sujian.editor.v2.render.AndroidRenderer
import com.xiwei.sujian.editor.v2.input.AndroidInputAdapter
import com.xiwei.sujian.editor.v2.input.AndroidTextIndexMap
import com.xiwei.sujian.editor.v2.host.EditorKernelBridge
import android.view.View
import uniffi.writer_core.EditorTransactionCauseDto

class AndroidEditorPipeline private constructor(
    val mirror: DisplayTextMirror,
    val layoutEngine: AndroidLayoutEngine,
    val visualPlanner: AndroidVisualPlanner,
    val resourceStore: VisualResourceStore,
    val coordinator: VisualTransactionCoordinator,
    val renderer: AndroidRenderer,
    var inputAdapter: AndroidInputAdapter?,
    var kernelBridge: EditorKernelBridge?
) {

    companion object {
        fun create(mirror: DisplayTextMirror, textPaint: TextPaint, hostView: View): AndroidEditorPipeline {
            val layoutEngine = AndroidLayoutEngine(mirror, textPaint)
            val visualPlanner = AndroidVisualPlanner()
            val resourceStore = VisualResourceStore()
            val coordinator = VisualTransactionCoordinator(resourceStore)
            val renderer = AndroidRenderer()
            val pipeline = AndroidEditorPipeline(mirror, layoutEngine, visualPlanner, resourceStore, coordinator, renderer, null, null)
            val inputAdapter = AndroidInputAdapter(mirror, pipeline)
            inputAdapter.setHostView(hostView)
            pipeline.inputAdapter = inputAdapter
            return pipeline
        }
    }

    private var autoIndentEnabled: Boolean = false
    private var autoIndentWidthSp: Float = 2f

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

    fun applyCompositionCommit(dto: uniffi.writer_core.EditorEditResultDto): PipelineOutput {
        val result = EditResult.fromDto(dto)
        return applyEditResult(result)
    }

    @Deprecated("Composition commit failure must reload from kernel, not fallback to plain Replace")
    fun clearCompositionAndReplace(byteStart: Int, byteEndExclusive: Int, replacementText: String, originalText: String, cause: EditorTransactionCauseDto): PipelineOutput {
        return replaceRangeTyped(byteStart, byteEndExclusive, replacementText, originalText, cause)
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

        val frameTimeMs = System.nanoTime() / 1_000_000
        val rebaseSnapshot = coordinator.captureCurrentFrame(frameTimeMs)

        val oldRevision = layoutEngine.captureImmutableRevision()
        val affectedOldLineIndices = visualPlanner.computeAffectedLineIndices(result.visualIntent, oldRevision, useNewRanges = false)
        val oldSnapshots = layoutEngine.captureLineBitmapSnapshotsWithClusters(affectedOldLineIndices)
        if (beforePatch != null) {
            beforePatch.invoke()
        }
        mirror.applyEditResult(result)
        layoutEngine.requestLayout()
        val newRevision = layoutEngine.getCurrentRevision()
        val affectedNewLineIndices = visualPlanner.computeAffectedLineIndices(result.visualIntent, newRevision, useNewRanges = true)
        val newSnapshots = layoutEngine.captureLineBitmapSnapshotsWithClusters(affectedNewLineIndices)
        val transaction = visualPlanner.prepare(result.visualIntent, oldRevision, newRevision, resourceStore, oldSnapshots, newSnapshots, rebaseSnapshot)
        coordinator.submitTransaction(transaction)

        return PipelineOutput.Edited(result)
    }

    fun applyCompositionUpdate(
        visualIntent: VisualIntent,
        mirrorUpdate: (() -> Unit)? = null
    ) {
        val frameTimeMs = System.nanoTime() / 1_000_000
        val rebaseSnapshot = coordinator.captureCurrentFrame(frameTimeMs)
        val oldRevision = layoutEngine.captureImmutableRevision()
        val affectedOldLineIndices = visualPlanner.computeAffectedLineIndices(visualIntent, oldRevision, useNewRanges = false)
        val oldSnapshots = layoutEngine.captureLineBitmapSnapshotsWithClusters(affectedOldLineIndices)
        mirrorUpdate?.invoke()
        layoutEngine.requestLayout()
        val newRevision = layoutEngine.getCurrentRevision()
        val affectedNewLineIndices = visualPlanner.computeAffectedLineIndices(visualIntent, newRevision, useNewRanges = true)
        val newSnapshots = layoutEngine.captureLineBitmapSnapshotsWithClusters(affectedNewLineIndices)
        val transaction = visualPlanner.prepare(visualIntent, oldRevision, newRevision, resourceStore, oldSnapshots, newSnapshots, rebaseSnapshot)
        coordinator.submitTransaction(transaction)
    }

    fun onCompositionUpdated() {
        layoutEngine.requestLayout()
    }

    fun computeFrame(
        frameTimeMs: Long,
        cursorUtf16: Int,
        cursorX: Float,
        cursorY: Float,
        cursorHeight: Float,
        selectionStartUtf16: Int,
        selectionEndUtf16: Int,
        compositionStartUtf16: Int,
        compositionEndUtf16: Int,
        searchHighlightsUtf16: List<Pair<Int, Int>>,
        viewportWidth: Int,
        viewportHeight: Int,
        scrollX: Float,
        scrollY: Float
    ): AndroidRenderFrame {
        return coordinator.computeFrame(
            frameTimeMs,
            cursorUtf16, cursorX, cursorY, cursorHeight,
            selectionStartUtf16, selectionEndUtf16,
            compositionStartUtf16, compositionEndUtf16,
            searchHighlightsUtf16,
            viewportWidth, viewportHeight,
            scrollX, scrollY
        )
    }

    fun cancelActiveTransaction() {
        coordinator.cancelActiveTransaction()
    }

    fun releaseAllResources() {
        resourceStore.releaseAll()
    }

    fun hasActiveAnimation(): Boolean = coordinator.hasActiveAnimation()

    fun updateLayout(width: Float) {
        layoutEngine.setWidth(width)
        layoutEngine.requestLayout()
    }

    fun resetAfterLoad() {
        coordinator.cancelActiveTransaction()
        resourceStore.releaseAll()
        visualPlanner.resetOldRevision()
    }

    fun drawFrame(canvas: android.graphics.Canvas, searchHighlightsUtf16: List<Pair<Int, Int>>, viewportWidth: Int, viewportHeight: Int, scrollX: Float, scrollY: Float) {
        val frameTimeMs = System.nanoTime() / 1_000_000
        val layout = layoutEngine.getLayout()
        if (layout != null) {
            val rev = layoutEngine.getCurrentRevision()
            val frame = computeFrame(
                frameTimeMs,
                cursorUtf16 = rev?.cursorUtf16 ?: mirror.getCursorUtf16(),
                cursorX = rev?.cursorX ?: 0f,
                cursorY = rev?.cursorY ?: 0f,
                cursorHeight = rev?.cursorHeight ?: 0f,
                selectionStartUtf16 = rev?.selectionStartUtf16 ?: mirror.getSelectionStartUtf16(),
                selectionEndUtf16 = rev?.selectionEndUtf16 ?: mirror.getSelectionEndUtf16(),
                compositionStartUtf16 = rev?.compositionStartUtf16 ?: -1,
                compositionEndUtf16 = rev?.compositionEndUtf16 ?: -1,
                searchHighlightsUtf16 = searchHighlightsUtf16,
                viewportWidth = viewportWidth,
                viewportHeight = viewportHeight,
                scrollX = scrollX,
                scrollY = scrollY
            )
            renderer.draw(canvas, layout, frame)
        }
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
        renderer.setThemeColors(textColor, cursorColor, selectionColor, preeditColor, bgColor)
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
}
