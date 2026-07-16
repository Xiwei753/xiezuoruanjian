package com.xiwei.sujian.editor.v2.visual

import com.xiwei.sujian.editor.v2.mirror.VisualIntent
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutRevision
import com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshotBuilder
import com.xiwei.sujian.editor.v2.layout.LineClusterSnapshot

class AndroidVisualPlanner {
    private var oldRevision: AndroidLayoutRevision? = null
    private var oldLayout: android.text.Layout? = null
    private val snapshotBuilder = AndroidLineSnapshotBuilder()

    fun prepare(
        visualIntent: VisualIntent,
        layoutEngine: com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine,
        resourceStore: VisualResourceStore
    ): PreparedVisualTransaction {
        val newRevision = layoutEngine.getCurrentRevision()
        val layout = layoutEngine.getLayout()
        val durationMs = visualIntent.durationMs
        val transactionKey = System.nanoTime()

        val staticPatches = mutableListOf<PreparedVisualTransaction.StaticPatch>()
        val animatedSlices = mutableListOf<PreparedVisualTransaction.AnimatedSlice>()
        var cursorTransition: PreparedVisualTransaction.CursorTransition? = null

        val oldRev = oldRevision
        val savedOldLayout = oldLayout
        val newRev = newRevision
        val snapshotOwner = SnapshotOwner.OwnedByTransaction(transactionKey)

        if (oldRev != null && newRev != null && layout != null) {
            val affectedLines = computeAffectedLines(visualIntent, oldRev, newRev)
            val mode = parseAnimationMode(visualIntent.animationMode)

            when (mode) {
                AnimationMode.GlyphAnimation, AnimationMode.ClusterAnimation -> {
                    planClusterLevelAnimation(
                        visualIntent, oldRev, newRev, savedOldLayout, layout,
                        affectedLines, animatedSlices, staticPatches, resourceStore, snapshotOwner,
                        layoutEngine
                    )
                }
                AnimationMode.RunAnimation -> {
                    planRunAnimation(
                        visualIntent, oldRev, newRev, savedOldLayout, layout,
                        affectedLines, animatedSlices, staticPatches, resourceStore, snapshotOwner,
                        layoutEngine
                    )
                }
                AnimationMode.LineReflowAnimation -> {
                    planLineReflowAnimation(
                        visualIntent, oldRev, newRev, savedOldLayout, layout,
                        affectedLines, animatedSlices, staticPatches, resourceStore, snapshotOwner,
                        layoutEngine
                    )
                }
                AnimationMode.SystemSuppressed -> {
                    planNoAnimation(newRev, staticPatches)
                }
                else -> {
                    planCrossfadeAnimation(
                        visualIntent, oldRev, newRev, savedOldLayout, layout,
                        affectedLines, animatedSlices, staticPatches, resourceStore, snapshotOwner,
                        layoutEngine
                    )
                }
            }
        } else if (newRev != null) {
            planNoAnimation(newRev, staticPatches)
        }

        if (visualIntent.coordinatedCursor.shouldAnimate && newRev != null) {
            val cursorLine = layoutEngine.getCursorLine()
            val cursorX = layoutEngine.getPrimaryHorizontalUtf8(visualIntent.coordinatedCursor.newByteOffset)
            val lineRange = newRev.lineRanges.getOrNull(cursorLine)

            val fromX = if (oldRev != null) {
                layoutEngine.getPrimaryHorizontalUtf8(visualIntent.coordinatedCursor.oldByteOffset)
            } else cursorX

            cursorTransition = PreparedVisualTransaction.CursorTransition(
                fromX = fromX,
                fromY = lineRange?.top ?: 0f,
                fromHeight = (lineRange?.bottom ?: 0f) - (lineRange?.top ?: 0f),
                toX = cursorX,
                toY = lineRange?.top ?: 0f,
                toHeight = (lineRange?.bottom ?: 0f) - (lineRange?.top ?: 0f),
                shouldAnimate = true
            )
        }

        val result = PreparedVisualTransaction(
            transactionId = transactionKey,
            oldRevision = oldRev,
            newRevision = newRev,
            staticPatches = staticPatches,
            animatedSlices = animatedSlices,
            selectionDecoration = buildSelectionDecoration(layoutEngine),
            preeditDecoration = buildPreeditDecoration(layoutEngine),
            cursorTransition = cursorTransition,
            durationMs = durationMs
        )

        oldRevision = newRevision
        oldLayout = layout

        return result
    }

