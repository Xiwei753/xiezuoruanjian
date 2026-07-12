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

        val textAnimationResult = when (vt.kind) {
            EditorAnimationKindData.Insert -> handleInsertTransaction(vt)
            EditorAnimationKindData.Delete -> handleDeleteTransaction(vt)
            EditorAnimationKindData.Cursor -> TextAnimationStartResult.Skipped
        }

        if (textAnimationResult == TextAnimationStartResult.Started) {
            val activeTx = renderer.getActiveTransactions().lastOrNull()
            if (activeTx != null && coordinatedAnimationEnabled) {
                cursorController.setTransactionDrivenCursor(activeTx.cursorTransition)
            }
            if (coordinatedAnimationEnabled && vt.newCursorRect != null) {
                val newRect = vt.newCursorRect!!
                cursorController.updateCursorTarget(
                    newRect.x.toFloat(),
                    newRect.top.toFloat(),
                    newRect.bottom.toFloat(),
                    true
                )
            }
        }
    }

    fun handleInsertTransaction(vt: EditorVisualTransactionData): TextAnimationStartResult {
        if (!animationEnabled) return TextAnimationStartResult.Skipped

        if (renderer.isScrolling()) {
            renderer.pauseAll()
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

        val offsetMap = EditOffsetMap.fromEdit(
            oldText = vt.oldText,
            newText = vt.newText,
            insertedRangeStart = vt.insertedRangeStart,
            insertedRangeEnd = vt.insertedRangeEnd,
            isDelete = false
        )

        val oldRevision = snapshotBuilder.currentRevision()
        val oldText = vt.oldText
        val staticLayout = layout.getLayout(text)
        val insertLine = staticLayout.getLineForOffset(rangeStartUtf16.coerceIn(0, text.length))
        val endLine = staticLayout.getLineForOffset(rangeEndUtf16.coerceIn(0, text.length))

        val affectedLineRange = computeStableSuffixRange(
            insertLine, endLine, staticLayout, offsetMap, vt.oldText
        )

        val oldLineSnapshots = if (oldText.isNotEmpty()) {
            val oldLayout = layout.getLayout(oldText)
            val oldAffectedRange = computeOldAffectedRange(
                affectedLineRange, staticLayout, oldLayout, offsetMap, oldText
            )
            snapshotBuilder.buildLineSnapshots(oldText, oldAffectedRange, oldRevision, renderer.getTextColor())
        } else {
            emptyList()
        }

        val newRevision = snapshotBuilder.nextRevisionAndIncrement()

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
                clusterUtf16Start < rangeEndUtf16 && clusterUtf16End > rangeStartUtf16
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
                val oldCluster = findOldCluster(cluster, oldLineSnapshots, offsetMap)

                if (oldCluster != null) {
                    val oldRect = RectF(
                        oldCluster.visualRectInDocument.left, oldCluster.visualRectInDocument.top,
                        oldCluster.visualRectInDocument.right, oldCluster.visualRectInDocument.bottom
                    )
                    val shapingChanged = oldCluster.shapingIdentity != cluster.shapingIdentity

                    if (shapingChanged) {
                        val oldLineSnap = oldLineSnapshots.find { it.clusters.contains(oldCluster) }
                        slices.add(AndroidAnimatedSlice.crossfade(
                            id = (vt.id shl 2) or 3u + cluster.platformTextStart.toULong(),
                            role = AndroidAnimatedSliceRole.CrossfadeOld,
                            snapshotId = oldLineSnap!!.id,
                            sourceRect = oldCluster.sourceRectInLineSnapshot,
                            fromRect = oldRect,
                            toRect = cluster.visualRectInDocument,
                            byteStart = cluster.documentByteStart,
                            byteEnd = cluster.documentByteEnd,
                            shapingIdentity = oldCluster.shapingIdentity
                        ))
                        slices.add(AndroidAnimatedSlice.crossfade(
                            id = (vt.id shl 2) or 4u + cluster.platformTextStart.toULong(),
                            role = AndroidAnimatedSliceRole.CrossfadeNew,
                            snapshotId = lineSnapshot.id,
                            sourceRect = cluster.sourceRectInLineSnapshot,
                            fromRect = oldRect,
                            toRect = cluster.visualRectInDocument,
                            byteStart = cluster.documentByteStart,
                            byteEnd = cluster.documentByteEnd,
                            shapingIdentity = cluster.shapingIdentity
                        ))
                    } else {
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
                } else {
                    val isInserted = offsetMap.isNewRangeInserted(
                        cluster.documentByteStart, cluster.documentByteEnd
                    )
                    if (isInserted) {
                        val fromRect = RectF(fromX, fromTop, fromX, fromTop + cluster.visualRectInDocument.height())
                        slices.add(AndroidAnimatedSlice.insertFadeIn(
                            id = (vt.id shl 2) or 7u + cluster.platformTextStart.toULong(),
                            snapshotId = lineSnapshot.id,
                            sourceRect = cluster.sourceRectInLineSnapshot,
                            fromRect = fromRect,
                            toRect = cluster.visualRectInDocument,
                            byteStart = cluster.documentByteStart,
                            byteEnd = cluster.documentByteEnd,
                            shapingIdentity = cluster.shapingIdentity
                        ))
                    } else {
                        slices.add(AndroidAnimatedSlice.crossfade(
                            id = (vt.id shl 2) or 8u + cluster.platformTextStart.toULong(),
                            role = AndroidAnimatedSliceRole.CrossfadeNew,
                            snapshotId = lineSnapshot.id,
                            sourceRect = cluster.sourceRectInLineSnapshot,
                            fromRect = RectF(
                                cluster.visualRectInDocument.left,
                                cluster.visualRectInDocument.top,
                                cluster.visualRectInDocument.right,
                                cluster.visualRectInDocument.bottom
                            ),
                            toRect = cluster.visualRectInDocument,
                            byteStart = cluster.documentByteStart,
                            byteEnd = cluster.documentByteEnd,
                            shapingIdentity = cluster.shapingIdentity
                        ))
                    }
                }
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

        if (slices.isEmpty()) {
            return TextAnimationStartResult.Skipped
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
            oldRevision = oldRevision,
            newRevision = newRevision,
            slices = slices,
            oldLineSnapshots = oldLineSnapshots.toMutableList(),
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

        if (renderer.isScrolling()) {
            renderer.pauseAll()
        }

        val decision = vt.animationMode
        if (decision == AnimationModeData.SystemSuppressed || decision == AnimationModeData.SnapshotAnimation) {
            if (!renderer.isScrolling()) {
                renderer.clearAnimations()
            }
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

        val offsetMap = EditOffsetMap.fromEdit(
            oldText = vt.oldText,
            newText = vt.newText,
            insertedRangeStart = 0,
            insertedRangeEnd = 0,
            isDelete = true,
            deletedRangeStart = vt.insertedRangeStart,
            deletedRangeEnd = vt.insertedRangeEnd
        )

        val newLineSnapshots = if (text.isNotEmpty()) {
            val staticLayout = layout.getLayout(text)
            val affectedLineIndices = computeDeleteAffectedLines(oldSnapshots, staticLayout, offsetMap)
            if (affectedLineIndices.isEmpty()) {
                emptyList()
            } else {
                val minLine = affectedLineIndices.minOrNull() ?: 0
                val maxLine = affectedLineIndices.maxOrNull() ?: 0
                val stableMaxLine = expandToStableSuffix(minLine, staticLayout, offsetMap, vt.oldText)
                snapshotBuilder.buildLineSnapshots(text, minLine..stableMaxLine, newRevision, renderer.getTextColor())
            }
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
                val oldCluster = findOldCluster(cluster, oldSnapshots, offsetMap)

                if (oldCluster == null) {
                    continue
                }

                val oldRect = RectF(
                    oldCluster.visualRectInDocument.left, oldCluster.visualRectInDocument.top,
                    oldCluster.visualRectInDocument.right, oldCluster.visualRectInDocument.bottom
                )

                val positionChanged = kotlin.math.abs(oldRect.top - cluster.visualRectInDocument.top) > 0.5f ||
                    kotlin.math.abs(oldRect.left - cluster.visualRectInDocument.left) > 0.5f
                if (!positionChanged) continue

                val shapingChanged = oldCluster.shapingIdentity != cluster.shapingIdentity

                if (shapingChanged) {
                    val oldLineSnap = oldSnapshots.find { it.clusters.contains(oldCluster) }
                    slices.add(AndroidAnimatedSlice.crossfade(
                        id = (vt.id shl 2) or 5u + cluster.platformTextStart.toULong(),
                        role = AndroidAnimatedSliceRole.CrossfadeOld,
                        snapshotId = oldLineSnap!!.id,
                        sourceRect = oldCluster.sourceRectInLineSnapshot,
                        fromRect = oldRect,
                        toRect = cluster.visualRectInDocument,
                        byteStart = cluster.documentByteStart,
                        byteEnd = cluster.documentByteEnd,
                        shapingIdentity = oldCluster.shapingIdentity
                    ))
                    slices.add(AndroidAnimatedSlice.crossfade(
                        id = (vt.id shl 2) or 6u + cluster.platformTextStart.toULong(),
                        role = AndroidAnimatedSliceRole.CrossfadeNew,
                        snapshotId = newSnapshot.id,
                        sourceRect = cluster.sourceRectInLineSnapshot,
                        fromRect = oldRect,
                        toRect = cluster.visualRectInDocument,
                        byteStart = cluster.documentByteStart,
                        byteEnd = cluster.documentByteEnd,
                        shapingIdentity = cluster.shapingIdentity
                    ))
                } else {
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

        if (slices.isEmpty()) {
            return TextAnimationStartResult.Skipped
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

    /**
     * 使用 OffsetMap 在旧快照中查找匹配的 cluster。
     *
     * 映射逻辑：
     * 1. 将新 cluster 的 documentByteStart/End 通过 OffsetMap 映射到旧 byte range。
     * 2. 在所有旧行快照中按映射后的 byte range 查找 cluster。
     * 3. 不再依赖 visualLineOrdinal 匹配。
     */
    private fun findOldCluster(
        newCluster: AndroidClusterSnapshot,
        oldLineSnapshots: List<AndroidLineSnapshot>,
        offsetMap: EditOffsetMap
    ): AndroidClusterSnapshot? {
        val oldRange = offsetMap.mapNewRangeToOld(newCluster.documentByteStart, newCluster.documentByteEnd)
            ?: return null

        for (oldLine in oldLineSnapshots) {
            val match = oldLine.clusters.find { oc ->
                oc.documentByteStart == oldRange.start && oc.documentByteEnd == oldRange.end
            }
            if (match != null) return match
        }

        for (oldLine in oldLineSnapshots) {
            val match = oldLine.clusters.find { oc ->
                oc.documentByteStart <= oldRange.start && oc.documentByteEnd >= oldRange.end &&
                    (oc.documentByteEnd - oc.documentByteStart) == (oldRange.end - oldRange.start)
            }
            if (match != null) return match
        }

        return null
    }

    /**
     * 计算受影响行范围，扩展到稳定后缀。
     *
     * 不再使用固定 insertLine + 10 限制。从编辑点向后比较 old/new cluster 映射，
     * 直到出现稳定后缀（连续若干 cluster 的映射 byte range 一致、
     * shaping identity 一致、visual line 后续断行边界一致）。
     * 若一直不稳定，处理到受影响段落结束。
     */
    private fun computeStableSuffixRange(
        insertLine: Int,
        endLine: Int,
        staticLayout: android.text.Layout,
        offsetMap: EditOffsetMap,
        oldText: String
    ): IntRange {
        val startLine = insertLine
        val lastLine = staticLayout.lineCount - 1

        var candidateEnd = endLine
        val stableConsecutiveNeeded = 2
        var stableConsecutive = 0

        while (candidateEnd < lastLine && stableConsecutive < stableConsecutiveNeeded) {
            val isStable = isLineStableSuffix(candidateEnd, staticLayout, offsetMap, oldText)
            if (isStable) {
                stableConsecutive++
            } else {
                stableConsecutive = 0
            }
            candidateEnd++
        }

        if (stableConsecutive >= stableConsecutiveNeeded) {
            candidateEnd = (candidateEnd - stableConsecutiveNeeded).coerceAtLeast(endLine)
        } else {
            candidateEnd = lastLine
        }

        return startLine..candidateEnd.coerceAtMost(lastLine)
    }

    private fun isLineStableSuffix(
        lineIdx: Int,
        staticLayout: android.text.Layout,
        offsetMap: EditOffsetMap,
        oldText: String
    ): Boolean {
        if (lineIdx < 0 || lineIdx >= staticLayout.lineCount) return true
        if (oldText.isEmpty()) return false

        val lineStart = staticLayout.getLineStart(lineIdx)
        val lineEnd = staticLayout.getLineEnd(lineIdx)
        if (lineStart >= lineEnd) return true

        val byteStart = SujianEditorBuffer.utf16ToUtf8(staticLayout.text.toString(), lineStart)
        val byteEnd = SujianEditorBuffer.utf16ToUtf8(staticLayout.text.toString(), lineEnd.coerceAtMost(staticLayout.text.length))

        val oldRange = offsetMap.mapNewRangeToOld(byteStart, byteEnd)
        if (oldRange == null) return false

        val oldByteEnd = SujianEditorBuffer.utf16ToUtf8(oldText, oldText.length)
        if (oldRange.end > oldByteEnd) return false

        return true
    }

    /**
     * 计算旧文本的受影响行范围，用于构建旧快照。
     */
    private fun computeOldAffectedRange(
        newAffectedRange: IntRange,
        newLayout: android.text.Layout,
        oldLayout: android.text.Layout,
        offsetMap: EditOffsetMap,
        oldText: String
    ): IntRange {
        val newStartByte = SujianEditorBuffer.utf16ToUtf8(
            newLayout.text.toString(),
            newLayout.getLineStart(newAffectedRange.first)
        )
        val newEndByte = SujianEditorBuffer.utf16ToUtf8(
            newLayout.text.toString(),
            newLayout.getLineEnd(newAffectedRange.last).coerceAtMost(newLayout.text.length)
        )

        val oldStartRange = offsetMap.mapNewRangeToOld(newStartByte, newStartByte + 1)
        val oldEndRange = offsetMap.mapNewRangeToOld(newEndByte - 1, newEndByte)

        val oldStartLine = if (oldStartRange != null) {
            val utf16 = SujianEditorBuffer.utf8ToUtf16(oldText, oldStartRange.start)
            oldLayout.getLineForOffset(utf16.coerceIn(0, oldText.length))
        } else {
            0
        }

        val oldEndLine = if (oldEndRange != null) {
            val utf16 = SujianEditorBuffer.utf8ToUtf16(oldText, oldEndRange.end)
            oldLayout.getLineForOffset(utf16.coerceIn(0, oldText.length))
        } else {
            oldLayout.lineCount - 1
        }

        return oldStartLine..oldEndLine.coerceAtMost(oldLayout.lineCount - 1)
    }

    /**
     * 计算删除事务的受影响行范围。
     */
    private fun computeDeleteAffectedLines(
        oldSnapshots: List<AndroidLineSnapshot>,
        staticLayout: android.text.Layout,
        offsetMap: EditOffsetMap
    ): Set<Int> {
        val affectedLineIndices = mutableSetOf<Int>()
        for (oldSnap in oldSnapshots) {
            val newRange = offsetMap.mapOldRangeToNew(oldSnap.documentByteStart, oldSnap.documentByteEnd)
            if (newRange != null) {
                val utf16Start = SujianEditorBuffer.utf8ToUtf16(buffer.text, newRange.start)
                val utf16End = SujianEditorBuffer.utf8ToUtf16(buffer.text, newRange.end)
                val startLine = staticLayout.getLineForOffset(utf16Start.coerceIn(0, buffer.text.length))
                val endLine = staticLayout.getLineForOffset(utf16End.coerceIn(0, buffer.text.length))
                for (l in startLine..endLine) {
                    affectedLineIndices.add(l)
                }
            } else {
                if (oldSnap.visualLineOrdinal < staticLayout.lineCount) {
                    affectedLineIndices.add(oldSnap.visualLineOrdinal)
                }
            }
        }
        return affectedLineIndices
    }

    /**
     * 扩展到稳定后缀（删除场景）。
     */
    private fun expandToStableSuffix(
        startLine: Int,
        staticLayout: android.text.Layout,
        offsetMap: EditOffsetMap,
        oldText: String
    ): Int {
        val lastLine = staticLayout.lineCount - 1
        var candidateEnd = startLine
        val stableConsecutiveNeeded = 2
        var stableConsecutive = 0

        while (candidateEnd < lastLine && stableConsecutive < stableConsecutiveNeeded) {
            val isStable = isLineStableSuffix(candidateEnd, staticLayout, offsetMap, oldText)
            if (isStable) {
                stableConsecutive++
            } else {
                stableConsecutive = 0
            }
            candidateEnd++
        }

        if (stableConsecutive >= stableConsecutiveNeeded) {
            return (candidateEnd - stableConsecutiveNeeded + 1).coerceAtLeast(startLine)
        }
        return lastLine
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
