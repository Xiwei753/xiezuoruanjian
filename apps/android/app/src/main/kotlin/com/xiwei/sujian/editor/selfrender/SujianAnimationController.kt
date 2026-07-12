package com.xiwei.sujian.editor.selfrender

import android.graphics.RectF
import com.xiwei.sujian.model.AnimationModeData
import com.xiwei.sujian.model.EditorAnimationKindData
import com.xiwei.sujian.model.EditorVisualTransactionData
import com.xiwei.sujian.model.SujianCursorRectData
import com.xiwei.sujian.model.SujianEditCauseData
import com.xiwei.sujian.model.SujianVisualEditContext
import com.xiwei.sujian.diagnostics.DiagnosticsLogger

class SujianAnimationController(
    private val buffer: SujianEditorBuffer,
    private val layout: SujianEditorLayout,
    private val renderer: SujianEditorRenderer,
    private val cursorController: SujianCursorController
) {
    private val TAG = "SujianAnimCtrl"

    enum class TextAnimationStartResult { Started, Skipped }

    var animationEnabled: Boolean = false
    var animationDurationMs: Long = 160L
    var coordinatedAnimationEnabled: Boolean = false

    private lateinit var snapshotBuilder: AndroidLayoutSnapshotBuilder

    fun setSnapshotBuilder(builder: AndroidLayoutSnapshotBuilder) {
        snapshotBuilder = builder
    }

    data class DeleteSnapshot(
        val deletedText: String,
        val oldLineSnapshots: List<AndroidLineSnapshot>,
        val oldCursorRect: SujianCursorRect,
        val animationId: ULong
    )
    private val deleteSnapshots = mutableListOf<DeleteSnapshot>()
    private var lastDeleteSnapshotId: ULong = 0u

    fun recordDeleteSnapshot(
        deletedText: String,
        oldLineSnapshots: List<AndroidLineSnapshot>,
        oldCursorRect: SujianCursorRect
    ): ULong {
        val id = nextAnimationId()
        deleteSnapshots.add(DeleteSnapshot(deletedText, oldLineSnapshots, oldCursorRect, id))
        lastDeleteSnapshotId = id
        return id
    }

    fun consumeDeleteSnapshot(id: ULong): DeleteSnapshot? {
        val idx = deleteSnapshots.indexOfFirst { it.animationId == id }
        if (idx < 0) return null
        return deleteSnapshots.removeAt(idx)
    }

    fun handleVisualEdit(context: SujianVisualEditContext, view: SujianEditorView) {
        if (!animationEnabled) return
        if (!shouldAnimateForCause(context.cause)) return

        val vt = fetchVisualTransaction(context, view)
        if (vt == null) {
            DiagnosticsLogger.d(TAG, "No visual transaction from Core for cause=${context.cause}")
            return
        }

        vt.oldCursorRect = context.oldCursorRect
        vt.newCursorRect = context.newCursorRect
        vt.reflowGlyphRects = context.reflowGlyphRects

        val textAnimationResult = when (vt.kind) {
            EditorAnimationKindData.Insert -> handleInsertTransaction(vt)
            EditorAnimationKindData.Delete -> handleDeleteTransaction(vt)
            EditorAnimationKindData.Cursor -> TextAnimationStartResult.Skipped
        }

        if (textAnimationResult == TextAnimationStartResult.Started &&
            coordinatedAnimationEnabled && vt.oldCursorRect != null && vt.newCursorRect != null
        ) {
            val newRect = vt.newCursorRect!!
            cursorController.updateCursorTarget(
                newRect.x.toFloat(),
                newRect.top.toFloat(),
                newRect.bottom.toFloat(),
                true
            )
        }
    }

    fun handleInsertTransaction(vt: EditorVisualTransactionData): TextAnimationStartResult {
        if (!animationEnabled) return TextAnimationStartResult.Skipped

        if (renderer.isScrolling()) {
            renderer.clearAnimations()
            return TextAnimationStartResult.Skipped
        }

        val text = buffer.text
        val decision = vt.animationMode

        if (decision == AnimationModeData.SystemSuppressed || decision == AnimationModeData.SnapshotAnimation) {
            renderer.clearAnimations()
            return TextAnimationStartResult.Skipped
        }

        val rangeStartUtf16 = buffer.utf8ToUtf16(vt.insertedRangeStart)
        val rangeEndUtf16 = buffer.utf8ToUtf16(vt.insertedRangeEnd)

        if (rangeStartUtf16 >= rangeEndUtf16) {
            return TextAnimationStartResult.Skipped
        }

        val newRevision = snapshotBuilder.nextRevisionAndIncrement()
        val staticLayout = layout.getLayout(text)

        val insertLine = staticLayout.getLineForOffset(rangeStartUtf16.coerceIn(0, text.length))
        val endLine = staticLayout.getLineForOffset(rangeEndUtf16.coerceIn(0, text.length))

        val affectedLineRange = insertLine..(endLine.coerceAtMost(insertLine + 10))

        val newLineSnapshots = snapshotBuilder.buildLineSnapshots(
            text, affectedLineRange, newRevision, renderer.getTextColor()
        )

        if (newLineSnapshots.isEmpty()) {
            return TextAnimationStartResult.Skipped
        }

        val slices = mutableListOf<AndroidAnimatedSlice>()
        val staticPatches = mutableListOf<AndroidStaticLinePatch>()

        val oldCursorRect = vt.oldCursorRect
        val fromX = oldCursorRect?.x?.toFloat() ?: layout.getCursorRect(text, rangeStartUtf16).x
        val fromTop = oldCursorRect?.top?.toFloat() ?: layout.getCursorRect(text, rangeStartUtf16).top
        val fromBaselineY = oldCursorRect?.baselineY?.toFloat() ?: layout.getCursorRect(text, rangeStartUtf16).baselineY

        for (lineSnapshot in newLineSnapshots) {
            val insertedClusters = lineSnapshot.clusters.filter { cluster ->
                val clusterUtf16Start = cluster.platformTextStart
                val clusterUtf16End = cluster.platformTextEnd
                clusterUtf16Start >= rangeStartUtf16 && clusterUtf16End <= rangeEndUtf16
            }

            for (cluster in insertedClusters) {
                val fromRect = RectF(fromX, fromTop, fromX, fromTop + (cluster.visualRectInDocument.height()))
                slices.add(AndroidAnimatedSlice.insertFadeIn(
                    id = (vt.id shl 2) + lineSnapshot.visualLineOrdinal.toULong(),
                    snapshotId = lineSnapshot.id,
                    sourceRect = cluster.sourceRectInLineSnapshot,
                    fromRect = fromRect,
                    toRect = cluster.visualRectInDocument,
                    byteStart = cluster.documentByteStart,
                    byteEnd = cluster.documentByteEnd,
                    shapingIdentity = cluster.shapingIdentity
                ))
            }

            val reflowClusters = lineSnapshot.clusters.filter { cluster ->
                val clusterUtf16End = cluster.platformTextEnd
                clusterUtf16End > rangeEndUtf16 || cluster.platformTextStart < rangeStartUtf16
            }

            for (cluster in reflowClusters) {
                val oldX = if (cluster.platformTextStart >= rangeStartUtf16) {
                    cluster.visualRectInDocument.left - (rangeEndUtf16 - rangeStartUtf16) * textPaintMeasureChar()
                } else {
                    cluster.visualRectInDocument.left
                }
                val oldRect = RectF(oldX, cluster.visualRectInDocument.top, oldX + cluster.visualRectInDocument.width(), cluster.visualRectInDocument.bottom)
                slices.add(AndroidAnimatedSlice.reflowMove(
                    id = (vt.id shl 2) or 1u + lineSnapshot.visualLineOrdinal.toULong(),
                    snapshotId = lineSnapshot.id,
                    sourceRect = cluster.sourceRectInLineSnapshot,
                    fromRect = oldRect,
                    toRect = cluster.visualRectInDocument,
                    byteStart = cluster.documentByteStart,
                    byteEnd = cluster.documentByteEnd,
                    shapingIdentity = cluster.shapingIdentity
                ))
            }

            val animatedByteRanges = slices.map { Pair(it.documentByteStart, it.documentByteEnd) }
            val visibleSourceRects = mutableListOf<RectF>()
            for (cluster in lineSnapshot.clusters) {
                val isAnimated = animatedByteRanges.any { (start, end) ->
                    !(cluster.documentByteEnd <= start || cluster.documentByteStart >= end)
                }
                if (!isAnimated) {
                    visibleSourceRects.add(cluster.sourceRectInLineSnapshot)
                }
            }

            staticPatches.add(AndroidStaticLinePatch(
                newSnapshotId = lineSnapshot.id,
                destinationDocumentRect = lineSnapshot.documentRect,
                visibleSourceRects = visibleSourceRects
            ))
        }

        val cursorTransition = if (vt.newCursorRect != null && vt.oldCursorRect != null) {
            val newCR = vt.newCursorRect!!
            val oldCR = vt.oldCursorRect!!
            AndroidCursorTransition.tween(
                RectF(oldCR.x.toFloat(), oldCR.top.toFloat(), oldCR.x.toFloat(), oldCR.bottom.toFloat()),
                RectF(newCR.x.toFloat(), newCR.top.toFloat(), newCR.x.toFloat(), newCR.bottom.toFloat()),
                vt.durationMs
            )
        } else {
            AndroidCursorTransition.snap(RectF(0f, 0f, 0f, 0f))
        }

        val tx = AndroidPlatformVisualTransaction(
            key = vt.id,
            state = AndroidVisualTransactionState.Pending,
            operationKind = AndroidVisualOperationKind.Insert,
            animationMode = decision,
            durationMs = vt.durationMs,
            oldRevision = newRevision - 1,
            newRevision = newRevision,
            slices = slices,
            oldLineSnapshots = mutableListOf(),
            newLineSnapshots = newLineSnapshots.toMutableList(),
            staticLinePatches = staticPatches.toMutableList(),
            cursorTransition = cursorTransition
        )

        if (!renderer.addTransaction(tx)) {
            return TextAnimationStartResult.Skipped
        }

        return TextAnimationStartResult.Started
    }

    fun handleDeleteTransaction(vt: EditorVisualTransactionData): TextAnimationStartResult {
        if (!animationEnabled) return TextAnimationStartResult.Skipped

        val decision = if (renderer.isScrolling()) AnimationModeData.SystemSuppressed else vt.animationMode
        if (decision == AnimationModeData.SystemSuppressed || decision == AnimationModeData.SnapshotAnimation) {
            renderer.clearAnimations()
            consumeDeleteSnapshot(lastDeleteSnapshotId)
            return TextAnimationStartResult.Skipped
        }

        val snapshot = consumeDeleteSnapshot(lastDeleteSnapshotId)
        if (snapshot == null) {
            val fallbackSnapshot = deleteSnapshots.firstOrNull()
            if (fallbackSnapshot != null) {
                deleteSnapshots.remove(fallbackSnapshot)
                return buildDeleteTransaction(vt, fallbackSnapshot.oldLineSnapshots, fallbackSnapshot.oldCursorRect)
            } else {
                DiagnosticsLogger.d(TAG, "No delete snapshot for transaction ${vt.id}, skipping")
                return TextAnimationStartResult.Skipped
            }
        }
        return buildDeleteTransaction(vt, snapshot.oldLineSnapshots, snapshot.oldCursorRect)
    }

    private fun buildDeleteTransaction(
        vt: EditorVisualTransactionData,
        oldSnapshots: List<AndroidLineSnapshot>,
        oldCursorRect: SujianCursorRect
    ): TextAnimationStartResult {
        if (oldSnapshots.isEmpty()) {
            return TextAnimationStartResult.Skipped
        }

        val newRevision = snapshotBuilder.nextRevisionAndIncrement()
        val text = buffer.text

        val newLineSnapshots = if (text.isNotEmpty()) {
            val staticLayout = layout.getLayout(text)
            val affectedLineIndices = mutableSetOf<Int>()
            for (oldSnap in oldSnapshots) {
                affectedLineIndices.add(oldSnap.visualLineOrdinal)
                if (oldSnap.visualLineOrdinal + 1 < staticLayout.lineCount) {
                    affectedLineIndices.add(oldSnap.visualLineOrdinal + 1)
                }
            }
            val minLine = affectedLineIndices.minOrNull() ?: 0
            val maxLine = affectedLineIndices.maxOrNull() ?: 0
            snapshotBuilder.buildLineSnapshots(text, minLine..maxLine, newRevision, renderer.getTextColor())
        } else {
            emptyList()
        }

        val slices = mutableListOf<AndroidAnimatedSlice>()
        val staticPatches = mutableListOf<AndroidStaticLinePatch>()

        val newCursorRect = vt.newCursorRect
        val toX = newCursorRect?.x?.toFloat() ?: oldCursorRect.x
        val toTop = newCursorRect?.top?.toFloat() ?: oldCursorRect.top
        val toBaselineY = newCursorRect?.baselineY?.toFloat() ?: oldCursorRect.baselineY

        for (oldSnapshot in oldSnapshots) {
            for (cluster in oldSnapshot.clusters) {
                val toRect = RectF(toX, toTop, toX, toTop + cluster.visualRectInDocument.height())
                slices.add(AndroidAnimatedSlice.deleteFadeOut(
                    id = (vt.id shl 2) + cluster.platformTextStart.toULong(),
                    snapshotId = oldSnapshot.id,
                    sourceRect = cluster.sourceRectInLineSnapshot,
                    fromRect = cluster.visualRectInDocument,
                    toRect = toRect,
                    byteStart = cluster.documentByteStart,
                    byteEnd = cluster.documentByteEnd,
                    shapingIdentity = cluster.shapingIdentity
                ))
            }
        }

        for (newSnapshot in newLineSnapshots) {
            val reflowClusters = newSnapshot.clusters
            for (cluster in reflowClusters) {
                val oldY = cluster.visualRectInDocument.top - (newSnapshot.documentRect.top - (oldSnapshots.firstOrNull()?.documentRect?.top ?: 0f))
                val oldRect = RectF(cluster.visualRectInDocument.left, oldY, cluster.visualRectInDocument.right, oldY + cluster.visualRectInDocument.height())
                if (kotlin.math.abs(oldRect.top - cluster.visualRectInDocument.top) > 0.5f ||
                    kotlin.math.abs(oldRect.left - cluster.visualRectInDocument.left) > 0.5f) {
                    slices.add(AndroidAnimatedSlice.reflowMove(
                        id = (vt.id shl 2) or 2u + cluster.platformTextStart.toULong(),
                        snapshotId = newSnapshot.id,
                        sourceRect = cluster.sourceRectInLineSnapshot,
                        fromRect = oldRect,
                        toRect = cluster.visualRectInDocument,
                        byteStart = cluster.documentByteStart,
                        byteEnd = cluster.documentByteEnd,
                        shapingIdentity = cluster.shapingIdentity
                    ))
                }
            }

            val animatedByteRanges = slices.map { Pair(it.documentByteStart, it.documentByteEnd) }
            val visibleSourceRects = mutableListOf<RectF>()
            for (cluster in newSnapshot.clusters) {
                val isAnimated = animatedByteRanges.any { (start, end) ->
                    !(cluster.documentByteEnd <= start || cluster.documentByteStart >= end)
                }
                if (!isAnimated) {
                    visibleSourceRects.add(cluster.sourceRectInLineSnapshot)
                }
            }
            staticPatches.add(AndroidStaticLinePatch(
                newSnapshotId = newSnapshot.id,
                destinationDocumentRect = newSnapshot.documentRect,
                visibleSourceRects = visibleSourceRects
            ))
        }

        val cursorTransition = if (vt.newCursorRect != null) {
            val newCR = vt.newCursorRect!!
            AndroidCursorTransition.tween(
                RectF(oldCursorRect.x, oldCursorRect.top, oldCursorRect.x, oldCursorRect.bottom),
                RectF(newCR.x.toFloat(), newCR.top.toFloat(), newCR.x.toFloat(), newCR.bottom.toFloat()),
                vt.durationMs
            )
        } else {
            AndroidCursorTransition.snap(RectF(oldCursorRect.x, oldCursorRect.top, oldCursorRect.x, oldCursorRect.bottom))
        }

        val tx = AndroidPlatformVisualTransaction(
            key = vt.id,
            state = AndroidVisualTransactionState.Pending,
            operationKind = AndroidVisualOperationKind.Delete,
            animationMode = vt.animationMode,
            durationMs = vt.durationMs,
            oldRevision = newRevision - 1,
            newRevision = newRevision,
            slices = slices,
            oldLineSnapshots = oldSnapshots.toMutableList(),
            newLineSnapshots = newLineSnapshots.toMutableList(),
            staticLinePatches = staticPatches.toMutableList(),
            cursorTransition = cursorTransition
        )

        if (!renderer.addTransaction(tx)) {
            return TextAnimationStartResult.Skipped
        }

        return TextAnimationStartResult.Started
    }

    private fun fetchVisualTransaction(
        context: SujianVisualEditContext,
        view: SujianEditorView
    ): EditorVisualTransactionData? {
        val provider = view.visualTransactionProvider ?: return null

        val oldText = context.oldText
        val newText = context.newText

        val oldCursorUtf8 = SujianEditorBuffer.utf16ToUtf8(oldText, context.oldSelectionHead)
        val newCursorUtf8 = SujianEditorBuffer.utf16ToUtf8(newText, context.newSelectionHead)

        val causeStr = context.cause.toCoreCauseString()

        return try {
            provider.provide(
                oldText = oldText,
                newText = newText,
                oldCursorIndex = oldCursorUtf8.toUInt(),
                newCursorIndex = newCursorUtf8.toUInt(),
                cause = causeStr,
                maxAnimatedChars = buffer.maxAnimatedChars.toUInt(),
                animationDurationMs = buffer.animationDurationMs.toULong()
            )
        } catch (e: Exception) {
            DiagnosticsLogger.d(TAG, "fetchVisualTransaction failed: ${e.message}")
            null
        }
    }

    fun setScrolling(scrolling: Boolean) {
        renderer.setScrolling(scrolling)
    }

    fun tick() {
        renderer.tickAnimations()
    }

    fun hasActiveAnimations(): Boolean = renderer.hasActiveAnimations()

    fun onDetachedFromWindow() {
        renderer.clearAnimations()
        deleteSnapshots.forEach { snap ->
            snap.oldLineSnapshots.forEach { it.release() }
        }
        deleteSnapshots.clear()
    }

    fun clearState() {
        renderer.clearAnimations()
        deleteSnapshots.forEach { snap ->
            snap.oldLineSnapshots.forEach { it.release() }
        }
        deleteSnapshots.clear()
    }

    private fun shouldAnimateForCause(cause: SujianEditCauseData): Boolean {
        return when (cause) {
            SujianEditCauseData.Typing,
            SujianEditCauseData.Delete,
            SujianEditCauseData.TypingCommit,
            SujianEditCauseData.Paste,
            SujianEditCauseData.Undo,
            SujianEditCauseData.Redo -> true
            SujianEditCauseData.Load,
            SujianEditCauseData.Format,
            SujianEditCauseData.ImeComposition,
            SujianEditCauseData.Programmatic -> false
        }
    }

    private fun textPaintMeasureChar(): Float {
        return layout.getLayout(buffer.text).let { if (it.lineCount > 0) it.getLineWidth(0) / (buffer.text.length.coerceAtLeast(1)) else 10f }
    }

    companion object {
        private var globalAnimationId: ULong = 1u

        private fun nextAnimationId(): ULong {
            val id = globalAnimationId
            globalAnimationId = globalAnimationId.inc()
            return id
        }
    }
}

private fun SujianEditCauseData.toCoreCauseString(): String = when (this) {
    SujianEditCauseData.Typing -> "Typing"
    SujianEditCauseData.Delete -> "Delete"
    SujianEditCauseData.ImeComposition -> "ImeComposition"
    SujianEditCauseData.TypingCommit -> "TypingCommit"
    SujianEditCauseData.Paste -> "Paste"
    SujianEditCauseData.Undo -> "Undo"
    SujianEditCauseData.Redo -> "Redo"
    SujianEditCauseData.Load -> "Load"
    SujianEditCauseData.Format -> "Format"
    SujianEditCauseData.Programmatic -> "Programmatic"
}
