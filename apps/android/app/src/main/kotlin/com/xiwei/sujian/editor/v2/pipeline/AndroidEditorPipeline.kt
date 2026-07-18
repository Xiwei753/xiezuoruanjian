package com.xiwei.sujian.editor.v2.pipeline

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

class AndroidEditorPipeline private constructor(
    val mirror: DisplayTextMirror,
    val layoutEngine: AndroidLayoutEngine,
    val visualPlanner: AndroidVisualPlanner,
    val resourceStore: VisualResourceStore,
    val coordinator: VisualTransactionCoordinator
) {

    companion object {
        fun create(mirror: DisplayTextMirror, textPaint: Paint): AndroidEditorPipeline {
            val layoutEngine = AndroidLayoutEngine(mirror, textPaint)
            val visualPlanner = AndroidVisualPlanner()
            val resourceStore = VisualResourceStore()
            val coordinator = VisualTransactionCoordinator(resourceStore)
            return AndroidEditorPipeline(mirror, layoutEngine, visualPlanner, resourceStore, coordinator)
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

    sealed class PipelineOutput {
        data class Edited(val result: EditResult) : PipelineOutput()
        object NeedReload : PipelineOutput()
        object StaleOrInvalid : PipelineOutput()
    }
}
