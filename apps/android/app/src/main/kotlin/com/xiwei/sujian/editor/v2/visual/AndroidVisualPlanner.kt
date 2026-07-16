package com.xiwei.sujian.editor.v2.visual

import com.xiwei.sujian.editor.v2.mirror.VisualIntent
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutRevision
import com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshotBuilder

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

        val staticPatches = mutableListOf<PreparedVisualTransaction.StaticPatch>()
        val animatedSlices = mutableListOf<PreparedVisualTransaction.AnimatedSlice>()
        var cursorTransition: PreparedVisualTransaction.CursorTransition? = null

        val oldRev = oldRevision
        val savedOldLayout = oldLayout
        val newRev = newRevision

        if (oldRev != null && newRev != null && layout != null) {
            val affectedLines = computeAffectedLines(visualIntent, oldRev, newRev)
            val mode = parseAnimationMode(visualIntent.animationMode)

            when (mode) {
                AnimationMode.GlyphAnimation, AnimationMode.ClusterAnimation -> {
                    planGlyphOrClusterAnimation(
                        visualIntent, oldRev, newRev, savedOldLayout, layout,
                        affectedLines, animatedSlices, staticPatches, resourceStore
                    )
                }
                AnimationMode.RunAnimation -> {
                    planRunAnimation(
                        visualIntent, oldRev, newRev, savedOldLayout, layout,
                        affectedLines, animatedSlices, staticPatches, resourceStore
                    )
                }
                AnimationMode.LineReflowAnimation -> {
                    planLineReflowAnimation(
                        visualIntent, oldRev, newRev, savedOldLayout, layout,
                        affectedLines, animatedSlices, staticPatches, resourceStore
                    )
                }
                AnimationMode.SystemSuppressed -> {
                    planNoAnimation(newRev, staticPatches)
                }
                else -> {
                    planCrossfadeAnimation(
                        visualIntent, oldRev, newRev, savedOldLayout, layout,
                        affectedLines, animatedSlices, staticPatches, resourceStore
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
            selectionDecoration = buildSelectionDecoration(layoutEngine),
            preeditDecoration = buildPreeditDecoration(layoutEngine),
            cursorTransition = cursorTransition,
            durationMs = durationMs
        )

        oldRevision = newRevision
        oldLayout = layout

        return result
    }

    private fun planGlyphOrClusterAnimation(
        visualIntent: VisualIntent,
        oldRev: AndroidLayoutRevision,
        newRev: AndroidLayoutRevision,
        oldLayout: android.text.Layout?,
        newLayout: android.text.Layout,
        affectedLines: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>,
        resourceStore: VisualResourceStore
    ) {
        val isInsert = visualIntent.isInsert()
        for (lineIndex in affectedLines) {
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex)
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex)

            if (isInsert && newLineRange != null) {
                val newSnapshot = createSnapshot(newLayout, lineIndex, newRev, resourceStore)
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
                val layoutForOldSnapshot = oldLayout ?: newLayout
                val oldSnapshot = createSnapshot(layoutForOldSnapshot, lineIndex, oldRev, resourceStore)
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
        oldLayout: android.text.Layout?,
        newLayout: android.text.Layout,
        affectedLines: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>,
        resourceStore: VisualResourceStore
    ) {
        for (lineIndex in affectedLines) {
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex)
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex)

            if (oldLineRange != null && newLineRange != null) {
                val layoutForOldSnapshot = oldLayout ?: newLayout
                val oldSnapshot = createSnapshot(layoutForOldSnapshot, lineIndex, oldRev, resourceStore)
                val newSnapshot = createSnapshot(newLayout, lineIndex, newRev, resourceStore)

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
                val newSnapshot = createSnapshot(newLayout, lineIndex, newRev, resourceStore)
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
        oldLayout: android.text.Layout?,
        newLayout: android.text.Layout,
        affectedLines: Set<Int>,
        animatedSlices: MutableList<PreparedVisualTransaction.AnimatedSlice>,
        staticPatches: MutableList<PreparedVisualTransaction.StaticPatch>,
        resourceStore: VisualResourceStore
    ) {
        for (lineIndex in affectedLines) {
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex)
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex)

            if (oldLineRange != null && newLineRange != null) {
                val newSnapshot = createSnapshot(newLayout, lineIndex, newRev, resourceStore)
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
                val newSnapshot = createSnapshot(newLayout, lineIndex, newRev, resourceStore)
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
                val layoutForOldSnapshot = oldLayout ?: newLayout
                val oldSnapshot = createSnapshot(layoutForOldSnapshot, lineIndex, oldRev, resourceStore)
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
        resourceStore: VisualResourceStore
    ) {
        for (lineIndex in affectedLines) {
            val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex)
            val newLineRange = newRev.lineRanges.getOrNull(lineIndex)

            if (oldLineRange != null && newLineRange != null) {
                val layoutForOldSnapshot = oldLayout ?: newLayout
                val oldSnapshot = createSnapshot(layoutForOldSnapshot, lineIndex, oldRev, resourceStore)
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
                val newSnapshot = createSnapshot(newLayout, lineIndex, newRev, resourceStore)
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
