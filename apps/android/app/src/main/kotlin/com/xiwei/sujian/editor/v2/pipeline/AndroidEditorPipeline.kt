package com.xiwei.sujian.editor.v2.pipeline

import android.content.Context
import android.graphics.Paint
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
import android.view.View

class AndroidEditorPipeline private constructor(
    val mirror: DisplayTextMirror,
    val layoutEngine: AndroidLayoutEngine,
    val visualPlanner: AndroidVisualPlanner,
    val resourceStore: VisualResourceStore,
    val coordinator: VisualTransactionCoordinator,
    val renderer: AndroidRenderer,
    val inputAdapter: AndroidInputAdapter
) {

    companion object {
        fun create(mirror: DisplayTextMirror, textPaint: Paint, hostView: View): AndroidEditorPipeline {
            val layoutEngine = AndroidLayoutEngine(mirror, textPaint)
            val visualPlanner = AndroidVisualPlanner()
            val resourceStore = VisualResourceStore()
            val coordinator = VisualTransactionCoordinator(resourceStore)
            val renderer = AndroidRenderer()
            val inputAdapter = AndroidInputAdapter(hostView.context, mirror, hostView)
            return AndroidEditorPipeline(mirror, layoutEngine, visualPlanner, resourceStore, coordinator, renderer, inputAdapter)
        }
    }

    fun applyEditResult(
        result: EditResult,
        beforePatch: (() -> Unit)? = null
    ): PipelineOutput {
        if (result.isStale() || result.isInvalid()) {
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

    sealed class PipelineOutput {
        data class Edited(val result: EditResult) : PipelineOutput()
        object NeedReload : PipelineOutput()
        object StaleOrInvalid : PipelineOutput()
    }
}
