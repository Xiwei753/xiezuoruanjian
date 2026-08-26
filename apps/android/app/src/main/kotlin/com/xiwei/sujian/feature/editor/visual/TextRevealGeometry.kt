package com.xiwei.sujian.feature.editor.visual

/**
 * #639 评论 5421085782 问题2：caret reveal 裁剪几何的单一事实来源。
 *
 * 之前 [com.xiwei.sujian.feature.editor.render.AndroidTextAnimationRenderer.computeRevealClipRect]
 * 是唯一持有这份几何的地方，CrossfadeOld 的 rebase 冻结裁剪只能另写一套"按 bitmap
 * 宽度乘 fraction"的近似，与真实 caret reveal 几何不一致（字形 overhang 让 bitmap
 * 宽度 > caret 宽度，LTR 时 left 偏移；RTL 时直接裁反）。
 *
 * 这里把纯几何部分抽成顶层函数，正常 Insert/Delete renderer 和 rebase 冻结都调用
 * 同一份。输入是 document-space 的 destination/mode/anchorX/boundaryFromX/boundaryToX
 * 和已算好的 fraction，返回 document-space 的可见 [android.graphics.RectF]。
 *
 * 不依赖 Compose/Activity/View，只依赖 android.graphics.RectF — 放在 visual/ 下
 * 符合 Android 分层（visual 只处理显示与动画，不写正文业务状态）。
 */
object TextRevealGeometry {
    /**
     * Compute the document-space clip rect for reveal/swallow animation at [fraction].
     *
     * REVEAL: fraction=0 -> null (not visible), fraction=1 -> full [destination].
     * SWALLOW: fraction=0 -> full [destination], fraction=1 -> null (not visible).
     *
     * The clip rect is the intersection of [destination] with the region between
     * [anchorX] and the interpolated boundary position. This ensures only the
     * visible portion of the glyph is drawn, while overhang at fraction=1 is fully
     * shown (returns complete destination, not clipped to caret X).
     *
     * 几何契约（与原 renderer.computeRevealClipRect 完全一致，只是去掉了
     * TextRevealSpec.fraction(globalProgress) 这一步，让调用方自己算 fraction）：
     * - boundary = boundaryFromX + (boundaryToX - boundaryFromX) * fraction
     * - left = min(anchorX, boundary), right = max(anchorX, boundary)
     * - clipLeft = max(left, destination.left), clipRight = min(right, destination.right)
     * - clipRight <= clipLeft -> null
     */
    fun computeRevealClipRect(
        destination: android.graphics.RectF,
        mode: TextRevealMode,
        anchorX: Float,
        boundaryFromX: Float,
        boundaryToX: Float,
        fraction: Float,
    ): android.graphics.RectF? {
        return when (mode) {
            TextRevealMode.REVEAL -> {
                if (fraction <= 0f) return null
                if (fraction >= 1f) return android.graphics.RectF(destination)
                val boundary = boundaryFromX + (boundaryToX - boundaryFromX) * fraction
                val left = kotlin.math.min(anchorX, boundary)
                val right = kotlin.math.max(anchorX, boundary)
                val clipLeft = kotlin.math.max(left, destination.left)
                val clipRight = kotlin.math.min(right, destination.right)
                if (clipRight <= clipLeft) return null
                android.graphics.RectF(clipLeft, destination.top, clipRight, destination.bottom)
            }
            TextRevealMode.SWALLOW -> {
                if (fraction >= 1f) return null
                if (fraction <= 0f) return android.graphics.RectF(destination)
                val boundary = boundaryFromX + (boundaryToX - boundaryFromX) * fraction
                val left = kotlin.math.min(anchorX, boundary)
                val right = kotlin.math.max(anchorX, boundary)
                val clipLeft = kotlin.math.max(left, destination.left)
                val clipRight = kotlin.math.min(right, destination.right)
                if (clipRight <= clipLeft) return null
                android.graphics.RectF(clipLeft, destination.top, clipRight, destination.bottom)
            }
        }
    }
}
