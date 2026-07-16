package com.xiwei.sujian.editor.v2.visual

import com.xiwei.sujian.editor.v2.mirror.VisualIntent
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutRevision
import com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshotBuilder

class AndroidVisualPlanner {
    private var oldRevision: AndroidLayoutRevision? = null
    private val snapshotBuilder = AndroidLineSnapshotBuilder()

    fun prepare(
        visualIntent: VisualIntent,
        layoutEngine: com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine,
        resourceStore: VisualResourceStore
    ): PreparedVisualTransaction {
        val newRevision = layoutEngine.getCurrentRevision()
        val layout = layoutEngine.getLayout()
        val durationMs = visualIntent.durationMs

        val staticPatches = mutableListOf<PreparedVisualTransaction.StaticPatch>()
        val animatedSlices = mutableListOf<PreparedVisualTransaction.AnimatedSlice>()
        var cursorTransition: PreparedVisualTransaction.CursorTransition? = null

        val oldRev = oldRevision
        val newRev = newRevision

        if (oldRev != null && newRev != null && layout != null) {
            val affectedLines = computeAffectedLines(visualIntent, oldRev, newRev)
            val mode = parseAnimationMode(visualIntent.animationMode)

            when (mode) {
                AnimationMode.GlyphAnimation, AnimationMode.ClusterAnimation -> {
                    planGlyphOrClusterAnimation(
                        visualIntent, oldRev, newRev, layout,
                        affectedLines, animatedSlices, staticPatches
                    )
                }
                AnimationMode.RunAnimation -> {
                    planRunAnimation(
                        visualIntent, oldRev, newRev, layout,
                        affectedLines, animatedSlices, staticPatches
                    )
                }
                AnimationMode.LineReflowAnimation -> {
                    planLineReflowAnimation(
                        visualIntent, oldRev, newRev, layout,
                        affectedLines, animatedSlices, staticPatches
                    )
                }
                AnimationMode.SystemSuppressed -> {
                    planNoAnimation(newRev, staticPatches)
                }
                else -> {
                    planCrossfadeAnimation(
                        visualIntent, oldRev, newRev, layout,
                        affectedLines, animatedSlices, staticPatches
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
            transactionId = System.nanoTime(),
            oldRevision = oldRev,
            newRevision = newRev,
            staticPatches = staticPatches,
            animatedSlices = animatedSlices,
            cursorTransition = cursorTransition,
            durationMs = durationMs
        )

        oldRevision = newRevision

        return result
    }

    private fun planGlyphOrClusterAnimation(
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision,
        layout: android.text.Layout,
        affectedLines: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>
    ) {
        val isInsert = visualIntent.isInsert()
        for (lineIndex in affectedLines) {
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex)
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex)

            if (isInsert && newLineRange != null) {
                val newSnapshot = createSnapshot(layout, lineIndex, newRev, resourceStore)
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
            } else if (!isInsert && oldLineRange != null) {
                val oldSnapshot = createSnapshot(layout, lineIndex, oldRev, resourceStore)
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

    private fun planRunAnimation(
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision,
        layout: android.text.Layout,
        affectedLines: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>
    ) {
        for (lineIndex in affectedLines) {
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex)
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex)

            if (oldLineRange != null && newLineRange != null) {
                val oldSnapshot = createSnapshot(layout, lineIndex, oldRev, resourceStore)
                val newSnapshot = createSnapshot(layout, lineIndex, newRev, resourceStore)

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
                }
            } else if (newLineRange != null) {
                val newSnapshot = createSnapshot(layout, lineIndex, newRev, resourceStore)
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
            }
        }
        addUnaffectedStaticPatches(newRev, affectedLines, staticPatches)
    }

    private fun planLineReflowAnimation(
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision,
        layout: android.text.Layout,
        affectedLines: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>
    ) {
        for (lineIndex in affectedLines) {
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex)
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex)

            if (oldLineRange != null && newLineRange != null) {
                val newSnapshot = createSnapshot(layout, lineIndex, newRev, resourceStore)
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
                }
            } else if (newLineRange != null) {
                val newSnapshot = createSnapshot(layout, lineIndex, newRev, resourceStore)
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
            } else if (oldLineRange != null) {
                val oldSnapshot = createSnapshot(layout, lineIndex, oldRev, resourceStore)
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
        layout: android.text.Layout,
        affectedLines: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>
    ) {
        for (lineIndex in affectedLines) {
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex)
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex)

            if (oldLineRange != null && newLineRange != null) {
                val oldSnapshot = createSnapshot(layout, lineIndex, oldRev, resourceStore)
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
                val newSnapshot = createSnapshot(layout, lineIndex, newRev, resourceStore)
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

    private fun parseAnimationMode(mode: String): AnimationMode {
        return when (mode) {
            "GlyphAnimation" -> AnimationMode.GlyphAnimation
            "ClusterAnimation" -> AnimationMode.ClusterAnimation
            "RunAnimation" -> AnimationMode.RunAnimation
            "LineReflowAnimation" -> AnimationMode.LineReflowAnimation
            "SystemSuppressed" -> AnimationMode.SystemSuppressed
            else -> AnimationMode.ClusterAnimation
        }
    }

    fun resetOldRevision() {
        oldRevision = null
    }

    private fun createSnapshot(
        layout: android.text.Layout,
        lineIndex: Int,
        revision: AndroidLayoutRevision,
        resourceStore: VisualResourceStore
    ): com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot? {
        val snapshot = snapshotBuilder.buildSnapshotForLine(layout, lineIndex, revision) ?: return null
        resourceStore.put(snapshot)
        return snapshot
    }

    private enum class AnimationMode {
        GlyphAnimation, ClusterAnimation, RunAnimation, LineReflowAnimation, SystemSuppressed
    }
}
