package com.xiwei.sujian.editor.v2.visual

import com.xiwei.sujian.editor.v2.mirror.VisualIntent
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutRevision
import com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshotBuilder
import com.xiwei.sujian.editor.v2.layout.LineClusterSnapshot
import com.xiwei.sujian.editor.v2.input.AndroidTextIndexMap

class AndroidVisualPlanner(
    private val mirror: DisplayTextMirror
) {
    private val snapshotBuilder = AndroidLineSnapshotBuilder()

    fun prepare(
        visualIntent: VisualIntent,
        oldRevision: AndroidLayoutRevision?,
        newRevision: AndroidLayoutRevision?,
        resourceStore: VisualResourceStore
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
                        affectedLines, animatedSlices, staticPatches, resourceStore, snapshotOwner
                    )
                }
                AnimationMode.RunAnimation -> {
                    planRunAnimation(
                        visualIntent, oldRev, newRev,
                        affectedLines, animatedSlices, staticPatches, resourceStore, snapshotOwner
                    )
                }
                AnimationMode.LineReflowAnimation -> {
                    planLineReflowAnimation(
                        visualIntent, oldRev, newRev,
                        affectedLines, animatedSlices, staticPatches, resourceStore, snapshotOwner
                    )
                }
                AnimationMode.SystemSuppressed -> {
                    planNoAnimation(newRev, staticPatches)
                }
                else -> {
                    planCrossfadeAnimation(
                        visualIntent, oldRev, newRev,
                        affectedLines, animatedSlices, staticPatches, resourceStore, snapshotOwner
                    )
                }
            }
        } else if (newRev != null) {
            planNoAnimation(newRev, staticPatches)
        }

        if (visualIntent.coordinatedCursor.shouldAnimate && newRev != null) {
            val cursorByteOffset = visualIntent.coordinatedCursor.newByteOffset
            val cursorLine = findLineForUtf8(newRev, cursorByteOffset)
            val lineRange = newRev.lineRanges.getOrNull(cursorLine)

            val fromX = if (oldRev != null) {
                val oldCursorLine = findLineForUtf8(oldRev, visualIntent.coordinatedCursor.oldByteOffset)
                oldRev.lineRanges.getOrNull(oldCursorLine)?.left ?: 0f
            } else lineRange?.left ?: 0f

            cursorTransition = PreparedVisualTransaction.CursorTransition(
                fromX = fromX,
                fromY = lineRange?.top ?: 0f,
                fromHeight = (lineRange?.bottom ?: 0f) - (lineRange?.top ?: 0f),
                toX = lineRange?.left ?: 0f,
                toY = lineRange?.top ?: 0f,
                toHeight = (lineRange?.bottom ?: 0f) - (lineRange?.top ?: 0f),
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
            preeditDecoration = buildPreeditDecoration(),
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
        snapshotOwner: SnapshotOwner
    ) {
        val isInsert = visualIntent.isInsert()
        val isDelete = visualIntent.isDelete()
        val isReplace = visualIntent.isReplace() || visualIntent.isCompositionCommit()

        if (isReplace) {
            planClusterReplaceAnimation(
                visualIntent, oldRev, newRev,
                affectedLines, animatedSlices, staticPatches, resourceStore, snapshotOwner
            )
            return
        }

        for (lineIndex in affectedLines) {
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex)
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex)

            if (isInsert && newLineRange != null) {
                val newSnapshot = createSnapshotFromRevision(newRev, lineIndex, resourceStore, snapshotOwner)
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
                }
            } else if (isDelete && oldLineRange != null) {
                val oldSnapshot = createSnapshotFromRevision(oldRev, lineIndex, resourceStore, snapshotOwner)
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

    private fun planClusterReplaceAnimation(
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision,
        affectedLines: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>,
        resourceStore: VisualResourceStore,
        snapshotOwner: SnapshotOwner
    ) {
        val offsetMap = buildOffsetMap(visualIntent, oldRev, newRev)

        for (lineIndex in affectedLines) {
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex)
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex)

            val oldSnapshot = if (oldLineRange != null) createSnapshotFromRevision(oldRev, lineIndex, resourceStore, snapshotOwner) else null
            val newSnapshot = if (newLineRange != null) createSnapshotFromRevision(newRev, lineIndex, resourceStore, snapshotOwner) else null

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

        val oldEnd = oldRanges.last().second
        val newEnd = newRanges.last().second
        val shift = newEnd - oldEnd

        for (lineRange in oldRev.lineRanges) {
            if (lineRange.startUtf8 >= oldEnd) {
                offsetMap[lineRange.startUtf8] = lineRange.startUtf8 + shift
                offsetMap[lineRange.endUtf8] = lineRange.endUtf8 + shift
            } else {
                offsetMap[lineRange.startUtf8] = lineRange.startUtf8
                offsetMap[lineRange.endUtf8] = lineRange.endUtf8
            }
        }
        return offsetMap
    }

    private fun planRunAnimation(
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision,
        affectedLines: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>,
        resourceStore: VisualResourceStore,
        snapshotOwner: SnapshotOwner
    ) {
        for (lineIndex in affectedLines) {
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex)
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex)

            if (oldLineRange != null && newLineRange != null) {
                val oldSnapshot = createSnapshotFromRevision(oldRev, lineIndex, resourceStore, snapshotOwner)
                val newSnapshot = createSnapshotFromRevision(newRev, lineIndex, resourceStore, snapshotOwner)

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
                val newSnapshot = createSnapshotFromRevision(newRev, lineIndex, resourceStore, snapshotOwner)
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
        snapshotOwner: SnapshotOwner
    ) {
        for (lineIndex in affectedLines) {
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex)
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex)

            if (oldLineRange != null && newLineRange != null) {
                val newSnapshot = createSnapshotFromRevision(newRev, lineIndex, resourceStore, snapshotOwner)
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
                val newSnapshot = createSnapshotFromRevision(newRev, lineIndex, resourceStore, snapshotOwner)
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
                val oldSnapshot = createSnapshotFromRevision(oldRev, lineIndex, resourceStore, snapshotOwner)
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
        snapshotOwner: SnapshotOwner
    ) {
        for (lineIndex in affectedLines) {
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex)
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex)

            if (oldLineRange != null && newLineRange != null) {
                val oldSnapshot = createSnapshotFromRevision(oldRev, lineIndex, resourceStore, snapshotOwner)
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
                val newSnapshot = createSnapshotFromRevision(newRev, lineIndex, resourceStore, snapshotOwner)
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
    }

    private fun createSnapshotFromRevision(
        revision: AndroidLayoutRevision,
        lineIndex: Int,
        resourceStore: VisualResourceStore,
        owner: SnapshotOwner
    ): com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot? {
        val lineRange = revision.lineRanges.getOrNull(lineIndex) ?: return null
        val snapshot = snapshotBuilder.buildSnapshotFromRevision(lineRange, mirror) ?: return null
        resourceStore.put(snapshot, owner)
        return snapshot
    }

    private fun buildSelectionDecoration(
        newRev: AndroidLayoutRevision?
    ): PreparedVisualTransaction.SelectionDecoration? {
        val selStart = mirror.getSelectionStartUtf16()
        val selEnd = mirror.getSelectionEndUtf16()
        if (selStart == selEnd || newRev == null) return null

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

    private fun buildPreeditDecoration(): PreparedVisualTransaction.PreeditDecoration? {
        val compRange = mirror.getCompositionRangeUtf16() ?: return null
        return PreparedVisualTransaction.PreeditDecoration(
            startUtf16 = compRange.first,
            endUtf16 = compRange.second,
            underlineColor = android.graphics.Color.BLACK
        )
    }

    private enum class AnimationMode {
        GlyphAnimation, ClusterAnimation, RunAnimation, LineReflowAnimation, SystemSuppressed
    }
}