    private fun planClusterLevelAnimation(
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision,
        oldLayout: android.text.Layout?,
        newLayout: android.text.Layout,
        affectedLines: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>,
        resourceStore: VisualResourceStore,
        snapshotOwner: SnapshotOwner,
        layoutEngine: com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine
    ) {
        val isInsert = visualIntent.isInsert()
        val isDelete = visualIntent.isDelete()
        val isReplace = visualIntent.isReplace() || visualIntent.isCompositionCommit()

        if (isReplace) {
            planClusterReplaceAnimation(
                visualIntent, oldRev, newRev, oldLayout, newLayout,
                affectedLines, animatedSlices, staticPatches, resourceStore, snapshotOwner, layoutEngine
            )
            return
        }

        for (lineIndex in affectedLines) {
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex)
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex)

            if (isInsert && newLineRange != null) {
                val newSnapshot = createSnapshotWithClusters(newLayout, lineIndex, newRev, resourceStore, snapshotOwner, layoutEngine)
                if (newSnapshot != null && newSnapshot.clusters.isNotEmpty()) {
                    addClusterInsertSlices(newSnapshot, visualIntent, animatedSlices)
                    addStaticPatchForAnimatedLine(newSnapshot, staticPatches)
                } else if (newSnapshot != null) {
                    animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.Insert,
                        snapshot = newSnapshot,
                        sourceRect = newSnapshot.sourceRect,
                        destinationRect = android.graphics.RectF(
                            newLineRange.left, newLineRange.top,
                            newLineRange.right, newLineRange.bottom
                        ),
                        startAlpha = 0f,
                        endAlpha = 1f
                    ))
                }
            } else if (isDelete && oldLineRange != null) {
                val layoutForOldSnapshot = oldLayout ?: newLayout
                val oldSnapshot = createSnapshotWithClusters(layoutForOldSnapshot, lineIndex, oldRev, resourceStore, snapshotOwner, layoutEngine)
                if (oldSnapshot != null && oldSnapshot.clusters.isNotEmpty()) {
                    addClusterDeleteSlices(oldSnapshot, visualIntent, animatedSlices)
                } else if (oldSnapshot != null) {
                    animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.Delete,
                        snapshot = oldSnapshot,
                        sourceRect = oldSnapshot.sourceRect,
                        destinationRect = android.graphics.RectF(
                            oldLineRange.left, oldLineRange.top,
                            oldLineRange.right, oldLineRange.bottom
                        ),
                        startAlpha = 1f,
                        endAlpha = 0f
                    ))
                }
            }
        }
        addUnaffectedStaticPatches(newRev, affectedLines, staticPatches)
    }

    private fun planClusterReplaceAnimation(
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision,
        oldLayout: android.text.Layout?,
        newLayout: android.text.Layout,
        affectedLines: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>,
        resourceStore: VisualResourceStore,
        snapshotOwner: SnapshotOwner,
        layoutEngine: com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine
    ) {
        val oldAffectedRanges = visualIntent.oldAffectedByteRanges
        val newAffectedRanges = visualIntent.newAffectedByteRanges

        for (lineIndex in affectedLines) {
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex)
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex)

            val layoutForOldSnapshot = oldLayout ?: newLayout
            val oldSnapshot = if (oldLineRange != null) createSnapshotWithClusters(layoutForOldSnapshot, lineIndex, oldRev, resourceStore, snapshotOwner, layoutEngine) else null
            val newSnapshot = if (newLineRange != null) createSnapshotWithClusters(newLayout, lineIndex, newRev, resourceStore, snapshotOwner, layoutEngine) else null

            val oldClusters = oldSnapshot?.clusters ?: emptyList()
            val newClusters = newSnapshot?.clusters ?: emptyList()

            val oldAffected = oldClusters.filter { c -> oldAffectedRanges.any { r -> c.documentByteStart < r.second && c.documentByteEndExclusive > r.first } }
            val newAffected = newClusters.filter { c -> newAffectedRanges.any { r -> c.documentByteStart < r.second && c.documentByteEndExclusive > r.first } }
            val oldUnaffected = oldClusters.filter { c -> oldAffectedRanges.none { r -> c.documentByteStart < r.second && c.documentByteEndExclusive > r.first } }
            val newUnaffected = newClusters.filter { c -> newAffectedRanges.none { r -> c.documentByteStart < r.second && c.documentByteEndExclusive > r.first } }

            for (cluster in oldAffected) {
                if (oldSnapshot != null) {
                    animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.Delete,
                        snapshot = oldSnapshot,
                        sourceRect = cluster.sourceRectInLineImage,
                        destinationRect = cluster.visualRectInDocument,
                        startAlpha = 1f,
                        endAlpha = 0f
                    ))
                }
            }

            for (cluster in newAffected) {
                if (newSnapshot != null) {
                    animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.Insert,
                        snapshot = newSnapshot,
                        sourceRect = cluster.sourceRectInLineImage,
                        destinationRect = cluster.visualRectInDocument,
                        startAlpha = 0f,
                        endAlpha = 1f
                    ))
                }
            }

            matchUnaffectedClusters(oldUnaffected, newUnaffected, oldSnapshot, newSnapshot, animatedSlices)

            if (newSnapshot != null) {
                addStaticPatchForAnimatedLine(newSnapshot, staticPatches)
            }
        }
        addUnaffectedStaticPatches(newRev, affectedLines, staticPatches)
    }

    private fun matchUnaffectedClusters(
        oldClusters: List<LineClusterSnapshot>,
        newClusters: List<LineClusterSnapshot>,
        oldSnapshot: com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot?,
        newSnapshot: com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot?,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>
    ) {
        val usedNewIndices = mutableSetOf<Int>()

        for (oldCluster in oldClusters) {
            val matchIdx = newClusters.indices.firstOrNull { idx ->
                idx !in usedNewIndices &&
                newClusters[idx].shapingFingerprint == oldCluster.shapingFingerprint &&
                newClusters[idx].documentByteStart == oldCluster.documentByteStart &&
                newClusters[idx].documentByteEndExclusive == oldCluster.documentByteEndExclusive
            }

            if (matchIdx != null) {
                usedNewIndices.add(matchIdx)
                val newCluster = newClusters[matchIdx]
                val geometryChanged = oldCluster.visualRectInDocument != newCluster.visualRectInDocument

                if (geometryChanged && newSnapshot != null) {
                    animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.Move,
                        snapshot = newSnapshot,
                        sourceRect = newCluster.sourceRectInLineImage,
                        destinationRect = newCluster.visualRectInDocument,
                        startAlpha = 1f,
                        endAlpha = 1f,
                        fromDestinationRect = oldCluster.visualRectInDocument
                    ))
                }
            }
        }
    }

    private fun addClusterInsertSlices(
        snapshot: com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot,
        visualIntent: VisualIntent,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>
    ) {
        val newRanges = visualIntent.newAffectedByteRanges
        for (cluster in snapshot.clusters) {
            val isAffected = newRanges.any { r -> cluster.documentByteStart < r.second && cluster.documentByteEndExclusive > r.first }
            if (isAffected) {
                animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                    role = SliceRole.Insert,
                    snapshot = snapshot,
                    sourceRect = cluster.sourceRectInLineImage,
                    destinationRect = cluster.visualRectInDocument,
                    startAlpha = 0f,
                    endAlpha = 1f
                ))
            }
        }
    }

    private fun addClusterDeleteSlices(
        snapshot: com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot,
        visualIntent: VisualIntent,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>
    ) {
        val oldRanges = visualIntent.oldAffectedByteRanges
        for (cluster in snapshot.clusters) {
            val isAffected = oldRanges.any { r -> cluster.documentByteStart < r.second && cluster.documentByteEndExclusive > r.first }
            if (isAffected) {
                animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                    role = SliceRole.Delete,
                    snapshot = snapshot,
                    sourceRect = cluster.sourceRectInLineImage,
                    destinationRect = cluster.visualRectInDocument,
                    startAlpha = 1f,
                    endAlpha = 0f
                ))
            }
        }
    }

    private fun addStaticPatchForAnimatedLine(
        snapshot: com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>
    ) {
        staticPatches.add(PreparedVisualTransaction.StaticPatch(
            newSnapshotId = snapshot.snapshotId,
            lineIndex = snapshot.lineIndex,
            destinationRect = snapshot.destinationRect,
            visibleSourceRects = emptyList()
        ))
    }

    private fun planRunAnimation(
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision,
        oldLayout: android.text.Layout?,
        newLayout: android.text.Layout,
        affectedLines: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>,
        resourceStore: VisualResourceStore,
        snapshotOwner: SnapshotOwner,
        layoutEngine: com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine
    ) {
        for (lineIndex in affectedLines) {
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex)
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex)

            if (oldLineRange != null && newLineRange != null) {
                val layoutForOldSnapshot = oldLayout ?: newLayout
                val oldSnapshot = createSnapshot(layoutForOldSnapshot, lineIndex, oldRev, resourceStore, snapshotOwner)
                val newSnapshot = createSnapshot(newLayout, lineIndex, newRev, resourceStore, snapshotOwner)

                if (oldSnapshot != null) {
                    animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.CrossfadeOld,
                        snapshot = oldSnapshot,
                        sourceRect = oldSnapshot.sourceRect,
                        destinationRect = android.graphics.RectF(
                            oldLineRange.left, oldLineRange.top,
                            oldLineRange.right, oldLineRange.bottom
                        ),
                        startAlpha = 1f,
                        endAlpha = 0f
                    ))
                }
                if (newSnapshot != null) {
                    animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.CrossfadeNew,
                        snapshot = newSnapshot,
                        sourceRect = newSnapshot.sourceRect,
                        destinationRect = android.graphics.RectF(
                            newLineRange.left, newLineRange.top,
                            newLineRange.right, newLineRange.bottom
                        ),
                        startAlpha = 0f,
                        endAlpha = 1f
                    ))
                    addStaticPatchForAnimatedLine(newSnapshot, staticPatches)
                }
            } else if (newLineRange != null) {
                val newSnapshot = createSnapshot(newLayout, lineIndex, newRev, resourceStore, snapshotOwner)
                if (newSnapshot != null) {
                    animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.Insert,
                        snapshot = newSnapshot,
                        sourceRect = newSnapshot.sourceRect,
                        destinationRect = android.graphics.RectF(
                            newLineRange.left, newLineRange.top,
                            newLineRange.right, newLineRange.bottom
                        ),
                        startAlpha = 0f,
                        endAlpha = 1f
                    ))
                    addStaticPatchForAnimatedLine(newSnapshot, staticPatches)
                }
            }
        }
        addUnaffectedStaticPatches(newRev, affectedLines, staticPatches)
    }

    private fun planLineReflowAnimation(
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision,
        oldLayout: android.text.Layout?,
        newLayout: android.text.Layout,
        affectedLines: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>,
        resourceStore: VisualResourceStore,
        snapshotOwner: SnapshotOwner,
        layoutEngine: com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine
    ) {
        for (lineIndex in affectedLines) {
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex)
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex)

            if (oldLineRange != null && newLineRange != null) {
                val newSnapshot = createSnapshot(newLayout, lineIndex, newRev, resourceStore, snapshotOwner)
                if (newSnapshot != null) {
                    animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.Move,
                        snapshot = newSnapshot,
                        sourceRect = newSnapshot.sourceRect,
                        destinationRect = android.graphics.RectF(
                            newLineRange.left, newLineRange.top,
                            newLineRange.right, newLineRange.bottom
                        ),
                        startAlpha = 1f,
                        endAlpha = 1f,
                        fromDestinationRect = android.graphics.RectF(
                            oldLineRange.left, oldLineRange.top,
                            oldLineRange.right, oldLineRange.bottom
                        )
                    ))
                    addStaticPatchForAnimatedLine(newSnapshot, staticPatches)
                }
            } else if (newLineRange != null) {
                val newSnapshot = createSnapshot(newLayout, lineIndex, newRev, resourceStore, snapshotOwner)
                if (newSnapshot != null) {
                    animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.Insert,
                        snapshot = newSnapshot,
                        sourceRect = newSnapshot.sourceRect,
                        destinationRect = android.graphics.RectF(
                            newLineRange.left, newLineRange.top,
                            newLineRange.right, newLineRange.bottom
                        ),
                        startAlpha = 0f,
                        endAlpha = 1f
                    ))
                    addStaticPatchForAnimatedLine(newSnapshot, staticPatches)
                }
            } else if (oldLineRange != null) {
                val layoutForOldSnapshot = oldLayout ?: newLayout
                val oldSnapshot = createSnapshot(layoutForOldSnapshot, lineIndex, oldRev, resourceStore, snapshotOwner)
                if (oldSnapshot != null) {
                    animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.Delete,
                        snapshot = oldSnapshot,
                        sourceRect = oldSnapshot.sourceRect,
                        destinationRect = android.graphics.RectF(
                            oldLineRange.left, oldLineRange.top,
                            oldLineRange.right, oldLineRange.bottom
                        ),
                        startAlpha = 1f,
                        endAlpha = 0f
                    ))
                }
            }
        }
        addUnaffectedStaticPatches(newRev, affectedLines, staticPatches)
    }

    private fun planCrossfadeAnimation(
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision,
        oldLayout: android.text.Layout?,
        newLayout: android.text.Layout,
        affectedLines: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>,
        resourceStore: VisualResourceStore,
        snapshotOwner: SnapshotOwner,
        layoutEngine: com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine
    ) {
        for (lineIndex in affectedLines) {
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex)
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex)

            if (oldLineRange != null && newLineRange != null) {
                val layoutForOldSnapshot = oldLayout ?: newLayout
                val oldSnapshot = createSnapshot(layoutForOldSnapshot, lineIndex, oldRev, resourceStore, snapshotOwner)
                if (oldSnapshot != null) {
                    animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.CrossfadeOld,
                        snapshot = oldSnapshot,
                        sourceRect = oldSnapshot.sourceRect,
                        destinationRect = android.graphics.RectF(
                            oldLineRange.left, oldLineRange.top,
                            oldLineRange.right, oldLineRange.bottom
                        ),
                        startAlpha = 1f,
                        endAlpha = 0f
                    ))
                }
            } else if (newLineRange != null) {
                val newSnapshot = createSnapshot(newLayout, lineIndex, newRev, resourceStore, snapshotOwner)
                if (newSnapshot != null) {
                    animatedSlices.add(PreparedVisualTransaction.AnimatedSlice(
                        role = SliceRole.Insert,
                        snapshot = newSnapshot,
                        sourceRect = newSnapshot.sourceRect,
                        destinationRect = android.graphics.RectF(
                            newLineRange.left, newLineRange.top,
                            newLineRange.right, newLineRange.bottom
                        ),
                        startAlpha = 0f,
                        endAlpha = 1f
                    ))
                    addStaticPatchForAnimatedLine(newSnapshot, staticPatches)
                }
            }
        }
        addUnaffectedStaticPatches(newRev, affectedLines, staticPatches)
    }

    private fun planNoAnimation(
        newRev: AndroidLayoutRevision,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>
    ) {
        for (i in 0 until newRev.lineCount) {
            val newLineRange = newRev.lineRanges.getOrNull(i) ?: continue
            staticPatches.add(PreparedVisualTransaction.StaticPatch(
                newSnapshotId = System.nanoTime() + i,
                lineIndex = i,
                destinationRect = android.graphics.RectF(
                    newLineRange.left, newLineRange.top,
                    newLineRange.right, newLineRange.bottom
                ),
                visibleSourceRects = emptyList()
            ))
        }
    }

    private fun addUnaffectedStaticPatches(
        newRev: AndroidLayoutRevision,
        affectedLines: Set<Int>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>
    ) {
        for (i in 0 until newRev.lineCount) {
            if (i !in affectedLines) {
                val newLineRange = newRev.lineRanges.getOrNull(i) ?: continue
                staticPatches.add(PreparedVisualTransaction.StaticPatch(
                    newSnapshotId = System.nanoTime() + i,
                    lineIndex = i,
                    destinationRect = android.graphics.RectF(
                        newLineRange.left, newLineRange.top,
                        newLineRange.right, newLineRange.bottom
                    ),
                    visibleSourceRects = emptyList()
                ))
            }
        }
    }

    private fun computeAffectedLines(
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision
    ): Set<Int> {
        val affectedLines = mutableSetOf<Int>()

        for ((start, end) in visualIntent.oldAffectedByteRanges) {
            for (i in oldRev.lineRanges.indices) {
                val lineRange = oldRev.lineRanges[i]
                if (start < lineRange.endUtf8 && end > lineRange.startUtf8) {
                    affectedLines.add(i)
                }
            }
        }

        for ((start, end) in visualIntent.newAffectedByteRanges) {
            for (i in newRev.lineRanges.indices) {
                val lineRange = newRev.lineRanges[i]
                if (start < lineRange.endUtf8 && end > lineRange.startUtf8) {
                    affectedLines.add(i)
                }
            }
        }

        return affectedLines
    }

    private fun computeStableSuffixEndLine(
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision,
        affectedLines: Set<Int>,
        oldText: String,
        newText: String
    ): Int {
        val maxCheckLines = minOf(oldRev.lineCount, newRev.lineCount)
        val maxAffected = affectedLines.maxOrNull() ?: -1
        var stableStart = maxAffected + 1
        if (stableStart >= maxCheckLines) return maxCheckLines

        var consecutiveStable = 0
        for (i in stableStart until maxCheckLines) {
            val oldRange = oldRev.lineRanges.getOrNull(i)
            val newRange = newRev.lineRanges.getOrNull(i)
            if (oldRange == null || newRange == null) break

            val oldLineText = if (oldRange.startUtf8 <= oldRange.endUtf8 && oldRange.endUtf8 <= oldText.length) {
                oldText.substring(oldRange.startUtf8, oldRange.endUtf8)
            } else ""
            val newLineText = if (newRange.startUtf8 <= newRange.endUtf8 && newRange.endUtf8 <= newText.length) {
                newText.substring(newRange.startUtf8, newRange.endUtf8)
            } else ""

            val textStable = oldLineText == newLineText
            val boundaryStable = oldRange.startUtf8 == newRange.startUtf8 &&
                                 oldRange.endUtf8 == newRange.endUtf8 &&
                                 oldRange.startUtf16 == newRange.startUtf16 &&
                                 oldRange.endUtf16 == newRange.endUtf16

            if (textStable && boundaryStable) {
                consecutiveStable++
                if (consecutiveStable >= 3) return i + 1
            } else {
                consecutiveStable = 0
            }
        }

        return maxCheckLines
    }

    private fun parseAnimationMode(mode: String): AnimationMode {
        return when (mode) {
            "GlyphAnimation" -> AnimationMode.GlyphAnimation
            "ClusterAnimation" -> AnimationMode.ClusterAnimation
            "RunAnimation" -> AnimationMode.RunAnimation
            "LineReflowAnimation" -> AnimationMode.LineReflowAnimation
            "SystemSuppressed" -> AnimationMode.SystemSuppressed
            else -> AnimationMode.SystemSuppressed
        }
    }

    fun resetOldRevision() {
        oldRevision = null
        oldLayout = null
    }

    private fun buildSelectionDecoration(
        layoutEngine: com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine
    ): PreparedVisualTransaction.SelectionDecoration? {
        val mirror = layoutEngine.getMirror()
        val selStart = mirror.getSelectionStartUtf16()
        val selEnd = mirror.getSelectionEndUtf16()
        if (selStart == selEnd) return null

        val layout = layoutEngine.getLayout() ?: return null
        val startLine = layout.getLineForOffset(selStart)
        val endLine = layout.getLineForOffset(selEnd)
        val rects = mutableListOf<android.graphics.RectF>()
        for (line in startLine..endLine) {
            val lineStart = if (line == startLine) selStart else layout.getLineStart(line)
            val lineEnd = if (line == endLine) selEnd else layout.getLineEnd(line)
            val left = layout.getPrimaryHorizontal(lineStart)
            val right = layout.getPrimaryHorizontal(lineEnd - 1) + layout.getLineWidth(line)
            val top = layout.getLineTop(line).toFloat()
            val bottom = layout.getLineBottom(line).toFloat()
            rects.add(android.graphics.RectF(
                layout.getLineLeft(line), top, layout.getLineRight(line), bottom
            ))
        }
        return PreparedVisualTransaction.SelectionDecoration(selStart, selEnd, rects)
    }

    private fun buildPreeditDecoration(
        layoutEngine: com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine
    ): PreparedVisualTransaction.PreeditDecoration? {
        val mirror = layoutEngine.getMirror()
        val compRange = mirror.getCompositionRangeUtf16() ?: return null
        return PreparedVisualTransaction.PreeditDecoration(
            startUtf16 = compRange.first,
            endUtf16 = compRange.second,
            underlineColor = android.graphics.Color.BLACK
        )
    }

    private fun createSnapshot(
        layout: android.text.Layout,
        lineIndex: Int,
        revision: AndroidLayoutRevision,
        resourceStore: VisualResourceStore,
        owner: SnapshotOwner = SnapshotOwner.OwnedByTransaction(System.nanoTime())
    ): com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot? {
        val snapshot = snapshotBuilder.buildSnapshotForLine(layout, lineIndex, revision) ?: return null
        resourceStore.put(snapshot, owner)
        return snapshot
    }

    private fun createSnapshotWithClusters(
        layout: android.text.Layout,
        lineIndex: Int,
        revision: AndroidLayoutRevision,
        resourceStore: VisualResourceStore,
        owner: SnapshotOwner,
        layoutEngine: com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine
    ): com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot? {
        val mirror = layoutEngine.getMirror()
        val snapshot = snapshotBuilder.buildSnapshotForLineWithClusters(layout, lineIndex, revision, mirror) ?: return null
        resourceStore.put(snapshot, owner)
        return snapshot
    }

    private enum class AnimationMode {
        GlyphAnimation, ClusterAnimation, RunAnimation, LineReflowAnimation, SystemSuppressed
    }
}
