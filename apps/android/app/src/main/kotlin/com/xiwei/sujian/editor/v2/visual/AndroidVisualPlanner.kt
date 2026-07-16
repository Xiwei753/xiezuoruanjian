package com.xiwei.sujian.editor.v2.visual

import com.xiwei.sujian.editor.v2.mirror.VisualIntent
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutRevision
import com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot
import com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshotBuilder

class PreparedVisualTransaction(
    val transactionId: Long,
    val oldRevision: AndroidLayoutRevision?,
    val newRevision: AndroidLayoutRevision?,
    val staticPatches: List<StaticPatch>,
    val animatedSlices: List<AnimatedSlice>,
    val cursorTransition: CursorTransition?,
    val durationMs: Long
) {
    class StaticPatch(
        val newSnapshotId: Long,
        val lineIndex: Int,
        val destinationRect: android.graphics.RectF,
        val visibleSourceRects: List<android.graphics.Rect>
    )

    class AnimatedSlice(
        val role: SliceRole,
        val snapshot: AndroidLineSnapshot?,
        val sourceRect: android.graphics.Rect,
        val destinationRect: android.graphics.RectF,
        val startAlpha: Float,
        val endAlpha: Float
    )

    class CursorTransition(
        val fromX: Float,
        val fromY: Float,
        val fromHeight: Float,
        val toX: Float,
        val toY: Float,
        val toHeight: Float,
        val shouldAnimate: Boolean
    )
}

enum class SliceRole {
    Insert, Delete, Move, CrossfadeOld, CrossfadeNew, Static
}

enum class TransactionState {
    Pending, Prepared, Rendering, Paused, Completed, Cancelled
}

class AndroidVisualPlanner {
    private var oldRevision: AndroidLayoutRevision? = null
    private val snapshotBuilder = AndroidLineSnapshotBuilder()

    fun prepare(
        visualIntent: VisualIntent,
        layoutEngine: com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine
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

            for (lineIndex in affectedLines) {
                val oldLineRange = oldRev.lineRanges.getOrNull(lineIndex)
                val newLineRange = newRev.lineRanges.getOrNull(lineIndex)

                if (oldLineRange != null && newLineRange != null) {
                    val oldSnapshot = snapshotBuilder.buildSnapshotForLine(layout, lineIndex, oldRev)
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
                    val newSnapshot = snapshotBuilder.buildSnapshotForLine(layout, lineIndex, newRev)
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

    fun resetOldRevision() {
        oldRevision = null
    }
}
