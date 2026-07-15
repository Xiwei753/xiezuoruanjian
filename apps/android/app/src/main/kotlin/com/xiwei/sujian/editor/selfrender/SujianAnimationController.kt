package com.xiwei.sujian.editor.selfrender

import android.graphics.RectF
import com.xiwei.sujian.model.AnimationModeData
import com.xiwei.sujian.model.EditorAnimationKindData
import com.xiwei.sujian.model.EditorVisualTransactionData
import com.xiwei.sujian.model.SujianCursorRectData
import com.xiwei.sujian.model.SujianEditCauseData
import com.xiwei.sujian.model.SujianVisualEditContext
import com.xiwei.sujian.diagnostics.DiagnosticsLogger

/**
 * Android 文字动画控制器 — 消费 Core 的视觉事务，驱动平台动画。
 *
 * 职责：
 * - 从 Core 获取 [EditorVisualTransactionData]，判断 Insert、Delete、Move、Crossfade。
 * - 捕获 old/new Android 行快照（通过 [AndroidLayoutSnapshotBuilder]）。
 * - 使用 [EditOffsetMap] 将新 cluster 的 UTF-8 byte range 映射到旧 cluster。
 * - 构建 [AndroidAnimatedSlice] 和 [AndroidStaticLinePatch]。
 * - 组装 [AndroidPlatformVisualTransaction] 并交给 [SujianEditorRenderer]。
 *
 * 不负责：
 * - 正文真相（buffer text 是唯一事实来源）。
 * - StaticLayout 的底层排版实现（由 [SujianEditorLayout] 完成）。
 * - 具体绘制和光标闪烁（由 [SujianEditorRenderer] 和 [SujianCursorController] 完成）。
 *
 * 跨平台坐标约定：
 * - 插入和删除都以 UTF-8 byte range 为跨平台身份。
 * - Android Layout 内部使用 UTF-16 offset，[SujianEditorBuffer.utf8ToUtf16]/[utf16ToUtf8] 做转换。
 *
 * 连续输入时 Renderer 从当前视觉帧 rebase 旧事务，不重新排版。
 * 滚动期间事务暂停或进入 pending queue，不销毁正文状态。
 *
 * 删除快照生命周期：
 * - [recordDeleteSnapshot] 在 buffer 修改前捕获旧行快照和光标位置。
 * - [consumeDeleteSnapshot] 在 [handleDeleteTransaction] 中消费，消费后从列表移除。
 * - [onDetachedFromWindow]/[clearState] 时释放所有残余快照资源。
 */
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
    ) {
        fun release() {
            oldLineSnapshots.forEach { it.release(SnapshotOwner.OwnedBySession(CompositionSessionId(it.revision))) }
        }
    }
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

    private fun releasePendingDeleteSnapshotForCurrentEdit() {
        val snapshot = consumeDeleteSnapshot(lastDeleteSnapshotId)
        snapshot?.release()
    }

    private fun mergeSnapshotsById(
        original: List<AndroidLineSnapshot>,
        supplemental: List<AndroidLineSnapshot>
    ): List<AndroidLineSnapshot> {
        val existingIds = original.map { it.id }.toSet()
        val result = original.toMutableList()
        for (snap in supplemental) {
            if (snap.id in existingIds) {
                snap.release(SnapshotOwner.OwnedBySession(CompositionSessionId(snap.revision)))
            } else {
                result.add(snap)
            }
        }
        return result
    }

    fun handleVisualEdit(context: SujianVisualEditContext, view: SujianEditorView) {
        if (!animationEnabled) {
            releasePendingDeleteSnapshotForCurrentEdit()
            return
        }
        if (!shouldAnimateForCause(context.cause)) {
            releasePendingDeleteSnapshotForCurrentEdit()
            return
        }

        val vt = fetchVisualTransaction(context, view)
        if (vt == null) {
            DiagnosticsLogger.d(TAG, "No visual transaction from Core for cause=${context.cause}")
            releasePendingDeleteSnapshotForCurrentEdit()
            return
        }

        vt.oldCursorRect = context.oldCursorRect
        vt.newCursorRect = context.newCursorRect

        val textAnimationResult = when (vt.kind) {
            EditorAnimationKindData.Insert -> {
                releasePendingDeleteSnapshotForCurrentEdit()
                handleInsertTransaction(vt)
            }
            EditorAnimationKindData.Delete -> handleDeleteTransaction(vt)
            EditorAnimationKindData.Cursor -> {
                releasePendingDeleteSnapshotForCurrentEdit()
                TextAnimationStartResult.Skipped
            }
        }

        if (textAnimationResult == TextAnimationStartResult.Started) {
            val activeTx = renderer.getActiveTransactions().lastOrNull()
            if (activeTx != null && coordinatedAnimationEnabled) {
                cursorController.setTransactionDrivenCursor(activeTx.cursorTransition)
                cursorController.setActiveTransaction(activeTx)
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

        if (!vt.hasInsertedRange) {
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

        val oldRevision = snapshotBuilder.currentCommittedRevision()
        val newRevision = snapshotBuilder.allocateNextRevision()
        val oldText = vt.oldText
        val staticLayout = layout.getLayout(text)
        val insertLine = staticLayout.getLineForOffset(rangeStartUtf16.coerceIn(0, text.length))
        val endLine = staticLayout.getLineForOffset(rangeEndUtf16.coerceIn(0, text.length))

        val oldLayout = if (oldText.isNotEmpty()) layout.getLayout(oldText) else null

        val affectedLineRange = computeStableSuffixRange(
            insertLine, endLine, staticLayout, offsetMap, vt.oldText, oldLayout
        )

        val (finalOldAffectedRange, finalNewAffectedRange) = if (oldText.isNotEmpty() && oldLayout != null) {
            expandAffectedRangesWithProbe(
                affectedLineRange, staticLayout, oldLayout, offsetMap, oldText, text, rangeStartUtf16, rangeEndUtf16
            )
        } else {
            val oldRange: HalfOpenRange? = if (oldText.isNotEmpty()) {
                computeOldAffectedRange(affectedLineRange, staticLayout, oldLayout!!, offsetMap, oldText)
            } else {
                null
            }
            Pair(oldRange, affectedLineRange)
        }

        val oldLineSnapshots: List<AndroidLineSnapshot> = finalOldAffectedRange?.let {
            if (oldText.isNotEmpty()) {
                snapshotBuilder.buildLineSnapshots(oldText, it, oldRevision, renderer.getTextColor())
            } else {
                emptyList()
            }
        } ?: emptyList()

        val newLineSnapshots = snapshotBuilder.buildLineSnapshots(
            text, finalNewAffectedRange, newRevision, renderer.getTextColor()
        )

        val oldRevisionObj: CommittedVisualRevision? = if (oldLineSnapshots.isNotEmpty()) {
            CommittedVisualRevision(
                revisionId = oldRevision,
                sessionId = CompositionSessionId(oldRevision),
                fullText = oldText,
                affectedParagraphRange = finalOldAffectedRange ?: HalfOpenRange.EMPTY,
                lineSnapshots = oldLineSnapshots,
                cursorRect = RectF(0f, 0f, 0f, 0f)
            )
        } else null

        val newRevisionObj: CommittedVisualRevision? = if (newLineSnapshots.isNotEmpty()) {
            CommittedVisualRevision(
                revisionId = newRevision,
                sessionId = CompositionSessionId(newRevision),
                fullText = text,
                affectedParagraphRange = finalNewAffectedRange,
                lineSnapshots = newLineSnapshots,
                cursorRect = RectF(0f, 0f, 0f, 0f)
            )
        } else null

        if (newLineSnapshots.isEmpty()) {
            oldRevisionObj?.release(oldRevisionObj.owner)
            snapshotBuilder.commitRevision(newRevision)
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
                val fromRect = RectF(fromX, fromTop, fromX + cluster.visualRectInDocument.width(), fromTop + cluster.visualRectInDocument.height())
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
                    val positionChanged = kotlin.math.abs(oldRect.top - cluster.visualRectInDocument.top) > 0.5f ||
                        kotlin.math.abs(oldRect.left - cluster.visualRectInDocument.left) > 0.5f
                    val shapingChanged = oldCluster.shapingIdentity != cluster.shapingIdentity

                    if (!positionChanged && !shapingChanged) continue

                    if (shapingChanged) {
                        val oldLineSnap = oldLineSnapshots.find { it.clusters.contains(oldCluster) }
                        slices.add(AndroidAnimatedSlice.crossfade(
                            id = ((vt.id shl 2) or 3u) + cluster.platformTextStart.toULong(),
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
                            id = ((vt.id shl 2) or 4u) + cluster.platformTextStart.toULong(),
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
                            id = ((vt.id shl 2) or 1u) + lineSnapshot.visualLineOrdinal.toULong(),
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
                        val fromRect = RectF(fromX, fromTop, fromX + cluster.visualRectInDocument.width(), fromTop + cluster.visualRectInDocument.height())
                        slices.add(AndroidAnimatedSlice.insertFadeIn(
                            id = ((vt.id shl 2) or 7u) + cluster.platformTextStart.toULong(),
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
                            id = ((vt.id shl 2) or 8u) + cluster.platformTextStart.toULong(),
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
            oldRevisionObj?.release(oldRevisionObj.owner)
            newRevisionObj?.release(newRevisionObj.owner)
            snapshotBuilder.commitRevision(newRevision)
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

        oldRevisionObj?.transferToTransaction(vt.id)
        newRevisionObj?.transferToTransaction(vt.id)

        val tx = AndroidPlatformVisualTransaction(
            key = vt.id,
            state = AndroidVisualTransactionState.Pending,
            operationKind = AndroidVisualOperationKind.Insert,
            animationMode = decision,
            durationMs = vt.durationMs,
            oldRevision = oldRevision,
            newRevision = newRevision,
            slices = slices,
            staticLinePatches = staticPatches.toMutableList(),
            decorationSlices = mutableListOf(),
            cursorTransition = cursorTransition,
            ownedOldRevision = oldRevisionObj,
            ownedNewRevision = newRevisionObj
        )

        if (!renderer.addTransaction(tx)) {
            tx.cancel("add_failed")
            snapshotBuilder.commitRevision(newRevision)
            return TextAnimationStartResult.Skipped
        }

        snapshotBuilder.commitRevision(newRevision)
        return TextAnimationStartResult.Started
    }

    fun handleDeleteTransaction(vt: EditorVisualTransactionData): TextAnimationStartResult {
        if (!animationEnabled) {
            releasePendingDeleteSnapshotForCurrentEdit()
            return TextAnimationStartResult.Skipped
        }

        if (renderer.isScrolling()) {
            renderer.pauseAll()
        }

        val decision = vt.animationMode
        if (decision == AnimationModeData.SystemSuppressed || decision == AnimationModeData.SnapshotAnimation) {
            if (!renderer.isScrolling()) {
                renderer.clearAnimations()
            }
            val suppressed = consumeDeleteSnapshot(lastDeleteSnapshotId)
            suppressed?.release()
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

        val oldRevision = snapshotBuilder.currentCommittedRevision()
        val newRevision = snapshotBuilder.allocateNextRevision()
        val text = buffer.text

        val offsetMap = EditOffsetMap.fromEdit(
            oldText = vt.oldText,
            newText = vt.newText,
            insertedRangeStart = if (vt.hasInsertedRange) vt.insertedRangeStart else 0,
            insertedRangeEnd = if (vt.hasInsertedRange) vt.insertedRangeEnd else 0,
            isDelete = vt.hasDeletedRange,
            deletedRangeStart = if (vt.hasDeletedRange) vt.deletedRangeStart else 0,
            deletedRangeEnd = if (vt.hasDeletedRange) vt.deletedRangeEnd else 0
        )

        val staticLayout = if (text.isNotEmpty()) layout.getLayout(text) else null
        val oldLayout = if (vt.oldText.isNotEmpty()) layout.getLayout(vt.oldText) else null

        val (finalOldSnapshots, finalNewAffectedRange) = if (staticLayout != null) {
            val affectedLineIndices = computeDeleteAffectedLines(oldSnapshots, staticLayout, offsetMap)
            if (affectedLineIndices.isEmpty()) {
                Pair(oldSnapshots, null as HalfOpenRange?)
            } else {
                val minLine = affectedLineIndices.minOrNull()!!
                val stableMaxLine = if (oldLayout != null) {
                    expandToStableSuffix(minLine, staticLayout, offsetMap, vt.oldText, oldLayout)
                } else {
                    affectedLineIndices.maxOrNull()!!
                }
                val newRange = HalfOpenRange(minLine, stableMaxLine + 1)

                val expandedOldRange = expandOldRangeWithProbe(
                    oldSnapshots, newRange, staticLayout, oldLayout, offsetMap, vt.oldText
                )
                val finalOld = if (expandedOldRange != null && vt.oldText.isNotEmpty()) {
                    val supplementalSnapshots = snapshotBuilder.buildLineSnapshots(vt.oldText, expandedOldRange, oldRevision, renderer.getTextColor())
                    mergeSnapshotsById(oldSnapshots, supplementalSnapshots)
                } else {
                    oldSnapshots
                }
                Pair(finalOld, newRange as HalfOpenRange?)
            }
        } else {
            Pair(oldSnapshots, null as HalfOpenRange?)
        }

        val newLineSnapshots: List<AndroidLineSnapshot> = finalNewAffectedRange?.let {
            if (text.isNotEmpty() && staticLayout != null) {
                snapshotBuilder.buildLineSnapshots(
                    text, it, newRevision, renderer.getTextColor()
                )
            } else {
                emptyList()
            }
        } ?: emptyList()

        val slices = mutableListOf<AndroidAnimatedSlice>()
        val staticPatches = mutableListOf<AndroidStaticLinePatch>()

        val newCursorRect = vt.newCursorRect
        val toX = newCursorRect?.x?.toFloat() ?: oldCursorRect.x
        val toTop = newCursorRect?.top?.toFloat() ?: oldCursorRect.top
        val toBaselineY = newCursorRect?.baselineY?.toFloat() ?: oldCursorRect.baselineY

        for (oldSnapshot in finalOldSnapshots) {
            for (cluster in oldSnapshot.clusters) {
                val newRange = offsetMap.mapOldRangeToNew(cluster.documentByteStart, cluster.documentByteEnd)
                if (newRange != null) {
                    val matchedInNew = newLineSnapshots.any { ns ->
                        ns.clusters.any { nc ->
                            nc.documentByteStart == newRange.start && nc.documentByteEnd == newRange.end
                        }
                    }
                    if (matchedInNew) continue
                }
                if (offsetMap.isOldRangeDeleted(cluster.documentByteStart, cluster.documentByteEnd)) {
                    val shrinkW = cluster.visualRectInDocument.width() * 0.7f
                    val shrinkH = cluster.visualRectInDocument.height() * 0.7f
                    val toRect = RectF(toX, toTop, toX + shrinkW, toTop + shrinkH)
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
        }

        for (newSnapshot in newLineSnapshots) {
            val reflowClusters = newSnapshot.clusters
            for (cluster in reflowClusters) {
                val oldCluster = findOldCluster(cluster, finalOldSnapshots, offsetMap)

                if (oldCluster == null) {
                    val isInserted = offsetMap.isNewRangeInserted(
                        cluster.documentByteStart, cluster.documentByteEnd
                    )
                    if (isInserted) {
                        val fromRect = RectF(toX, toTop, toX + cluster.visualRectInDocument.width(), toTop + cluster.visualRectInDocument.height())
                        slices.add(AndroidAnimatedSlice.insertFadeIn(
                            id = ((vt.id shl 2) or 9u) + cluster.platformTextStart.toULong(),
                            snapshotId = newSnapshot.id,
                            sourceRect = cluster.sourceRectInLineSnapshot,
                            fromRect = fromRect,
                            toRect = cluster.visualRectInDocument,
                            byteStart = cluster.documentByteStart,
                            byteEnd = cluster.documentByteEnd,
                            shapingIdentity = cluster.shapingIdentity
                        ))
                    } else {
                        slices.add(AndroidAnimatedSlice.crossfade(
                            id = ((vt.id shl 2) or 10u) + cluster.platformTextStart.toULong(),
                            role = AndroidAnimatedSliceRole.CrossfadeNew,
                            snapshotId = newSnapshot.id,
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
                    continue
                }

                val oldRect = RectF(
                    oldCluster.visualRectInDocument.left, oldCluster.visualRectInDocument.top,
                    oldCluster.visualRectInDocument.right, oldCluster.visualRectInDocument.bottom
                )

                val positionChanged = kotlin.math.abs(oldRect.top - cluster.visualRectInDocument.top) > 0.5f ||
                    kotlin.math.abs(oldRect.left - cluster.visualRectInDocument.left) > 0.5f
                val shapingChanged = oldCluster.shapingIdentity != cluster.shapingIdentity

                if (!positionChanged && !shapingChanged) continue

                if (shapingChanged) {
                    val oldLineSnap = finalOldSnapshots.find { it.clusters.contains(oldCluster) }
                    slices.add(AndroidAnimatedSlice.crossfade(
                        id = ((vt.id shl 2) or 5u) + cluster.platformTextStart.toULong(),
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
                        id = ((vt.id shl 2) or 6u) + cluster.platformTextStart.toULong(),
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
                        id = ((vt.id shl 2) or 2u) + cluster.platformTextStart.toULong(),
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

        val oldRevisionObj: CommittedVisualRevision? = if (finalOldSnapshots.isNotEmpty()) {
            val oldSessionId = (finalOldSnapshots.first().owner as? SnapshotOwner.OwnedBySession)?.sessionId
                ?: CompositionSessionId(oldRevision)
            CommittedVisualRevision(
                revisionId = oldRevision,
                sessionId = oldSessionId,
                fullText = vt.oldText,
                affectedParagraphRange = HalfOpenRange.EMPTY,
                lineSnapshots = finalOldSnapshots,
                cursorRect = RectF(oldCursorRect.x, oldCursorRect.top, oldCursorRect.x, oldCursorRect.bottom)
            )
        } else null

        val newRevisionObj: CommittedVisualRevision? = if (newLineSnapshots.isNotEmpty()) {
            CommittedVisualRevision(
                revisionId = newRevision,
                sessionId = CompositionSessionId(newRevision),
                fullText = text,
                affectedParagraphRange = finalNewAffectedRange ?: HalfOpenRange.EMPTY,
                lineSnapshots = newLineSnapshots,
                cursorRect = RectF(0f, 0f, 0f, 0f)
            )
        } else null

        if (slices.isEmpty()) {
            oldRevisionObj?.release(oldRevisionObj.owner)
            newRevisionObj?.release(newRevisionObj.owner)
            snapshotBuilder.commitRevision(newRevision)
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

        oldRevisionObj?.transferToTransaction(vt.id)
        newRevisionObj?.transferToTransaction(vt.id)

        val tx = AndroidPlatformVisualTransaction(
            key = vt.id,
            state = AndroidVisualTransactionState.Pending,
            operationKind = AndroidVisualOperationKind.Delete,
            animationMode = vt.animationMode,
            durationMs = vt.durationMs,
            oldRevision = oldRevision,
            newRevision = newRevision,
            slices = slices,
            staticLinePatches = staticPatches.toMutableList(),
            decorationSlices = mutableListOf(),
            cursorTransition = cursorTransition,
            ownedOldRevision = oldRevisionObj,
            ownedNewRevision = newRevisionObj
        )

        if (!renderer.addTransaction(tx)) {
            tx.cancel("add_failed")
            snapshotBuilder.commitRevision(newRevision)
            return TextAnimationStartResult.Skipped
        }

        snapshotBuilder.commitRevision(newRevision)
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

        return null
    }

    private fun expandAffectedRangesWithProbe(
        newAffectedRange: HalfOpenRange,
        newLayout: android.text.Layout,
        oldLayout: android.text.Layout,
        offsetMap: EditOffsetMap,
        oldText: String,
        newText: String,
        rangeStartUtf16: Int,
        rangeEndUtf16: Int
    ): Pair<HalfOpenRange?, HalfOpenRange> {
        val oldAffectedRange = computeOldAffectedRange(newAffectedRange, newLayout, oldLayout, offsetMap, oldText)
        if (oldText.isEmpty()) return Pair(null, newAffectedRange)

        val preliminaryOldProbes = snapshotBuilder.buildLineLayoutProbes(oldText, oldAffectedRange)
        val preliminaryNewProbes = snapshotBuilder.buildLineLayoutProbes(newText, newAffectedRange)

        var expandedOldStart = oldAffectedRange.start
        var expandedOldEnd = oldAffectedRange.end

        for (newProbe in preliminaryNewProbes) {
            for (cluster in newProbe.clusters) {
                val mappedOld = offsetMap.mapNewRangeToOld(cluster.documentByteStart, cluster.documentByteEnd)
                if (mappedOld == null) continue
                if (offsetMap.isNewRangeInserted(cluster.documentByteStart, cluster.documentByteEnd)) continue

                val foundInOld = preliminaryOldProbes.any { op ->
                    op.clusters.any { oc ->
                        oc.documentByteStart == mappedOld.start && oc.documentByteEnd == mappedOld.end
                    }
                }
                if (!foundInOld) {
                    val oldUtf16Start = SujianEditorBuffer.utf8ToUtf16(oldText, mappedOld.start).coerceIn(0, oldText.length)
                    val oldUtf16End = SujianEditorBuffer.utf8ToUtf16(oldText, mappedOld.end).coerceIn(0, oldText.length)
                    val oldLineStart = (oldLayout.getLineForOffset(oldUtf16Start) - 1).coerceAtLeast(0)
                    val oldLineEnd = (oldLayout.getLineForOffset(oldUtf16End) + 1).coerceAtMost(oldLayout.lineCount)
                    expandedOldStart = minOf(expandedOldStart, oldLineStart)
                    expandedOldEnd = maxOf(expandedOldEnd, oldLineEnd)
                }
            }
        }

        return Pair(HalfOpenRange(expandedOldStart, expandedOldEnd), newAffectedRange)
    }

    private fun expandOldRangeWithProbe(
        oldSnapshots: List<AndroidLineSnapshot>,
        newAffectedRange: HalfOpenRange,
        newLayout: android.text.Layout,
        oldLayout: android.text.Layout?,
        offsetMap: EditOffsetMap,
        oldText: String
    ): HalfOpenRange? {
        if (oldLayout == null || oldText.isEmpty()) return null

        val newProbes = snapshotBuilder.buildLineLayoutProbes(buffer.text, newAffectedRange)
        val oldSnapshotByteRanges = oldSnapshots.map { it.documentByteStart to it.documentByteEnd }

        var expandedStart = Int.MAX_VALUE
        var expandedEnd = Int.MIN_VALUE

        for (newProbe in newProbes) {
            for (cluster in newProbe.clusters) {
                val mappedOld = offsetMap.mapNewRangeToOld(cluster.documentByteStart, cluster.documentByteEnd)
                if (mappedOld == null) continue
                if (offsetMap.isNewRangeInserted(cluster.documentByteStart, cluster.documentByteEnd)) continue

                val foundInOldSnapshots = oldSnapshotByteRanges.any { (start, end) ->
                    mappedOld.start >= start && mappedOld.end <= end
                }
                if (!foundInOldSnapshots) {
                    val oldUtf16Start = SujianEditorBuffer.utf8ToUtf16(oldText, mappedOld.start).coerceIn(0, oldText.length)
                    val oldUtf16End = SujianEditorBuffer.utf8ToUtf16(oldText, mappedOld.end).coerceIn(0, oldText.length)
                    val oldLineStart = oldLayout.getLineForOffset(oldUtf16Start)
                    val oldLineEnd = oldLayout.getLineForOffset(oldUtf16End)
                    expandedStart = minOf(expandedStart, oldLineStart)
                    expandedEnd = maxOf(expandedEnd, oldLineEnd + 1)
                }
            }
        }

        return if (expandedStart < expandedEnd) {
            HalfOpenRange(expandedStart.coerceAtLeast(0), expandedEnd.coerceAtMost(oldLayout.lineCount))
        } else {
            null
        }
    }

    /**
     * 从编辑点向后逐行比较 old/new cluster 映射和 shaping identity，
     * 直到出现稳定后缀（连续若干行映射一致、shaping identity 一致、断行边界一致）。
     * 不会扩展到整个章节末尾：不稳定时只扩展到受影响段落及因换行合并/拆分关联的相邻段落。
     */
    private fun computeStableSuffixRange(
        insertLine: Int,
        endLine: Int,
        staticLayout: android.text.Layout,
        offsetMap: EditOffsetMap,
        oldText: String,
        oldLayout: android.text.Layout?
    ): HalfOpenRange {
        val startLine = insertLine
        val lastLine = staticLayout.lineCount - 1

        val affectedParagraphEndLine = findAffectedParagraphEndLine(staticLayout, endLine)

        val mutableOldProbes = mutableListOf<AndroidLineLayoutProbe>()
        val mutableNewProbes = mutableListOf<AndroidLineLayoutProbe>()

        if (oldText.isNotEmpty() && oldLayout != null) {
            val preliminaryOldRange = computeOldAffectedRangeFromLayouts(
                HalfOpenRange(startLine, endLine.coerceAtMost(lastLine) + 1), staticLayout, oldLayout, offsetMap, oldText
            )
            mutableOldProbes.addAll(snapshotBuilder.buildLineLayoutProbes(oldText, preliminaryOldRange))
        }
        mutableNewProbes.addAll(snapshotBuilder.buildLineLayoutProbes(buffer.text, HalfOpenRange(startLine, endLine.coerceAtMost(lastLine) + 1)))

        var candidateEnd = endLine
        val stableConsecutiveNeeded = 2
        var stableConsecutive = 0

        while (candidateEnd < lastLine && stableConsecutive < stableConsecutiveNeeded) {
            if (candidateEnd > affectedParagraphEndLine + 1) {
                break
            }

            var newProbe = mutableNewProbes.find { it.visualLineOrdinal == candidateEnd }

            if (newProbe == null && candidateEnd < staticLayout.lineCount) {
                val onTheFlyNew = snapshotBuilder.buildLineLayoutProbes(
                    buffer.text, HalfOpenRange(candidateEnd, candidateEnd + 1)
                )
                if (onTheFlyNew.isNotEmpty()) {
                    newProbe = onTheFlyNew.first()
                    mutableNewProbes.add(newProbe)
                }
            }

            var oldProbe: AndroidLineLayoutProbe? = null
            if (newProbe != null) {
                val newByteStart = newProbe.clusters.firstOrNull()?.documentByteStart
                val newByteEnd = newProbe.clusters.lastOrNull()?.documentByteEnd
                if (newByteStart != null && newByteEnd != null) {
                    val mappedOld = offsetMap.mapNewRangeToOld(newByteStart, newByteEnd)
                    if (mappedOld != null) {
                        oldProbe = mutableOldProbes.find { probe ->
                            probe.clusters.any { it.documentByteStart == mappedOld.start || it.documentByteEnd == mappedOld.end }
                        }
                    }
                }
            }

            if (oldProbe == null && oldText.isNotEmpty() && newProbe != null && oldLayout != null) {
                val newByteStart = newProbe.clusters.firstOrNull()?.documentByteStart
                val newByteEnd = newProbe.clusters.lastOrNull()?.documentByteEnd
                if (newByteStart != null && newByteEnd != null) {
                    val mappedOld = offsetMap.mapNewRangeToOld(newByteStart, newByteEnd)
                    if (mappedOld != null) {
                        val oldUtf16Start = SujianEditorBuffer.utf8ToUtf16(oldText, mappedOld.start).coerceIn(0, oldText.length)
                        val oldLineIdx = oldLayout.getLineForOffset(oldUtf16Start)
                        val onTheFlyOld = snapshotBuilder.buildLineLayoutProbes(
                            oldText, HalfOpenRange(oldLineIdx, oldLineIdx + 1)
                        )
                        if (onTheFlyOld.isNotEmpty()) {
                            oldProbe = onTheFlyOld.first()
                            mutableOldProbes.add(oldProbe)
                        }
                    }
                }
            }

            val isStable = isLineStableSuffixWithProbes(
                candidateEnd, staticLayout, offsetMap, oldText,
                oldProbe, newProbe, oldLayout
            )
            if (isStable) {
                stableConsecutive++
            } else {
                stableConsecutive = 0
            }
            candidateEnd++
        }

        if (stableConsecutive >= stableConsecutiveNeeded) {
            candidateEnd = (candidateEnd - stableConsecutiveNeeded + 1).coerceAtLeast(endLine)
        } else {
            candidateEnd = affectedParagraphEndLine.coerceAtMost(lastLine)
        }

        return HalfOpenRange(startLine, candidateEnd.coerceAtMost(lastLine) + 1)
    }

    private fun findAffectedParagraphEndLine(
        staticLayout: android.text.Layout,
        startFromLine: Int
    ): Int {
        var endLine = startFromLine
        val totalLines = staticLayout.lineCount
        val text = staticLayout.text.toString()
        while (endLine < totalLines - 1) {
            val lineEnd = staticLayout.getLineEnd(endLine)
            if (lineEnd > 0 && lineEnd <= text.length && text[lineEnd - 1] == '\n') {
                break
            }
            endLine++
        }
        return (endLine + 1).coerceAtMost(totalLines - 1)
    }

    private fun isLineStableSuffixWithProbes(
        lineIdx: Int,
        staticLayout: android.text.Layout,
        offsetMap: EditOffsetMap,
        oldText: String,
        oldProbe: AndroidLineLayoutProbe?,
        newProbe: AndroidLineLayoutProbe?,
        oldLayout: android.text.Layout?
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

        if (oldProbe != null && newProbe != null) {
            val newClusters = newProbe.clusters.filter {
                it.documentByteStart >= byteStart && it.documentByteEnd <= byteEnd
            }
            val oldClusters = oldProbe.clusters.filter {
                it.documentByteStart >= oldRange.start && it.documentByteEnd <= oldRange.end
            }

            for (newCluster in newClusters) {
                val mappedOld = offsetMap.mapNewRangeToOld(newCluster.documentByteStart, newCluster.documentByteEnd)
                if (mappedOld == null) return false
                val matchingOld = oldClusters.find {
                    it.documentByteStart == mappedOld.start && it.documentByteEnd == mappedOld.end
                }
                if (matchingOld == null) return false
                if (matchingOld.shapingIdentity != newCluster.shapingIdentity) return false
            }

            if (oldProbe.breakIdentity != newProbe.breakIdentity) return false

            val nextLineIdx = lineIdx + 1
            if (nextLineIdx < staticLayout.lineCount) {
                val nextLineStart = staticLayout.getLineStart(nextLineIdx)
                val nextByteStart = SujianEditorBuffer.utf16ToUtf8(staticLayout.text.toString(), nextLineStart)
                val nextOldRange = offsetMap.mapNewRangeToOld(nextByteStart, nextByteStart + 1)
                if (nextOldRange == null) return false
                if (oldLayout != null) {
                    val oldNextUtf16 = SujianEditorBuffer.utf8ToUtf16(oldText, nextOldRange.start).coerceIn(0, oldText.length)
                    val oldNextLine = oldLayout.getLineForOffset(oldNextUtf16)
                    val oldNextLineEnd = oldLayout.getLineEnd(oldNextLine)
                    val oldNextByteEnd = SujianEditorBuffer.utf16ToUtf8(oldText, oldNextLineEnd.coerceAtMost(oldText.length))
                    val newNextLineEnd = staticLayout.getLineEnd(nextLineIdx)
                    val newNextByteEnd = SujianEditorBuffer.utf16ToUtf8(staticLayout.text.toString(), newNextLineEnd.coerceAtMost(staticLayout.text.length))
                    val mappedNewNextEnd = offsetMap.mapNewRangeToOld(newNextByteEnd - 1, newNextByteEnd)
                    if (mappedNewNextEnd == null || mappedNewNextEnd.end != oldNextByteEnd) return false
                }
            }
        } else {
            return false
        }

        return true
    }

    private fun isLineStableSuffixWithProbeLists(
        lineIdx: Int,
        staticLayout: android.text.Layout,
        offsetMap: EditOffsetMap,
        oldText: String,
        oldProbes: List<AndroidLineLayoutProbe>,
        newProbes: List<AndroidLineLayoutProbe>,
        oldLayout: android.text.Layout?
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

        val oldLine = oldProbes.find {
            it.documentByteStart <= oldRange.start && it.documentByteEnd >= oldRange.end
        }
        if (oldLine == null) return false

        val newLine = newProbes.find {
            it.documentByteStart <= byteStart && it.documentByteEnd >= byteEnd
        }
        if (newLine == null) return false

        val newClusters = newLine.clusters.filter {
            it.documentByteStart >= byteStart && it.documentByteEnd <= byteEnd
        }
        val oldClusters = oldLine.clusters.filter {
            it.documentByteStart >= oldRange.start && it.documentByteEnd <= oldRange.end
        }

        for (newCluster in newClusters) {
            val mappedOld = offsetMap.mapNewRangeToOld(newCluster.documentByteStart, newCluster.documentByteEnd)
            if (mappedOld == null) return false
            val matchingOld = oldClusters.find {
                it.documentByteStart == mappedOld.start && it.documentByteEnd == mappedOld.end
            }
            if (matchingOld == null) return false
            if (matchingOld.shapingIdentity != newCluster.shapingIdentity) return false
        }

        if (oldLine.breakIdentity != newLine.breakIdentity) return false

        val nextLineIdx = lineIdx + 1
        if (nextLineIdx < staticLayout.lineCount) {
            val nextLineStart = staticLayout.getLineStart(nextLineIdx)
            val nextByteStart = SujianEditorBuffer.utf16ToUtf8(staticLayout.text.toString(), nextLineStart)
            val nextOldRange = offsetMap.mapNewRangeToOld(nextByteStart, nextByteStart + 1)
            if (nextOldRange == null) return false
            if (oldLayout != null) {
                val oldNextUtf16 = SujianEditorBuffer.utf8ToUtf16(oldText, nextOldRange.start).coerceIn(0, oldText.length)
                val oldNextLine = oldLayout.getLineForOffset(oldNextUtf16)
                val oldNextLineEnd = oldLayout.getLineEnd(oldNextLine)
                val oldNextByteEnd = SujianEditorBuffer.utf16ToUtf8(oldText, oldNextLineEnd.coerceAtMost(oldText.length))
                val newNextLineEnd = staticLayout.getLineEnd(nextLineIdx)
                val newNextByteEnd = SujianEditorBuffer.utf16ToUtf8(staticLayout.text.toString(), newNextLineEnd.coerceAtMost(staticLayout.text.length))
                val mappedNewNextEnd = offsetMap.mapNewRangeToOld(newNextByteEnd - 1, newNextByteEnd)
                if (mappedNewNextEnd == null || mappedNewNextEnd.end != oldNextByteEnd) return false
            }
        }

        return true
    }

    private fun computeOldAffectedRangeFromLayouts(
        newAffectedRange: HalfOpenRange,
        newLayout: android.text.Layout,
        oldLayout: android.text.Layout,
        offsetMap: EditOffsetMap,
        oldText: String
    ): HalfOpenRange {
        val newStartByte = SujianEditorBuffer.utf16ToUtf8(
            newLayout.text.toString(),
            newLayout.getLineStart(newAffectedRange.start)
        )
        val newEndByte = SujianEditorBuffer.utf16ToUtf8(
            newLayout.text.toString(),
            newLayout.getLineEnd(newAffectedRange.end - 1).coerceAtMost(newLayout.text.length)
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

        return HalfOpenRange(oldStartLine, oldEndLine.coerceAtMost(oldLayout.lineCount - 1) + 1)
    }

    /**
     * 计算旧文本的受影响行范围，用于构建旧快照。
     */
    private fun computeOldAffectedRange(
        newAffectedRange: HalfOpenRange,
        newLayout: android.text.Layout,
        oldLayout: android.text.Layout,
        offsetMap: EditOffsetMap,
        oldText: String
    ): HalfOpenRange {
        val newStartByte = SujianEditorBuffer.utf16ToUtf8(
            newLayout.text.toString(),
            newLayout.getLineStart(newAffectedRange.start)
        )
        val newEndByte = SujianEditorBuffer.utf16ToUtf8(
            newLayout.text.toString(),
            newLayout.getLineEnd(newAffectedRange.end - 1).coerceAtMost(newLayout.text.length)
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

        return HalfOpenRange(oldStartLine, oldEndLine.coerceAtMost(oldLayout.lineCount - 1) + 1)
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
                if (offsetMap.isOldRangeDeleted(oldSnap.documentByteStart, oldSnap.documentByteEnd)) {
                    continue
                }
                val adjacentNewByte = offsetMap.mapOldRangeToNew(
                    oldSnap.documentByteStart.coerceAtMost(buffer.text.length),
                    (oldSnap.documentByteStart + 1).coerceAtMost(buffer.text.length)
                )
                if (adjacentNewByte != null) {
                    val utf16Start = SujianEditorBuffer.utf8ToUtf16(buffer.text, adjacentNewByte.start)
                    val line = staticLayout.getLineForOffset(utf16Start.coerceIn(0, buffer.text.length))
                    if (line < staticLayout.lineCount) {
                        affectedLineIndices.add(line)
                    }
                }
            }
        }
        return affectedLineIndices
    }

    /**
     * 扩展到稳定后缀（删除场景）。
     * 不扩展到整个章节末尾：不稳定时只扩展到受影响段落及因换行合并/拆分关联的相邻段落。
     */
    private fun expandToStableSuffix(
        startLine: Int,
        staticLayout: android.text.Layout,
        offsetMap: EditOffsetMap,
        oldText: String,
        oldLayout: android.text.Layout
    ): Int {
        val lastLine = staticLayout.lineCount - 1
        val affectedParagraphEndLine = findAffectedParagraphEndLine(staticLayout, startLine)

        val mutableOldProbes = mutableListOf<AndroidLineLayoutProbe>()
        val mutableNewProbes = mutableListOf<AndroidLineLayoutProbe>()

        var candidateEnd = startLine
        val stableConsecutiveNeeded = 2
        var stableConsecutive = 0

        while (candidateEnd < lastLine && stableConsecutive < stableConsecutiveNeeded) {
            if (candidateEnd > affectedParagraphEndLine + 1) {
                break
            }

            val hasNewProbe = mutableNewProbes.any { probe ->
                val lineStart = staticLayout.getLineStart(candidateEnd)
                val lineEnd = staticLayout.getLineEnd(candidateEnd)
                val byteStart = SujianEditorBuffer.utf16ToUtf8(buffer.text, lineStart)
                val byteEnd = SujianEditorBuffer.utf16ToUtf8(buffer.text, lineEnd.coerceAtMost(buffer.text.length))
                probe.documentByteStart <= byteStart && probe.documentByteEnd >= byteEnd
            }

            if (!hasNewProbe && candidateEnd < staticLayout.lineCount) {
                val onTheFlyNew = snapshotBuilder.buildLineLayoutProbes(
                    buffer.text, HalfOpenRange(candidateEnd, candidateEnd + 1)
                )
                if (onTheFlyNew.isNotEmpty()) {
                    mutableNewProbes.add(onTheFlyNew.first())
                }
            }

            val hasOldProbe = mutableOldProbes.any { probe ->
                val lineStart = staticLayout.getLineStart(candidateEnd)
                val lineEnd = staticLayout.getLineEnd(candidateEnd)
                val byteStart = SujianEditorBuffer.utf16ToUtf8(buffer.text, lineStart)
                val byteEnd = SujianEditorBuffer.utf16ToUtf8(buffer.text, lineEnd.coerceAtMost(buffer.text.length))
                val oldRange = offsetMap.mapNewRangeToOld(byteStart, byteEnd)
                if (oldRange != null) {
                    probe.documentByteStart <= oldRange.start && probe.documentByteEnd >= oldRange.end
                } else false
            }

            if (!hasOldProbe && oldText.isNotEmpty()) {
                val lineStart = staticLayout.getLineStart(candidateEnd)
                val lineEnd = staticLayout.getLineEnd(candidateEnd)
                val byteStart = SujianEditorBuffer.utf16ToUtf8(buffer.text, lineStart)
                val byteEnd = SujianEditorBuffer.utf16ToUtf8(buffer.text, lineEnd.coerceAtMost(buffer.text.length))
                val oldRange = offsetMap.mapNewRangeToOld(byteStart, byteEnd)
                if (oldRange != null) {
                    val oldUtf16Start = SujianEditorBuffer.utf8ToUtf16(oldText, oldRange.start).coerceIn(0, oldText.length)
                    val oldLineIdx = oldLayout.getLineForOffset(oldUtf16Start)
                    val onTheFlyOld = snapshotBuilder.buildLineLayoutProbes(
                        oldText, HalfOpenRange(oldLineIdx, oldLineIdx + 1)
                    )
                    if (onTheFlyOld.isNotEmpty()) {
                        mutableOldProbes.add(onTheFlyOld.first())
                    }
                }
            }

            val isStable = isLineStableSuffixWithProbeLists(
                candidateEnd, staticLayout, offsetMap, oldText,
                mutableOldProbes, mutableNewProbes, oldLayout
            )
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
        return affectedParagraphEndLine.coerceAtMost(lastLine)
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

    private val compositionManager = AndroidCompositionManager()

    fun handleCompositionUpdate(
        committedText: String,
        compositionReplaceRange: Utf16Range,
        preeditText: String,
        composingCursorUtf16: Int,
        sessionId: CompositionSessionId = CompositionSessionId(0)
    ): TextAnimationStartResult {
        if (!animationEnabled || preeditText.isEmpty()) {
            return TextAnimationStartResult.Skipped
        }

        val virtualText = compositionManager.buildVirtualText(committedText, compositionReplaceRange, preeditText)

        val newRevision = snapshotBuilder.allocateNextRevision()

        val virtualLayout = if (virtualText.isNotEmpty()) layout.getLayout(virtualText) else null
        if (virtualLayout == null) {
            snapshotBuilder.commitRevision(newRevision)
            return TextAnimationStartResult.Skipped
        }

        val txKey = nextAnimationId()
        val managerGeneration = compositionManager.getGeneration()

        var prevCompositionRevision: AndroidCompositionVisualRevision? = null
        var prevRevisionFromActiveTransaction = false
        var detachedOldFromActive: OwnedVisualRevision? = null
        when (val takeResult = compositionManager.takeCurrentForTransactionTyped(txKey)) {
            is TakeCurrentResult.Success -> {
                prevCompositionRevision = takeResult.revision
            }
            is TakeCurrentResult.RevisionWithActiveTransaction -> {
                val source = renderer.takeCompositionSource(takeResult.activeTransactionKey, txKey)
                if (source != null) {
                    prevCompositionRevision = source.newRevision as? AndroidCompositionVisualRevision
                    detachedOldFromActive = source.oldRevision
                    prevRevisionFromActiveTransaction = true
                }
                compositionManager.reassignActiveTransactionKey(txKey)
            }
            is TakeCurrentResult.NoRevisionAvailable -> {
            }
        }

        if (detachedOldFromActive != null) {
            detachedOldFromActive.release(detachedOldFromActive.owner)
        }

        val affectedStartLine = if (prevCompositionRevision != null) {
            val prevVirtualLayout = if (prevCompositionRevision.virtualText.isNotEmpty()) layout.getLayout(prevCompositionRevision.virtualText) else null
            if (prevVirtualLayout != null) {
                prevVirtualLayout.getLineForOffset(prevCompositionRevision.preeditRangeInVirtualText.start.coerceIn(0, prevCompositionRevision.virtualText.length))
            } else {
                0
            }
        } else {
            val committedLayout = if (committedText.isNotEmpty()) layout.getLayout(committedText) else null
            if (committedLayout != null && compositionReplaceRange.start < committedText.length) {
                committedLayout.getLineForOffset(compositionReplaceRange.start.coerceIn(0, committedText.length))
            } else {
                0
            }
        }

        val preeditEndLine = virtualLayout.getLineForOffset(
            (compositionReplaceRange.start + preeditText.length).coerceIn(0, virtualText.length)
        )
        val affectedParagraphEndLine = findAffectedParagraphEndLine(virtualLayout, preeditEndLine)
        val snapshotEndLine = affectedParagraphEndLine.coerceAtMost(virtualLayout.lineCount - 1)

        val preliminaryEndLine = computeStableSuffixEndLine(
            prevCompositionRevision, committedText, virtualText, virtualLayout,
            compositionReplaceRange, preeditText, affectedStartLine, preeditEndLine
        )

        val oldLineSnapshots: List<AndroidLineSnapshot> = if (prevCompositionRevision != null) {
            prevCompositionRevision.lineSnapshots
        } else {
            val committedLayout = if (committedText.isNotEmpty()) layout.getLayout(committedText) else null
            val oldRevision = snapshotBuilder.currentCommittedRevision()
            if (committedLayout != null && committedText.isNotEmpty()) {
                val endLine = snapshotEndLine.coerceAtMost(committedLayout.lineCount - 1)
                snapshotBuilder.buildLineSnapshots(committedText, HalfOpenRange(affectedStartLine, endLine + 1), oldRevision, renderer.getTextColor(), sessionId)
            } else {
                emptyList()
            }
        }

        val newLineSnapshots = snapshotBuilder.buildLineSnapshots(
            virtualText, HalfOpenRange(affectedStartLine, snapshotEndLine + 1), newRevision, renderer.getTextColor(), sessionId
        )

        val affectedEndLine = computeStableSuffixEndLine(
            prevCompositionRevision, committedText, virtualText, virtualLayout,
            compositionReplaceRange, preeditText, affectedStartLine, preeditEndLine,
            oldLineSnapshots, newLineSnapshots
        )

        if (newLineSnapshots.isEmpty()) {
            if (prevCompositionRevision != null) {
                prevCompositionRevision.release(prevCompositionRevision.owner)
            }
            snapshotBuilder.commitRevision(newRevision)
            return TextAnimationStartResult.Skipped
        }

        val offsetMap = if (prevCompositionRevision != null) {
            val oldVirtualText = prevCompositionRevision.virtualText
            val oldReplaceStartUtf8 = SujianEditorBuffer.utf16ToUtf8(oldVirtualText, prevCompositionRevision.preeditRangeInVirtualText.start)
            val oldReplaceEndUtf8 = SujianEditorBuffer.utf16ToUtf8(oldVirtualText, prevCompositionRevision.preeditRangeInVirtualText.endExclusive)
            val newReplaceStartUtf8 = SujianEditorBuffer.utf16ToUtf8(virtualText, compositionReplaceRange.start)
            val newReplaceEndUtf8 = SujianEditorBuffer.utf16ToUtf8(virtualText, compositionReplaceRange.start + preeditText.length)
            EditOffsetMap.fromReplacement(
                oldText = oldVirtualText,
                newText = virtualText,
                oldReplaceStart = oldReplaceStartUtf8,
                oldReplaceEnd = oldReplaceEndUtf8,
                newReplaceStart = newReplaceStartUtf8,
                newReplaceEnd = newReplaceEndUtf8
            )
        } else {
            val committedReplaceStartUtf8 = SujianEditorBuffer.utf16ToUtf8(committedText, compositionReplaceRange.start)
            val committedReplaceEndUtf8 = SujianEditorBuffer.utf16ToUtf8(committedText, compositionReplaceRange.endExclusive)
            val newReplaceStartUtf8 = SujianEditorBuffer.utf16ToUtf8(virtualText, compositionReplaceRange.start)
            val newReplaceEndUtf8 = SujianEditorBuffer.utf16ToUtf8(virtualText, compositionReplaceRange.start + preeditText.length)
            EditOffsetMap.fromReplacement(
                oldText = committedText,
                newText = virtualText,
                oldReplaceStart = committedReplaceStartUtf8,
                oldReplaceEnd = committedReplaceEndUtf8,
                newReplaceStart = newReplaceStartUtf8,
                newReplaceEnd = newReplaceEndUtf8
            )
        }

        val slices = mutableListOf<AndroidAnimatedSlice>()
        val staticPatches = mutableListOf<AndroidStaticLinePatch>()

        val composingStartUtf16 = compositionReplaceRange.start
        val composingEndUtf16 = composingStartUtf16 + preeditText.length

        for (lineSnapshot in newLineSnapshots) {
            for (cluster in lineSnapshot.clusters) {
                val clusterUtf16Start = cluster.platformTextStart
                val clusterUtf16End = cluster.platformTextEnd
                val isComposing = clusterUtf16Start < composingEndUtf16 && clusterUtf16End > composingStartUtf16

                if (isComposing) {
                    slices.add(AndroidAnimatedSlice.insertFadeIn(
                        id = (nextAnimationId() shl 2) + lineSnapshot.visualLineOrdinal.toULong(),
                        snapshotId = lineSnapshot.id,
                        sourceRect = cluster.sourceRectInLineSnapshot,
                        fromRect = RectF(cluster.visualRectInDocument.left, cluster.visualRectInDocument.top, cluster.visualRectInDocument.left + cluster.visualRectInDocument.width(), cluster.visualRectInDocument.top + cluster.visualRectInDocument.height()),
                        toRect = cluster.visualRectInDocument,
                        byteStart = cluster.documentByteStart,
                        byteEnd = cluster.documentByteEnd,
                        shapingIdentity = cluster.shapingIdentity
                    ))
                } else {
                    val mappedOld = offsetMap.mapNewRangeToOld(cluster.documentByteStart, cluster.documentByteEnd)
                    val oldCluster = if (mappedOld != null) {
                        oldLineSnapshots.flatMap { it.clusters }.find { oc ->
                            oc.documentByteStart == mappedOld.start && oc.documentByteEnd == mappedOld.end
                        }
                    } else {
                        null
                    }
                    if (oldCluster != null) {
                        val positionChanged = kotlin.math.abs(oldCluster.visualRectInDocument.top - cluster.visualRectInDocument.top) > 0.5f ||
                            kotlin.math.abs(oldCluster.visualRectInDocument.left - cluster.visualRectInDocument.left) > 0.5f
                        val shapingChanged = oldCluster.shapingIdentity != cluster.shapingIdentity
                        if (shapingChanged) {
                            slices.add(AndroidAnimatedSlice.crossfade(
                                id = (nextAnimationId() shl 2) + cluster.platformTextStart.toULong(),
                                role = AndroidAnimatedSliceRole.CrossfadeNew,
                                snapshotId = lineSnapshot.id,
                                sourceRect = cluster.sourceRectInLineSnapshot,
                                fromRect = oldCluster.visualRectInDocument,
                                toRect = cluster.visualRectInDocument,
                                byteStart = cluster.documentByteStart,
                                byteEnd = cluster.documentByteEnd,
                                shapingIdentity = cluster.shapingIdentity
                            ))
                        } else if (positionChanged) {
                            slices.add(AndroidAnimatedSlice.reflowMove(
                                id = (nextAnimationId() shl 2) + cluster.platformTextStart.toULong(),
                                snapshotId = lineSnapshot.id,
                                sourceRect = cluster.sourceRectInLineSnapshot,
                                fromRect = RectF(oldCluster.visualRectInDocument.left, oldCluster.visualRectInDocument.top, oldCluster.visualRectInDocument.right, oldCluster.visualRectInDocument.bottom),
                                toRect = cluster.visualRectInDocument,
                                byteStart = cluster.documentByteStart,
                                byteEnd = cluster.documentByteEnd,
                                shapingIdentity = cluster.shapingIdentity
                            ))
                        }
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

        val cursorRect = if (virtualText.isNotEmpty() && virtualLayout != null) {
            val cursorUtf16 = (composingStartUtf16 + composingCursorUtf16).coerceIn(0, virtualText.length)
            val cursorLine = virtualLayout.getLineForOffset(cursorUtf16)
            val cursorX = virtualLayout.getPrimaryHorizontal(cursorUtf16)
            val baseline = virtualLayout.getLineBaseline(cursorLine).toFloat()
            val ascent = virtualLayout.getLineAscent(cursorLine).toFloat()
            val descent = virtualLayout.getLineDescent(cursorLine).toFloat()
            RectF(cursorX, baseline + ascent, cursorX, baseline + descent)
        } else {
            RectF(0f, 0f, 0f, 0f)
        }

        val compositionRevision = AndroidCompositionVisualRevision(
            revisionId = newRevision,
            sessionId = sessionId,
            committedText = committedText,
            compositionReplaceRange = compositionReplaceRange,
            preeditRangeInVirtualText = Utf16Range(composingStartUtf16, composingEndUtf16),
            preeditText = preeditText,
            virtualText = virtualText,
            affectedParagraphRange = HalfOpenRange(affectedStartLine, affectedEndLine + 1),
            lineSnapshots = newLineSnapshots,
            cursorRect = cursorRect,
            decorationRanges = listOf(Utf16Range(composingStartUtf16, composingEndUtf16))
        )
        compositionRevision.transferToTransaction(txKey)

        val cursorTransition = AndroidCursorTransition.tween(
            cursorRect,
            cursorRect,
            animationDurationMs
        )

        val decorationSlices = mutableListOf<AndroidDecorationSlice>()
        for (decRange in compositionRevision.decorationRanges) {
            decorationSlices.add(AndroidDecorationSlice(decRange, DecorationKind.Underline))
        }

        val oldRevisionId = if (prevCompositionRevision != null) prevCompositionRevision.revisionId else snapshotBuilder.currentCommittedRevision()

        if (prevCompositionRevision != null && prevRevisionFromActiveTransaction) {
            prevCompositionRevision.reassignToTransaction(txKey)
        }

        val tx = AndroidPlatformVisualTransaction(
            key = txKey,
            state = AndroidVisualTransactionState.Pending,
            operationKind = AndroidVisualOperationKind.CompositionUpdate,
            animationMode = AnimationModeData.GlyphAnimation,
            durationMs = animationDurationMs,
            oldRevision = oldRevisionId,
            newRevision = newRevision,
            slices = slices,
            staticLinePatches = staticPatches.toMutableList(),
            decorationSlices = decorationSlices,
            cursorTransition = cursorTransition,
            ownedOldRevision = prevCompositionRevision,
            ownedNewRevision = compositionRevision,
            onTransactionComplete = { newRev, completedTxKey ->
                compositionManager.returnFromTransaction(newRev, completedTxKey, managerGeneration)
            }
        )

        if (!renderer.addTransaction(tx)) {
            tx.cancel("add_failed")
            snapshotBuilder.commitRevision(newRevision)
            return TextAnimationStartResult.Skipped
        }

        snapshotBuilder.commitRevision(newRevision)

        if (coordinatedAnimationEnabled) {
            cursorController.setTransactionDrivenCursor(cursorTransition)
            cursorController.setActiveTransaction(renderer.getActiveTransactions().lastOrNull())
        }

        return TextAnimationStartResult.Started
    }

    private fun computeStableSuffixEndLine(
        prevCompositionRevision: AndroidCompositionVisualRevision?,
        committedText: String,
        virtualText: String,
        virtualLayout: android.text.Layout,
        compositionReplaceRange: Utf16Range,
        preeditText: String,
        affectedStartLine: Int,
        preeditEndLine: Int,
        oldLineSnapshots: List<AndroidLineSnapshot> = emptyList(),
        newLineSnapshots: List<AndroidLineSnapshot> = emptyList()
    ): Int {
        val oldText = prevCompositionRevision?.virtualText ?: committedText
        val oldLayout = if (oldText.isNotEmpty()) layout.getLayout(oldText) else null

        val offsetMap = if (prevCompositionRevision != null) {
            val oldReplaceStartUtf8 = SujianEditorBuffer.utf16ToUtf8(oldText, prevCompositionRevision.preeditRangeInVirtualText.start)
            val oldReplaceEndUtf8 = SujianEditorBuffer.utf16ToUtf8(oldText, prevCompositionRevision.preeditRangeInVirtualText.endExclusive)
            val newReplaceStartUtf8 = SujianEditorBuffer.utf16ToUtf8(virtualText, compositionReplaceRange.start)
            val newReplaceEndUtf8 = SujianEditorBuffer.utf16ToUtf8(virtualText, compositionReplaceRange.start + preeditText.length)
            EditOffsetMap.fromReplacement(
                oldText = oldText,
                newText = virtualText,
                oldReplaceStart = oldReplaceStartUtf8,
                oldReplaceEnd = oldReplaceEndUtf8,
                newReplaceStart = newReplaceStartUtf8,
                newReplaceEnd = newReplaceEndUtf8
            )
        } else {
            val committedReplaceStartUtf8 = SujianEditorBuffer.utf16ToUtf8(committedText, compositionReplaceRange.start)
            val committedReplaceEndUtf8 = SujianEditorBuffer.utf16ToUtf8(committedText, compositionReplaceRange.endExclusive)
            val newReplaceStartUtf8 = SujianEditorBuffer.utf16ToUtf8(virtualText, compositionReplaceRange.start)
            val newReplaceEndUtf8 = SujianEditorBuffer.utf16ToUtf8(virtualText, compositionReplaceRange.start + preeditText.length)
            EditOffsetMap.fromReplacement(
                oldText = committedText,
                newText = virtualText,
                oldReplaceStart = committedReplaceStartUtf8,
                oldReplaceEnd = committedReplaceEndUtf8,
                newReplaceStart = newReplaceStartUtf8,
                newReplaceEnd = newReplaceEndUtf8
            )
        }

        val affectedParagraphEndLine = findAffectedParagraphEndLine(virtualLayout, preeditEndLine)

        var candidateEnd = preeditEndLine
        val stableConsecutiveNeeded = 2
        var stableConsecutive = 0
        val lastLine = virtualLayout.lineCount - 1

        while (candidateEnd < lastLine && stableConsecutive < stableConsecutiveNeeded) {
            if (candidateEnd > affectedParagraphEndLine + 1) {
                break
            }

            val lineStart = virtualLayout.getLineStart(candidateEnd)
            val lineEnd = virtualLayout.getLineEnd(candidateEnd)
            if (lineStart >= lineEnd) {
                stableConsecutive++
                candidateEnd++
                continue
            }

            val byteStart = SujianEditorBuffer.utf16ToUtf8(virtualText, lineStart)
            val byteEnd = SujianEditorBuffer.utf16ToUtf8(virtualText, lineEnd.coerceAtMost(virtualText.length))

            val oldRange = offsetMap.mapNewRangeToOld(byteStart, byteEnd)
            val isStable = if (oldRange != null && oldLayout != null) {
                val oldByteEnd = SujianEditorBuffer.utf16ToUtf8(oldText, oldText.length)
                if (oldRange.end > oldByteEnd) {
                    false
                } else {
                    val oldUtf16Start = SujianEditorBuffer.utf8ToUtf16(oldText, oldRange.start).coerceIn(0, oldText.length)
                    val oldLineIdx = oldLayout.getLineForOffset(oldUtf16Start)
                    val oldLineEnd = oldLayout.getLineEnd(oldLineIdx)
                    val oldByteLineEnd = SujianEditorBuffer.utf16ToUtf8(oldText, oldLineEnd.coerceAtMost(oldText.length))
                    val newLineEnd = lineEnd
                    val newByteLineEnd = SujianEditorBuffer.utf16ToUtf8(virtualText, newLineEnd.coerceAtMost(virtualText.length))
                    val mappedOldLineEnd = offsetMap.mapNewRangeToOld(newByteLineEnd - 1, newByteLineEnd)
                    val lineBoundaryStable = mappedOldLineEnd != null && mappedOldLineEnd.end == oldByteLineEnd
                    if (!lineBoundaryStable) {
                        false
                    } else {
                        val newLineSnap = newLineSnapshots.find { ns ->
                            ns.documentByteStart == byteStart || (ns.documentByteStart <= byteStart && ns.documentByteEnd >= byteEnd)
                        }
                        val oldLineSnap = oldLineSnapshots.find { os ->
                            os.documentByteStart == oldRange.start || (os.documentByteStart <= oldRange.start && os.documentByteEnd >= oldRange.end)
                        }
                        val probeOldClusters: List<ClusterStabilityInfo>? = if (oldLineSnap == null && oldLayout != null && oldText.isNotEmpty()) {
                            val pOldUtf16Start = SujianEditorBuffer.utf8ToUtf16(oldText, oldRange.start).coerceIn(0, oldText.length)
                            val pOldLineIdx = oldLayout.getLineForOffset(pOldUtf16Start)
                            snapshotBuilder.buildLineLayoutProbes(oldText, HalfOpenRange(pOldLineIdx, pOldLineIdx + 1)).firstOrNull()?.clusters?.filter { c ->
                                c.documentByteStart >= oldRange.start && c.documentByteEnd <= oldRange.end
                            }
                        } else null
                         val probeNewClusters: List<ClusterStabilityInfo>? = if (newLineSnap == null) {
                             snapshotBuilder.buildLineLayoutProbes(virtualText, HalfOpenRange(candidateEnd, candidateEnd + 1)).firstOrNull()?.clusters?.filter { c ->
                                 c.documentByteStart >= byteStart && c.documentByteEnd <= byteEnd
                             }
                          } else null
                         val effectiveOldClusters: List<ClusterStabilityInfo>? = oldLineSnap?.clusters?.filter { c ->
                             c.documentByteStart >= oldRange.start && c.documentByteEnd <= oldRange.end
                         } ?: probeOldClusters
                         val effectiveNewClusters: List<ClusterStabilityInfo>? = newLineSnap?.clusters?.filter { c ->
                             c.documentByteStart >= byteStart && c.documentByteEnd <= byteEnd
                         } ?: probeNewClusters
                         if (effectiveOldClusters == null || effectiveNewClusters == null) {
                             false
                         } else if (effectiveOldClusters.isEmpty() && effectiveNewClusters.isEmpty()) {
                             true
                         } else if (effectiveNewClusters.size != effectiveOldClusters.size) {
                             false
                         } else {
                             effectiveNewClusters.zip(effectiveOldClusters).all { (nc, oc) ->
                                 nc.shapingIdentity == oc.shapingIdentity &&
                                 nc.textDirection == oc.textDirection &&
                                 nc.documentByteEnd - nc.documentByteStart == oc.documentByteEnd - oc.documentByteStart
                             }
                         }
                     }
                 }
             } else {
                 false
             }

             if (isStable) {
                 stableConsecutive++
             } else {
                 stableConsecutive = 0
             }
             candidateEnd++
         }

         if (stableConsecutive >= stableConsecutiveNeeded) {
            return (candidateEnd - stableConsecutiveNeeded + 1).coerceAtLeast(affectedStartLine)
        }
        return affectedParagraphEndLine.coerceAtMost(lastLine).coerceAtLeast(affectedStartLine)
    }

    fun handleCompositionCommitOrCancel(
        committedText: String,
        isCommit: Boolean,
        committedCandidateText: String = "",
        candidateUtf16Start: Int = 0,
        candidateUtf16EndExclusive: Int = 0
    ): TextAnimationStartResult {
        val txKey = nextAnimationId()
        val managerGeneration = compositionManager.getGeneration()
        var prevRevision: AndroidCompositionVisualRevision? = null
        var prevRevisionFromActiveTransaction = false
        var detachedOldFromActive: OwnedVisualRevision? = null

        when (val takeResult = compositionManager.takeCurrentForTransactionTyped(txKey)) {
            is TakeCurrentResult.Success -> {
                prevRevision = takeResult.revision
            }
            is TakeCurrentResult.RevisionWithActiveTransaction -> {
                val source = renderer.takeCompositionSource(takeResult.activeTransactionKey, txKey)
                if (source != null) {
                    prevRevision = source.newRevision as? AndroidCompositionVisualRevision
                    detachedOldFromActive = source.oldRevision
                    prevRevisionFromActiveTransaction = true
                }
                compositionManager.reassignActiveTransactionKey(txKey)
            }
            is TakeCurrentResult.NoRevisionAvailable -> {
            }
        }

        if (detachedOldFromActive != null) {
            detachedOldFromActive.release(detachedOldFromActive.owner)
        }

        compositionManager.clear()

        if (!animationEnabled || prevRevision == null) {
            if (prevRevision != null) {
                prevRevision.release(prevRevision.owner)
            }
            return TextAnimationStartResult.Skipped
        }

        val oldRevision = snapshotBuilder.currentCommittedRevision()
        val newRevision = snapshotBuilder.allocateNextRevision()

        if (prevRevisionFromActiveTransaction) {
            prevRevision?.reassignToTransaction(txKey)
        }

        val oldLineSnapshots = prevRevision.lineSnapshots.toMutableList()
        val newText = committedText
        val newLayout = if (newText.isNotEmpty()) layout.getLayout(newText) else null

        val newAffectedRange: HalfOpenRange? = if (newLayout != null && newText.isNotEmpty()) {
            val preeditStartLine = if (prevRevision.preeditRangeInVirtualText.start < prevRevision.virtualText.length) {
                val prevLayout = if (prevRevision.virtualText.isNotEmpty()) layout.getLayout(prevRevision.virtualText) else null
                prevLayout?.getLineForOffset(prevRevision.preeditRangeInVirtualText.start.coerceIn(0, prevRevision.virtualText.length)) ?: 0
            } else 0
            val preeditEndLine = newLayout.getLineForOffset(
                prevRevision.compositionReplaceRange.start.coerceIn(0, newText.length)
            )
            val commitCancelSnapshotEndLine = findAffectedParagraphEndLine(newLayout, preeditEndLine).coerceAtMost(newLayout.lineCount - 1)
            val preliminaryEndLine = computeCommitCancelStableSuffixEndLine(
                prevRevision, newText, newLayout, preeditEndLine,
                isCommit = isCommit,
                candidateUtf16Start = candidateUtf16Start,
                candidateUtf16EndExclusive = candidateUtf16EndExclusive
            )
            val preliminaryNewLineSnapshots = snapshotBuilder.buildLineSnapshots(
                newText, HalfOpenRange(preeditStartLine.coerceAtMost(commitCancelSnapshotEndLine), commitCancelSnapshotEndLine + 1), newRevision, renderer.getTextColor(), prevRevision.sessionId
            )
            val endLine = computeCommitCancelStableSuffixEndLine(
                prevRevision, newText, newLayout, preeditEndLine,
                isCommit = isCommit,
                candidateUtf16Start = candidateUtf16Start,
                candidateUtf16EndExclusive = candidateUtf16EndExclusive,
                oldLineSnapshots = oldLineSnapshots,
                newLineSnapshots = preliminaryNewLineSnapshots
            )
            HalfOpenRange(preeditStartLine.coerceAtMost(endLine), endLine + 1)
        } else {
            null
        }

        val newLineSnapshots: List<AndroidLineSnapshot> = newAffectedRange?.let {
            snapshotBuilder.buildLineSnapshots(newText, it, newRevision, renderer.getTextColor(), prevRevision.sessionId)
        } ?: emptyList()

        val slices = mutableListOf<AndroidAnimatedSlice>()
        val staticPatches = mutableListOf<AndroidStaticLinePatch>()

        val offsetMap = if (isCommit) {
            val oldPreeditByteStart = SujianEditorBuffer.utf16ToUtf8(prevRevision.virtualText, prevRevision.preeditRangeInVirtualText.start)
            val oldPreeditByteEnd = SujianEditorBuffer.utf16ToUtf8(prevRevision.virtualText, prevRevision.preeditRangeInVirtualText.endExclusive)
            val newCommittedByteStart = SujianEditorBuffer.utf16ToUtf8(newText, candidateUtf16Start.coerceIn(0, newText.length))
            val newCommittedByteEnd = SujianEditorBuffer.utf16ToUtf8(newText, candidateUtf16EndExclusive.coerceIn(0, newText.length))
            EditOffsetMap.fromReplacement(
                oldText = prevRevision.virtualText,
                newText = newText,
                oldReplaceStart = oldPreeditByteStart,
                oldReplaceEnd = oldPreeditByteEnd,
                newReplaceStart = newCommittedByteStart,
                newReplaceEnd = newCommittedByteEnd
            )
        } else {
            val oldPreeditByteStart = SujianEditorBuffer.utf16ToUtf8(prevRevision.virtualText, prevRevision.preeditRangeInVirtualText.start)
            val oldPreeditByteEnd = SujianEditorBuffer.utf16ToUtf8(prevRevision.virtualText, prevRevision.preeditRangeInVirtualText.endExclusive)
            val candidateUtf16Start = prevRevision.compositionReplaceRange.start
            val newCommittedByteStart = SujianEditorBuffer.utf16ToUtf8(newText, candidateUtf16Start)
            EditOffsetMap.fromReplacement(
                oldText = prevRevision.virtualText,
                newText = newText,
                oldReplaceStart = oldPreeditByteStart,
                oldReplaceEnd = oldPreeditByteEnd,
                newReplaceStart = newCommittedByteStart,
                newReplaceEnd = newCommittedByteStart
            )
        }

        if (isCommit) {
            val visualTextUnchanged = prevRevision.virtualText == newText
            if (visualTextUnchanged) {
                // 视觉文字完全相同：不重复播放吐字，只移除 underline/装饰
            } else {
                for (oldSnap in prevRevision.lineSnapshots) {
                    for (cluster in oldSnap.clusters) {
                        val utf16Start = cluster.platformTextStart
                        val utf16End = cluster.platformTextEnd
                        val wasPreedit = prevRevision.preeditRangeInVirtualText.let { r ->
                            utf16Start < r.endExclusive && utf16End > r.start
                        }
                        if (wasPreedit) {
                            val mappedNew = offsetMap.mapOldRangeToNew(cluster.documentByteStart, cluster.documentByteEnd)
                            val matchedInNew = mappedNew != null && newLineSnapshots.any { ns ->
                                ns.clusters.any { nc ->
                                    nc.documentByteStart == mappedNew.start && nc.documentByteEnd == mappedNew.end
                                }
                            }
                            if (!matchedInNew) {
                                slices.add(AndroidAnimatedSlice.deleteFadeOut(
                                    id = nextAnimationId(),
                                    snapshotId = oldSnap.id,
                                    sourceRect = cluster.sourceRectInLineSnapshot,
                                    fromRect = cluster.visualRectInDocument,
                                    toRect = RectF(prevRevision.cursorRect.left, prevRevision.cursorRect.top, prevRevision.cursorRect.left + cluster.visualRectInDocument.width() * 0.7f, prevRevision.cursorRect.top + cluster.visualRectInDocument.height() * 0.7f),
                                    byteStart = cluster.documentByteStart,
                                    byteEnd = cluster.documentByteEnd,
                                    shapingIdentity = cluster.shapingIdentity
                                ))
                            } else {
                                val oldCluster = cluster
                                val newCluster = newLineSnapshots.flatMap { it.clusters }.find { nc ->
                                    mappedNew != null && nc.documentByteStart == mappedNew.start && nc.documentByteEnd == mappedNew.end
                                }
                                if (newCluster != null && oldCluster.shapingIdentity != newCluster.shapingIdentity) {
                                    val oldLineSnap = prevRevision.lineSnapshots.find { it.clusters.contains(oldCluster) }
                                    if (oldLineSnap != null) {
                                        slices.add(AndroidAnimatedSlice.crossfade(
                                            id = nextAnimationId(),
                                            role = AndroidAnimatedSliceRole.CrossfadeOld,
                                            snapshotId = oldLineSnap.id,
                                            sourceRect = oldCluster.sourceRectInLineSnapshot,
                                            fromRect = oldCluster.visualRectInDocument,
                                            toRect = newCluster.visualRectInDocument,
                                            byteStart = oldCluster.documentByteStart,
                                            byteEnd = oldCluster.documentByteEnd,
                                            shapingIdentity = oldCluster.shapingIdentity
                                        ))
                                    }
                                }
                            }
                        }
                    }
                }

                for (newSnap in newLineSnapshots) {
                    for (cluster in newSnap.clusters) {
                        val mappedOld = offsetMap.mapNewRangeToOld(cluster.documentByteStart, cluster.documentByteEnd)
                        val foundInOld = mappedOld != null && prevRevision.lineSnapshots.any { os ->
                            os.clusters.any { oc ->
                                oc.documentByteStart == mappedOld.start && oc.documentByteEnd == mappedOld.end
                            }
                        }
                        if (!foundInOld) {
                            val composingStartUtf16 = prevRevision.preeditRangeInVirtualText.start
                            val composingEndUtf16 = prevRevision.preeditRangeInVirtualText.endExclusive
                            val isComposing = cluster.platformTextStart < composingEndUtf16 && cluster.platformTextEnd > composingStartUtf16
                            if (isComposing) {
                                slices.add(AndroidAnimatedSlice.insertFadeIn(
                                    id = nextAnimationId(),
                                    snapshotId = newSnap.id,
                                    sourceRect = cluster.sourceRectInLineSnapshot,
                                    fromRect = RectF(prevRevision.cursorRect.left, prevRevision.cursorRect.top, prevRevision.cursorRect.left + cluster.visualRectInDocument.width(), prevRevision.cursorRect.top + cluster.visualRectInDocument.height()),
                                    toRect = cluster.visualRectInDocument,
                                    byteStart = cluster.documentByteStart,
                                    byteEnd = cluster.documentByteEnd,
                                    shapingIdentity = cluster.shapingIdentity
                                ))
                            }
                        } else {
                            val oldCluster = prevRevision.lineSnapshots.flatMap { it.clusters }.find { oc ->
                                mappedOld != null && oc.documentByteStart == mappedOld.start && oc.documentByteEnd == mappedOld.end
                            }
                            if (oldCluster != null && oldCluster.shapingIdentity != cluster.shapingIdentity) {
                                slices.add(AndroidAnimatedSlice.crossfade(
                                    id = nextAnimationId(),
                                    role = AndroidAnimatedSliceRole.CrossfadeNew,
                                    snapshotId = newSnap.id,
                                    sourceRect = cluster.sourceRectInLineSnapshot,
                                    fromRect = oldCluster.visualRectInDocument,
                                    toRect = cluster.visualRectInDocument,
                                    byteStart = cluster.documentByteStart,
                                    byteEnd = cluster.documentByteEnd,
                                    shapingIdentity = cluster.shapingIdentity
                                ))
                            }
                        }
                    }
                }
            }
        } else {
            for (oldSnap in prevRevision.lineSnapshots) {
                for (cluster in oldSnap.clusters) {
                    val utf16Start = cluster.platformTextStart
                    val utf16End = cluster.platformTextEnd
                    val wasPreedit = prevRevision.preeditRangeInVirtualText.let { r ->
                        utf16Start < r.endExclusive && utf16End > r.start
                    }
                    if (wasPreedit) {
                        slices.add(AndroidAnimatedSlice.deleteFadeOut(
                            id = nextAnimationId(),
                            snapshotId = oldSnap.id,
                            sourceRect = cluster.sourceRectInLineSnapshot,
                            fromRect = cluster.visualRectInDocument,
                            toRect = RectF(prevRevision.cursorRect.left, prevRevision.cursorRect.top, prevRevision.cursorRect.left + cluster.visualRectInDocument.width() * 0.7f, prevRevision.cursorRect.top + cluster.visualRectInDocument.height() * 0.7f),
                            byteStart = cluster.documentByteStart,
                            byteEnd = cluster.documentByteEnd,
                            shapingIdentity = cluster.shapingIdentity
                        ))
                    }
                }
            }
        }

        for (newSnap in newLineSnapshots) {
            val animatedByteRanges = slices.map { Pair(it.documentByteStart, it.documentByteEnd) }
            val visibleSourceRects = mutableListOf<RectF>()
            for (cluster in newSnap.clusters) {
                val isAnimated = animatedByteRanges.any { (start, end) ->
                    !(cluster.documentByteEnd <= start || cluster.documentByteStart >= end)
                }
                if (!isAnimated) {
                    visibleSourceRects.add(cluster.sourceRectInLineSnapshot)
                }
            }
            staticPatches.add(AndroidStaticLinePatch(
                newSnapshotId = newSnap.id,
                destinationDocumentRect = newSnap.documentRect,
                visibleSourceRects = visibleSourceRects
            ))
        }

        val cursorRect = if (newLayout != null && newText.isNotEmpty()) {
            val cursorPos = buffer.selection.head.coerceIn(0, newText.length)
            val cursorLine = newLayout.getLineForOffset(cursorPos)
            val cursorX = newLayout.getPrimaryHorizontal(cursorPos)
            val baseline = newLayout.getLineBaseline(cursorLine).toFloat()
            val ascent = newLayout.getLineAscent(cursorLine).toFloat()
            val descent = newLayout.getLineDescent(cursorLine).toFloat()
            RectF(cursorX, baseline + ascent, cursorX, baseline + descent)
        } else {
            prevRevision.cursorRect
        }

        val cursorTransition = AndroidCursorTransition.tween(
            prevRevision.cursorRect,
            cursorRect,
            animationDurationMs
        )

        val commitCancelRevision = if (newLineSnapshots.isNotEmpty()) {
            CommittedVisualRevision(
                revisionId = newRevision,
                sessionId = prevRevision.sessionId,
                fullText = newText,
                affectedParagraphRange = newAffectedRange ?: HalfOpenRange.EMPTY,
                lineSnapshots = newLineSnapshots,
                cursorRect = cursorRect
            )
        } else {
            null
        }
        commitCancelRevision?.transferToTransaction(txKey)

        val tx = AndroidPlatformVisualTransaction(
            key = txKey,
            state = AndroidVisualTransactionState.Pending,
            operationKind = AndroidVisualOperationKind.CompositionCommitOrCancel,
            animationMode = AnimationModeData.GlyphAnimation,
            durationMs = animationDurationMs,
            oldRevision = oldRevision,
            newRevision = newRevision,
            slices = slices,
            staticLinePatches = staticPatches.toMutableList(),
            decorationSlices = mutableListOf(),
            cursorTransition = cursorTransition,
            ownedOldRevision = prevRevision,
            ownedNewRevision = commitCancelRevision,
            onTransactionComplete = { rev, completedTxKey ->
                compositionManager.returnFromTransaction(rev, completedTxKey, managerGeneration)
            }
        )

        if (!renderer.addTransaction(tx)) {
            tx.cancel("add_failed")
            snapshotBuilder.commitRevision(newRevision)
            return TextAnimationStartResult.Skipped
        }

        snapshotBuilder.commitRevision(newRevision)

        if (coordinatedAnimationEnabled) {
            cursorController.setTransactionDrivenCursor(cursorTransition)
            cursorController.setActiveTransaction(renderer.getActiveTransactions().lastOrNull())
        }

        return TextAnimationStartResult.Started
    }

    private fun computeCommitCancelStableSuffixEndLine(
        prevRevision: AndroidCompositionVisualRevision,
        newText: String,
        newLayout: android.text.Layout,
        preeditEndLine: Int,
        isCommit: Boolean = false,
        candidateUtf16Start: Int = 0,
        candidateUtf16EndExclusive: Int = 0,
        oldLineSnapshots: List<AndroidLineSnapshot> = emptyList(),
        newLineSnapshots: List<AndroidLineSnapshot> = emptyList()
    ): Int {
        val oldText = prevRevision.virtualText
        val oldLayout = if (oldText.isNotEmpty()) layout.getLayout(oldText) else null

        val oldPreeditByteStart = SujianEditorBuffer.utf16ToUtf8(oldText, prevRevision.preeditRangeInVirtualText.start)
        val oldPreeditByteEnd = SujianEditorBuffer.utf16ToUtf8(oldText, prevRevision.preeditRangeInVirtualText.endExclusive)
        val newCommittedByteStart = SujianEditorBuffer.utf16ToUtf8(newText, candidateUtf16Start.coerceIn(0, newText.length))
        val newCommittedByteEnd = if (isCommit) {
            SujianEditorBuffer.utf16ToUtf8(newText, candidateUtf16EndExclusive.coerceIn(0, newText.length))
        } else {
            newCommittedByteStart
        }

        val offsetMap = EditOffsetMap.fromReplacement(
            oldText = oldText,
            newText = newText,
            oldReplaceStart = oldPreeditByteStart,
            oldReplaceEnd = oldPreeditByteEnd,
            newReplaceStart = newCommittedByteStart,
            newReplaceEnd = newCommittedByteEnd
        )

        val affectedParagraphEndLine = findAffectedParagraphEndLine(newLayout, preeditEndLine)

        var candidateEnd = preeditEndLine
        val stableConsecutiveNeeded = 2
        var stableConsecutive = 0
        val lastLine = newLayout.lineCount - 1

        while (candidateEnd < lastLine && stableConsecutive < stableConsecutiveNeeded) {
            if (candidateEnd > affectedParagraphEndLine + 1) {
                break
            }

            val lineStart = newLayout.getLineStart(candidateEnd)
            val lineEnd = newLayout.getLineEnd(candidateEnd)
            if (lineStart >= lineEnd) {
                stableConsecutive++
                candidateEnd++
                continue
            }

            val byteStart = SujianEditorBuffer.utf16ToUtf8(newText, lineStart)
            val byteEnd = SujianEditorBuffer.utf16ToUtf8(newText, lineEnd.coerceAtMost(newText.length))

            val oldRange = offsetMap.mapNewRangeToOld(byteStart, byteEnd)
            val isStable = if (oldRange != null && oldLayout != null) {
                val oldByteEnd = SujianEditorBuffer.utf16ToUtf8(oldText, oldText.length)
                if (oldRange.end > oldByteEnd) {
                    false
                } else {
                    val oldUtf16Start = SujianEditorBuffer.utf8ToUtf16(oldText, oldRange.start).coerceIn(0, oldText.length)
                    val oldLineIdx = oldLayout.getLineForOffset(oldUtf16Start)
                    val oldLineEnd = oldLayout.getLineEnd(oldLineIdx)
                    val oldByteLineEnd = SujianEditorBuffer.utf16ToUtf8(oldText, oldLineEnd.coerceAtMost(oldText.length))
                    val newLineEnd = lineEnd
                    val newByteLineEnd = SujianEditorBuffer.utf16ToUtf8(newText, newLineEnd.coerceAtMost(newText.length))
                    val mappedOldLineEnd = offsetMap.mapNewRangeToOld(newByteLineEnd - 1, newByteLineEnd)
                    val lineBoundaryStable = mappedOldLineEnd != null && mappedOldLineEnd.end == oldByteLineEnd
                    if (!lineBoundaryStable) {
                        false
                    } else {
                        val newLineSnap = newLineSnapshots.find { ns ->
                            ns.documentByteStart == byteStart || (ns.documentByteStart <= byteStart && ns.documentByteEnd >= byteEnd)
                        }
                        val oldLineSnap = oldLineSnapshots.find { os ->
                            os.documentByteStart == oldRange.start || (os.documentByteStart <= oldRange.start && os.documentByteEnd >= oldRange.end)
                        }
                        val probeOldClusters: List<ClusterStabilityInfo>? = if (oldLineSnap == null && oldLayout != null && oldText.isNotEmpty()) {
                            val pOldUtf16Start = SujianEditorBuffer.utf8ToUtf16(oldText, oldRange.start).coerceIn(0, oldText.length)
                            val pOldLineIdx = oldLayout.getLineForOffset(pOldUtf16Start)
                            snapshotBuilder.buildLineLayoutProbes(oldText, HalfOpenRange(pOldLineIdx, pOldLineIdx + 1)).firstOrNull()?.clusters?.filter { c ->
                                c.documentByteStart >= oldRange.start && c.documentByteEnd <= oldRange.end
                            }
                        } else null
                         val probeNewClusters: List<ClusterStabilityInfo>? = if (newLineSnap == null) {
                             snapshotBuilder.buildLineLayoutProbes(newText, HalfOpenRange(candidateEnd, candidateEnd + 1)).firstOrNull()?.clusters?.filter { c ->
                                 c.documentByteStart >= byteStart && c.documentByteEnd <= byteEnd
                             }
                         } else null
                         val effectiveOldClusters: List<ClusterStabilityInfo>? = oldLineSnap?.clusters?.filter { c ->
                             c.documentByteStart >= oldRange.start && c.documentByteEnd <= oldRange.end
                         } ?: probeOldClusters
                         val effectiveNewClusters: List<ClusterStabilityInfo>? = newLineSnap?.clusters?.filter { c ->
                             c.documentByteStart >= byteStart && c.documentByteEnd <= byteEnd
                         } ?: probeNewClusters
                         if (effectiveOldClusters == null || effectiveNewClusters == null) {
                             false
                         } else if (effectiveOldClusters.isEmpty() && effectiveNewClusters.isEmpty()) {
                             true
                         } else if (effectiveNewClusters.size != effectiveOldClusters.size) {
                             false
                         } else {
                             effectiveNewClusters.zip(effectiveOldClusters).all { (nc, oc) ->
                                 nc.shapingIdentity == oc.shapingIdentity &&
                                 nc.textDirection == oc.textDirection &&
                                 nc.documentByteEnd - nc.documentByteStart == oc.documentByteEnd - oc.documentByteStart
                             }
                         }
                     }
                 }
             } else {
                 false
             }

             if (isStable) {
                 stableConsecutive++
             } else {
                 stableConsecutive = 0
             }
             candidateEnd++
         }

         if (stableConsecutive >= stableConsecutiveNeeded) {
             return (candidateEnd - stableConsecutiveNeeded + 1).coerceAtLeast(preeditEndLine)
        }
        return affectedParagraphEndLine.coerceAtMost(lastLine).coerceAtLeast(preeditEndLine)
    }

    fun handleCursorOnlyTransaction(
        oldCursorRect: RectF,
        newCursorRect: RectF,
        durationMs: Long = cursorController.smoothCursorDurationMs
    ): TextAnimationStartResult {
        if (!cursorController.smoothCursorEnabled) {
            return TextAnimationStartResult.Skipped
        }

        val oldRevision = snapshotBuilder.currentCommittedRevision()
        val newRevision = snapshotBuilder.allocateNextRevision()

        val cursorTransition = AndroidCursorTransition.tween(
            oldCursorRect, newCursorRect, durationMs
        )

        val tx = AndroidPlatformVisualTransaction(
            key = nextAnimationId(),
            state = AndroidVisualTransactionState.Pending,
            operationKind = AndroidVisualOperationKind.Cursor,
            animationMode = AnimationModeData.GlyphAnimation,
            durationMs = durationMs,
            oldRevision = oldRevision,
            newRevision = newRevision,
            slices = mutableListOf(),
            staticLinePatches = mutableListOf(),
            decorationSlices = mutableListOf(),
            cursorTransition = cursorTransition
        )

        if (!renderer.addTransaction(tx)) {
            snapshotBuilder.commitRevision(newRevision)
            return TextAnimationStartResult.Skipped
        }

        snapshotBuilder.commitRevision(newRevision)

        if (coordinatedAnimationEnabled) {
            cursorController.setTransactionDrivenCursor(cursorTransition)
            cursorController.setActiveTransaction(tx)
        }

        return TextAnimationStartResult.Started
    }

    fun tick() {
        renderer.tickAnimations()
        cursorController.tickCursorFromTimeline()
    }

    fun hasActiveAnimations(): Boolean = renderer.hasActiveAnimations() || cursorController.isCursorAnimating

    fun onDetachedFromWindow() {
        renderer.clearAnimations()
        deleteSnapshots.forEach { it.release() }
        deleteSnapshots.clear()
    }

    fun clearState() {
        renderer.clearAnimations()
        deleteSnapshots.forEach { it.release() }
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
