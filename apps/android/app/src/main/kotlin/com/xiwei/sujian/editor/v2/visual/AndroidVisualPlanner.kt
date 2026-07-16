package com.xiwei.sujian.editor.v2.visual

import com.xiwei.sujian.editor.v2.mirror.VisualIntent
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutRevision
import com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot

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

class AndroidVisualPlanner {

    fun prepare(
        visualIntent: VisualIntent,
        layoutEngine: com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine
    ): PreparedVisualTransaction {
        val newRevision = layoutEngine.getCurrentRevision()
        val durationMs = visualIntent.durationMs

        val staticPatches = mutableListOf<PreparedVisualTransaction.StaticPatch>()
        val animatedSlices = mutableListOf<PreparedVisualTransaction.AnimatedSlice>()
        var cursorTransition: PreparedVisualTransaction.CursorTransition? = null

        if (visualIntent.coordinatedCursor.shouldAnimate) {
            cursorTransition = PreparedVisualTransaction.CursorTransition(
                fromX = 0f, fromY = 0f, fromHeight = 0f,
                toX = 0f, toY = 0f, toHeight = 0f,
                shouldAnimate = true
            )
        }

        return PreparedVisualTransaction(
            transactionId = System.nanoTime(),
            oldRevision = null,
            newRevision = newRevision,
            staticPatches = staticPatches,
            animatedSlices = animatedSlices,
            cursorTransition = cursorTransition,
            durationMs = durationMs
        )
    }
}
