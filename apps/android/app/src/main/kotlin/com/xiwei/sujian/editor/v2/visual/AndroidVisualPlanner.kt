package com.xiwei.sujian.editor.v2.visual

import com.xiwei.sujian.editor.v2.mirror.VisualIntent
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutRevision
import com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshotBuilder
import com.xiwei.sujian.editor.v2.layout.LineClusterSnapshot
import com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot
import uniffi.writer_core.AnimationModeDto

class AndroidVisualPlanner(
) {
    private val snapshotBuilder = AndroidLineSnapshotBuilder()

    fun computeAffectedLineIndices(
        visualIntent: VisualIntent,
        revision: AndroidLayoutRevision?,
        useNewRanges: Boolean = false
    ): Set<Int> {
        if (revision == null) return emptySet()
        val affectedLines = mutableSetOf<Int>()
        val ranges = if (useNewRanges) visualIntent.newAffectedByteRanges else visualIntent.oldAffectedByteRanges
        for ((start, end) in ranges) {
            for (i in revision.lineRanges.indices) {
                val lineRange = revision.lineRanges[i]
                if (start < lineRange.endUtf8 && end > lineRange.startUtf8) {
                    affectedLines.add(i)
                }
            }
        }
        return affectedLines
    }

    fun prepare(
        visualIntent: VisualIntent,
        oldRevision: AndroidLayoutRevision?,
        newRevision: AndroidLayoutRevision?,
        resourceStore: VisualResourceStore,
        preCapturedOldSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        preCapturedNewSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap()
    ): PreparedVisualTransaction {
        val durationMs = visualIntent.durationMs
        val transactionKey = System.nanoTime()

        val staticPatches = mutableListOf<PreparedVisualTransaction.StaticPatch>()
        val animatedSlices = mutableListOf<PreparedVisualTransaction.AnimatedSlice>()
        var cursorTransition: PreparedVisualTransaction.CursorTransition? = null

        val oldRev = oldRevision
        val newRev = newRevision
        val snapshotOwner = SnapshotOwner.OwnedByTransaction(transactionKey)

        if (oldRev != null && newRev != null) {
            val affectedLines = computeAffectedLines(visualIntent, oldRev, newRev)
            val mode = parseAnimationMode(visualIntent.animationMode)

            when (mode) {
                AnimationMode.GlyphAnimation, AnimationMode.ClusterAnimation -> {
                    planClusterLevelAnimation(
                        visualIntent, oldRev, newRev,
                        affectedLines, animatedSlices, staticPatches, resourceStore, snapshotOwner, preCapturedOldSnapshots, preCapturedNewSnapshots
                    )
                }
                AnimationMode.RunAnimation -> {
                    planRunAnimation(
                        visualIntent, oldRev, newRev,
                        affectedLines, animatedSlices, staticPatches, resourceStore, snapshotOwner, preCapturedOldSnapshots, preCapturedNewSnapshots
                    )
                }
                AnimationMode.LineReflowAnimation -> {
                    planLineReflowAnimation(
                        visualIntent, oldRev, newRev,
                        affectedLines, animatedSlices, staticPatches, resourceStore, snapshotOwner, preCapturedOldSnapshots, preCapturedNewSnapshots
                    )
                }
                AnimationMode.SnapshotAnimation -> {
                    planCrossfadeAnimation(
                        visualIntent, oldRev, newRev,
                        affectedLines, animatedSlices, staticPatches, resourceStore, snapshotOwner, preCapturedOldSnapshots, preCapturedNewSnapshots
                    )
                }
                AnimationMode.SystemSuppressed -> {
                    planNoAnimation(newRev, staticPatches)
                }
            }
        } else if (newRev != null) {
            planNoAnimation(newRev, staticPatches)
        }

        if (visualIntent.coordinatedCursor.shouldAnimate && newRev != null) {
            val fromX = if (oldRev != null) oldRev.cursorX else newRev.cursorX
            val fromY = if (oldRev != null) oldRev.cursorY else newRev.cursorY
            val fromHeight = if (oldRev != null) oldRev.cursorHeight else newRev.cursorHeight

            cursorTransition = PreparedVisualTransaction.CursorTransition(
                fromX = fromX,
                fromY = fromY,
                fromHeight = fromHeight,
                toX = newRev.cursorX,
                toY = newRev.cursorY,
                toHeight = newRev.cursorHeight,
                shouldAnimate = true
            )
        }

        return PreparedVisualTransaction(
            transactionId = transactionKey,
            oldRevision = oldRev,
            newRevision = newRev,
            staticPatches = staticPatches,
            animatedSlices = animatedSlices,
            selectionDecoration = buildSelectionDecoration(newRev),
            preeditDecoration = buildPreeditDecoration(newRev),
            cursorTransition = cursorTransition,
            durationMs = durationMs
        )
    }

    private fun planClusterLevelAnimation(
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision,
        affectedLines: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>,
        resourceStore: VisualResourceStore,
        snapshotOwner: SnapshotOwner,
        preCapturedOldSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        preCapturedNewSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap()
    ) {
        val isInsert = visualIntent.isInsert()
        val isDelete = visualIntent.isDelete()
        val isReplace = visualIntent.isReplace() || visualIntent.isCompositionCommit()

        if (isReplace) {
            planClusterReplaceAnimation(
                visualIntent, oldRev, newRev,
                affectedLines, animatedSlices, staticPatches, resourceStore, snapshotOwner, preCapturedOldSnapshots, preCapturedNewSnapshots
            )
            return
        }

        for (lineIndex in affectedLines) {
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex)
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex)

            if (isInsert && newLineRange != null) {
                val newSnapshot = createSnapshotFromRevision(newRev, lineIndex, resourceStore, snapshotOwner, preCapturedOldSnapshots, preCapturedNewSnapshots, isNewRevision = true)
                if (newSnapshot != null) {
                    val newClusters = newSnapshot.clusters
                    if (newClusters.isNotEmpty() && visualIntent.newAffectedByteRanges.isNotEmpty()) {
                        for (cluster in newClusters) {
                            val inRange = visualIntent.newAffectedByteRanges.any { (s, e) ->
                                cluster.documentByteStart >= s && cluster.documentByteEndExclusive <= e
                            }
                            if (inRange) {
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
                    } else {
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
                }
            } else if (isDelete && oldLineRange != null) {
                val oldSnapshot = createSnapshotFromRevision(oldRev, lineIndex, resourceStore, snapshotOwner, preCapturedOldSnapshots)
                if (oldSnapshot != null) {
                    val oldClusters = oldSnapshot.clusters
                    if (oldClusters.isNotEmpty() && visualIntent.oldAffectedByteRanges.isNotEmpty()) {
                        for (cluster in oldClusters) {
                            val inRange = visualIntent.oldAffectedByteRanges.any { (s, e) ->
                                cluster.documentByteStart >= s && cluster.documentByteEndExclusive <= e
                            }
                            if (inRange) {
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
                    } else {
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
        }
        addUnaffectedStaticPatches(newRev, affectedLines, staticPatches)
    }

    private fun planClusterReplaceAnimation(
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision,
        affectedLines: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>,
        resourceStore: VisualResourceStore,
        snapshotOwner: SnapshotOwner,
        preCapturedOldSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        preCapturedNewSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap()
    ) {
        val offsetMap = buildOffsetMap(visualIntent, oldRev, newRev)

        for (lineIndex in affectedLines) {
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex)
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex)

            val oldSnapshot = if (oldLineRange != null) createSnapshotFromRevision(oldRev, lineIndex, resourceStore, snapshotOwner, preCapturedOldSnapshots, preCapturedNewSnapshots) else null
            val newSnapshot = if (newLineRange != null) createSnapshotFromRevision(newRev, lineIndex, resourceStore, snapshotOwner, preCapturedOldSnapshots, preCapturedNewSnapshots, isNewRevision = true) else null

            if (oldSnapshot != null && oldLineRange != null) {
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

            if (newSnapshot != null && newLineRange != null) {
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
        addUnaffectedStaticPatches(newRev, affectedLines, staticPatches)
    }

    private fun buildOffsetMap(
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision
    ): Map<Int, Int> {
        val offsetMap = mutableMapOf<Int, Int>()
        val oldRanges = visualIntent.oldAffectedByteRanges
        val newRanges = visualIntent.newAffectedByteRanges

        if (oldRanges.isEmpty() || newRanges.isEmpty()) return offsetMap

        val oldAffectedEnd = oldRanges.last().second
        val newAffectedEnd = newRanges.last().second
        val shift = newAffectedEnd - oldAffectedEnd

        for (lineRange in oldRev.lineRanges) {
            if (lineRange.startUtf8 >= oldAffectedEnd) {
                offsetMap[lineRange.startUtf8] = lineRange.startUtf8 + shift
                offsetMap[lineRange.endUtf8] = lineRange.endUtf8 + shift
            } else if (lineRange.endUtf8 <= oldRanges.first().first) {
                offsetMap[lineRange.startUtf8] = lineRange.startUtf8
                offsetMap[lineRange.endUtf8] = lineRange.endUtf8
            } else {
                val overlapStart = lineRange.startUtf8.coerceAtLeast(oldRanges.first().first)
                val overlapEnd = lineRange.endUtf8.coerceAtMost(oldAffectedEnd)
                val newOverlapStart = mapThroughRanges(overlapStart, oldRanges, newRanges)
                val newOverlapEnd = mapThroughRanges(overlapEnd, oldRanges, newRanges)
                if (newOverlapStart >= 0) offsetMap[overlapStart] = newOverlapStart
                if (newOverlapEnd >= 0) offsetMap[overlapEnd] = newOverlapEnd
            }
        }
        return offsetMap
    }

    private fun mapThroughRanges(offset: Int, oldRanges: List<Pair<Int, Int>>, newRanges: List<Pair<Int, Int>>): Int {
        for (i in oldRanges.indices) {
            val (oldStart, oldEnd) = oldRanges[i]
            if (offset in oldStart..oldEnd) {
                val newRange = newRanges.getOrNull(i) ?: continue
                val ratio = if (oldEnd == oldStart) 0f else (offset - oldStart).toFloat() / (oldEnd - oldStart)
                return newRange.first + (ratio * (newRange.second - newRange.first)).toInt()
            }
        }
        return -1
    }

    private fun planRunAnimation(
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision,
        affectedLines: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>,
        resourceStore: VisualResourceStore,
        snapshotOwner: SnapshotOwner,
        preCapturedOldSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        preCapturedNewSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap()
    ) {
        for (lineIndex in affectedLines) {
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex)
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex)

            if (oldLineRange != null && newLineRange != null) {
                val oldSnapshot = createSnapshotFromRevision(oldRev, lineIndex, resourceStore, snapshotOwner, preCapturedOldSnapshots, preCapturedNewSnapshots)
                val newSnapshot = createSnapshotFromRevision(newRev, lineIndex, resourceStore, snapshotOwner, preCapturedOldSnapshots, preCapturedNewSnapshots, isNewRevision = true)
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
                val newSnapshot = createSnapshotFromRevision(newRev, lineIndex, resourceStore, snapshotOwner, preCapturedOldSnapshots, preCapturedNewSnapshots, isNewRevision = true)
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
        affectedLines: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>,
        resourceStore: VisualResourceStore,
        snapshotOwner: SnapshotOwner,
        preCapturedOldSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        preCapturedNewSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap()
    ) {
        for (lineIndex in affectedLines) {
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex)
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex)

            if (oldLineRange != null && newLineRange != null) {
                val newSnapshot = createSnapshotFromRevision(newRev, lineIndex, resourceStore, snapshotOwner, preCapturedOldSnapshots, preCapturedNewSnapshots, isNewRevision = true)
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
                val newSnapshot = createSnapshotFromRevision(newRev, lineIndex, resourceStore, snapshotOwner, preCapturedOldSnapshots, preCapturedNewSnapshots, isNewRevision = true)
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
                val oldSnapshot = createSnapshotFromRevision(oldRev, lineIndex, resourceStore, snapshotOwner, preCapturedOldSnapshots, preCapturedNewSnapshots)
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
        affectedLines: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>,
        resourceStore: VisualResourceStore,
        snapshotOwner: SnapshotOwner,
        preCapturedOldSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        preCapturedNewSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap()
    ) {
        for (lineIndex in affectedLines) {
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex)
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex)

            if (oldLineRange != null && newLineRange != null) {
                val oldSnapshot = createSnapshotFromRevision(oldRev, lineIndex, resourceStore, snapshotOwner, preCapturedOldSnapshots, preCapturedNewSnapshots)
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
                val newSnapshot = createSnapshotFromRevision(newRev, lineIndex, resourceStore, snapshotOwner, preCapturedOldSnapshots, preCapturedNewSnapshots, isNewRevision = true)
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

    private fun findLineForUtf8(rev: AndroidLayoutRevision, byteOffset: Int): Int {
        for (i in rev.lineRanges.indices) {
            val range = rev.lineRanges[i]
            if (byteOffset <= range.endUtf8) return i
        }
        return rev.lineRanges.lastIndex.coerceAtLeast(0)
    }

    private fun parseAnimationMode(mode: AnimationModeDto): AnimationMode {
        return when (mode) {
            AnimationModeDto.GLYPH_ANIMATION -> AnimationMode.GlyphAnimation
            AnimationModeDto.CLUSTER_ANIMATION -> AnimationMode.ClusterAnimation
            AnimationModeDto.RUN_ANIMATION -> AnimationMode.RunAnimation
            AnimationModeDto.LINE_REFLOW_ANIMATION -> AnimationMode.LineReflowAnimation
            AnimationModeDto.SNAPSHOT_ANIMATION -> AnimationMode.SnapshotAnimation
            AnimationModeDto.SYSTEM_SUPPRESSED -> AnimationMode.SystemSuppressed
        }
    }

    fun resetOldRevision() {
    }

    private fun createSnapshotFromRevision(
        revision: AndroidLayoutRevision,
        lineIndex: Int,
        resourceStore: VisualResourceStore,
        owner: SnapshotOwner,
        preCapturedOldSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        preCapturedNewSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        isNewRevision: Boolean = false
    ): com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot? {
        if (isNewRevision) {
            val preCapturedNew = preCapturedNewSnapshots[lineIndex]
            if (preCapturedNew != null) {
                resourceStore.put(preCapturedNew, owner)
                return preCapturedNew
            }
        }
        val preCaptured = preCapturedOldSnapshots[lineIndex]
        if (preCaptured != null) {
            resourceStore.put(preCaptured, owner)
            return preCaptured
        }
        return null
    }

    private fun buildSelectionDecoration(
        newRev: AndroidLayoutRevision?
    ): PreparedVisualTransaction.SelectionDecoration? {
        if (newRev == null) return null
        val selStart = newRev.selectionStartUtf16
        val selEnd = newRev.selectionEndUtf16
        if (selStart == selEnd) return null

        val rects = mutableListOf<android.graphics.RectF>()
        for (lineRange in newRev.lineRanges) {
            if (selStart < lineRange.endUtf16 && selEnd > lineRange.startUtf16) {
                rects.add(android.graphics.RectF(
                    lineRange.left, lineRange.top, lineRange.right, lineRange.bottom
                ))
            }
        }
        if (rects.isEmpty()) return null
        return PreparedVisualTransaction.SelectionDecoration(selStart, selEnd, rects)
    }

    private fun buildPreeditDecoration(newRev: AndroidLayoutRevision?): PreparedVisualTransaction.PreeditDecoration? {
        if (newRev == null) return null
        val compStart = newRev.compositionStartUtf16
        val compEnd = newRev.compositionEndUtf16
        if (compStart < 0 || compEnd < 0 || compStart >= compEnd) return null
        return PreparedVisualTransaction.PreeditDecoration(
            startUtf16 = compStart,
            endUtf16 = compEnd,
            underlineColor = android.graphics.Color.BLACK
        )
    }

    private enum class AnimationMode {
        GlyphAnimation, ClusterAnimation, RunAnimation, LineReflowAnimation, SnapshotAnimation, SystemSuppressed
    }
}
