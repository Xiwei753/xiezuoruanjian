package com.xiwei.sujian.feature.editor.render

import android.graphics.Canvas
import android.graphics.Paint
import com.xiwei.sujian.feature.editor.visual.PreparedVisualTransaction
import com.xiwei.sujian.feature.editor.visual.StaticSuppressionMode
import com.xiwei.sujian.feature.editor.visual.TextRevealGeometry
import com.xiwei.sujian.feature.editor.visual.TextRevealSpec
import com.xiwei.sujian.feature.editor.visual.defaultStaticSuppressionModeForRole

class AndroidTextAnimationRenderer {
    private val slicePaint =
        Paint().apply {
            isAntiAlias = true
        }

    fun drawAnimatedSlices(
        canvas: Canvas,
        transaction: PreparedVisualTransaction,
        progress: Float,
    ) {
        for (slice in transaction.animatedSlices) {
            val snapshot = slice.snapshot ?: continue
            val bitmap = snapshot.bitmap ?: continue
            // #637 评论 5386066978 项2：每个 slice 先把全局 progress 映射到
            // 本 slice 的 local progress（rebase continuation 时已走部分不重新计时）。
            val localProgress = slice.progressWindow.map(progress)
            // #639 评论 5427183226：fixedRevealClipRect 提到 drawOrthogonalSlice
            // 正交化，不再按 SliceRole 分支。所有 role（Move/Insert/Delete/CrossfadeOld/
            // CrossfadeNew/Static）统一走 drawOrthogonalSlice，三条轨正交绘制：
            // 1. 位置轨：visualDestinationRectAt(progress) 得到 currentRect。
            // 2. alpha 轨：startAlpha -> endAlpha 线性插值。
            // 3. reveal 轨：优先级 fixedRevealClipRect > revealSpec clip > no clip。
            //    fixedRevealClipRect 是冻结的 document-space clip rect（CrossfadeOld
            //    冻结半截字），revealSpec clip 是动态 reveal clip。有 clip 时仍用当前
            //    alpha，不强制 alpha=255。
            drawOrthogonalSlice(canvas, bitmap, slice, progress, localProgress)
        }
    }

    /**
     * #639 评论 5427183226：位置、alpha、reveal 三条轨正交绘制。
     *
     * 所有 role（Move/Insert/Delete/CrossfadeOld/CrossfadeNew/Static）共用这一个入口：
     * 1. 位置轨：[PreparedVisualTransaction.AnimatedSlice.visualDestinationRectAt] 得到 currentRect。
     * 2. alpha 轨：startAlpha -> endAlpha 线性插值。
     * 3. reveal 轨：优先级 fixedRevealClipRect > revealSpec clip > no clip。
     *    - fixedRevealClipRect 非 null 时用冻结的 document-space clip rect（CrossfadeOld
     *      冻结半截字再淡出），clip rect 在本次动画期间保持不动，只让 alpha 变化。
     *    - 否则 revealSpec 非 null 时按 [TextRevealGeometry] 算动态 clip。
     *    - clip 为 null 不画。有 clip 时仍用当前 alpha，不强制 alpha=255 — 这修掉场景2
     *      （CrossfadeNew alpha=0.4 -> Insert 的 alpha 续播被 reveal clip 吞掉）。
     *
     * revealSpec 为 null 且 fixedRevealClipRect 为 null 时退化为纯位置+alpha 绘制。
     * fromDestinationRect 为 null 时 currentRect==destinationRect，位置轨退化为常量。
     */
    private fun drawOrthogonalSlice(
        canvas: Canvas,
        bitmap: android.graphics.Bitmap,
        slice: PreparedVisualTransaction.AnimatedSlice,
        globalProgress: Float,
        localProgress: Float,
    ) {
        val currentRect = slice.visualDestinationRectAt(globalProgress)
        val alpha = slice.startAlpha + (slice.endAlpha - slice.startAlpha) * localProgress
        slicePaint.alpha = (alpha * 255).toInt().coerceIn(0, 255)

        // #639 评论 5427183226：优先级 fixedRevealClipRect > revealSpec clip > no clip。
        val fixedClip = slice.fixedRevealClipRect
        if (fixedClip != null) {
            // #639 评论 5427812180 缺陷5：fixedClipBaseRect 非 null 时，fixedRevealClipRect
            // 是相对于 baseRect 的 document-space clip。每帧用 currentRect - baseRect 平移，
            // 让 clip 跟 bitmap 一起移动（mapped rebase 后位置插值时 clip 不钉在绝对坐标）。
            val effectiveClip = computeEffectiveFixedClip(fixedClip, slice.fixedClipBaseRect, currentRect)
            val save = canvas.save()
            canvas.clipRect(effectiveClip)
            canvas.drawBitmap(bitmap, slice.sourceRect, currentRect, slicePaint)
            canvas.restoreToCount(save)
            return
        }
        val spec = slice.revealSpec
        if (spec != null) {
            val fraction = spec.fraction(localProgress)
            // #639 评论 5422606865 问题2：当 fromDestinationRect 非 null（rebase 把
            // Insert 接到旧 Move 当前位置）时，bitmap 画在 currentRect，reveal clip 几何
            // 随 currentRect 平移：anchorX/boundaryFromX/boundaryToX 同步加
            // currentRect.left - destinationRect.left，再交给 TextRevealGeometry。
            val dx = currentRect.left - slice.destinationRect.left
            val clipRect =
                TextRevealGeometry.computeRevealClipRect(
                    currentRect,
                    spec.mode,
                    spec.anchorX + dx,
                    spec.boundaryFromX + dx,
                    spec.boundaryToX + dx,
                    fraction,
                ) ?: return
            val save = canvas.save()
            canvas.clipRect(clipRect)
            // #639 评论 5425871530 第三部分：有 reveal clip 时仍然使用当前 alpha，
            // 不强制 slicePaint.alpha = 255。这修掉场景2（CrossfadeNew alpha=0.4 -> Insert
            // 的 alpha 续播被 reveal clip 吞掉）。
            canvas.drawBitmap(bitmap, slice.sourceRect, currentRect, slicePaint)
            canvas.restoreToCount(save)
        } else {
            canvas.drawBitmap(bitmap, slice.sourceRect, currentRect, slicePaint)
        }
    }

    /**
     * Compute the clip rect for reveal/swallow animation at [globalProgress].
     *
     * #639 评论 5421085782 问题2：纯几何部分抽到 [TextRevealGeometry.computeRevealClipRect]，
     * 正常 Insert/Delete renderer 和 rebase 冻结（CrossfadeOld 的 fixedRevealClipRect）
     * 共用同一份几何，不再有第二套"按 bitmap 宽度乘 fraction"的近似。
     *
     * REVEAL: fraction=0 -> null (not visible), fraction=1 -> full destination.
     * SWALLOW: fraction=0 -> full destination, fraction=1 -> null (not visible).
     */
    fun computeRevealClipRect(
        destination: android.graphics.RectF,
        spec: TextRevealSpec,
        globalProgress: Float,
    ): android.graphics.RectF? {
        val fraction = spec.fraction(globalProgress)
        return TextRevealGeometry.computeRevealClipRect(
            destination,
            spec.mode,
            spec.anchorX,
            spec.boundaryFromX,
            spec.boundaryToX,
            fraction,
        )
    }

    fun drawAnimatedCursor(
        canvas: Canvas,
        transaction: PreparedVisualTransaction,
        progress: Float,
        cursorPaint: Paint,
    ) {
        val ct = transaction.cursorTransition ?: return
        drawAnimatedCursor(canvas, ct, progress, cursorPaint)
    }

    /**
     * #595 五：按独立光标过渡几何绘制动画光标 — 供静态文字路径（文字轨结束或
     * CursorOnly 抑制）在光标轨未结束时继续绘制平滑光标。
     *
     * #637 评论 5386066978 项2：先映射 [transition.progressWindow] 再插值，
     * rebase continuation 时光标已走部分不重新计时。
     *
     * #637 评论 5389230907：几何计算调用 [CursorTransition.rectAt] — 与
     * [AndroidTextAnimationEngine.computeCurrentCursorRect] 共用同一份实现，
     * 不会再次漂移。
     */
    fun drawAnimatedCursor(
        canvas: Canvas,
        transition: PreparedVisualTransaction.CursorTransition,
        progress: Float,
        cursorPaint: Paint,
    ) {
        if (!transition.shouldAnimate) return

        val rect = transition.rectAt(progress)
        canvas.drawRect(rect.left, rect.top, rect.right, rect.bottom, cursorPaint)
    }

    /**
     * #638: Compute static suppression regions for hole-punching at [progress].
     *
     * - Insert/Move/CrossfadeNew/Static: suppress [slice.destinationRect] (new-layout
     *   content). 静态底图里的完整字始终在 destinationRect（新 Layout 中完整静态像素
     *   的位置），不在正在移动的 currentRect。在 currentRect 挖洞会让 destinationRect
     *   的静态完整字没被 suppress → 双影。renderer / captureFrame 仍用
     *   visualDestinationRectAt(progress) 画动画像素，这里是静态底图去重，两者语义不同。
     * - CrossfadeOld: alpha-mixed, NOT suppressed (no hole).
     * - Delete + SWALLOW: suppress only the currently visible portion via
     *   [computeRevealClipRect]. The destination is the old position; new layout text
     *   may have shifted there, so we only hide pixels the Delete slice still occupies.
     *
     * This ensures the same pixel is never double-rendered in the same frame:
     * static text with holes renders the new layout minus suppressed regions, then
     * animated slices draw on top. For Delete SWALLOW, the visible shrink-away region
     * is suppressed from the static draw, so the old snapshot doesn't overlap new text.
     */
    fun computeStaticSuppressionRegions(
        transaction: PreparedVisualTransaction,
        progress: Float,
    ): List<android.graphics.RectF> {
        val regions = mutableListOf<android.graphics.RectF>()
        for (slice in transaction.animatedSlices) {
            val srcRect = slice.sourceRect
            if (srcRect.width() <= 0 || srcRect.height() <= 0) continue

            // #639 评论 5427812180 缺陷4：按 slice.staticSuppressionMode 判断，不再 when(slice.role)。
            // mapped rebase 继续旧视觉轨后 role 和"静态底图怎么挖洞"会不一致，所以 suppression
            // 必须按独立 mode。slice.staticSuppressionMode == null 时按 role fallback（向后兼容）。
            val mode = slice.staticSuppressionMode ?: defaultStaticSuppressionModeForRole(slice.role)
            when (mode) {
                StaticSuppressionMode.NONE -> { }
                StaticSuppressionMode.DESTINATION_RECT -> {
                    // #639 评论 5424613367 问题2：Insert/Move/CrossfadeNew/Static 的静态底图
                    // suppression 用 destinationRect（新 Layout 中完整静态像素的位置），不用
                    // currentRect（动画当前几何）。静态底图里的完整字始终在 destinationRect，
                    // 在 currentRect 挖洞会让 destinationRect 的静态完整字没被 suppress → 双影。
                    regions.add(android.graphics.RectF(slice.destinationRect))
                }
                StaticSuppressionMode.VISIBLE_CLIP -> {
                    // #639 评论 5427183226：Delete 携带 fixed clip 时 suppress fixed clip，
                    // 不能因为 revealSpec == null 就 continue 跳过 — 否则静态底图双影。
                    val fixedClip = slice.fixedRevealClipRect
                    if (fixedClip != null) {
                        // #639 评论 5427812180 缺陷5：fixedClipBaseRect 非 null 时按 currentRect 平移。
                        val currentRect = slice.visualDestinationRectAt(progress)
                        regions.add(computeEffectiveFixedClip(fixedClip, slice.fixedClipBaseRect, currentRect))
                        continue
                    }
                    val revealSpec = slice.revealSpec ?: continue
                    // #639 评论 5422606865 问题2：clipRect 基于 currentRect 和平移后的
                    // spec，与 drawRevealSlice 一致，suppression 挖的洞就是 renderer
                    // 实际画的位置。
                    val currentRect = slice.visualDestinationRectAt(progress)
                    val localProgress = slice.progressWindow.map(progress)
                    val fraction = revealSpec.fraction(localProgress)
                    val dx = currentRect.left - slice.destinationRect.left
                    val clipRect =
                        TextRevealGeometry.computeRevealClipRect(
                            currentRect,
                            revealSpec.mode,
                            revealSpec.anchorX + dx,
                            revealSpec.boundaryFromX + dx,
                            revealSpec.boundaryToX + dx,
                            fraction,
                        )
                    if (clipRect != null) {
                        regions.add(clipRect)
                    }
                }
            }
        }
        return regions
    }

    /**
     * #639 评论 5427812180 缺陷5：计算 effective document-space fixed clip rect。
     *
     * [fixedClip] 是相对于 [baseRect] 的 document-space clip。每帧用
     * currentRect - baseRect 平移，让 clip 跟 bitmap 一起移动（mapped rebase 后
     * 位置插值时 clip 不钉在绝对坐标）。[baseRect] 为 null 表示 [fixedClip] 是绝对
     * document-space（位置不动或未 mapped），原样返回。
     */
    private fun computeEffectiveFixedClip(
        fixedClip: android.graphics.RectF,
        baseRect: android.graphics.RectF?,
        currentRect: android.graphics.RectF,
    ): android.graphics.RectF {
        if (baseRect == null) return android.graphics.RectF(fixedClip)
        val dx = currentRect.left - baseRect.left
        val dy = currentRect.top - baseRect.top
        return android.graphics.RectF(
            fixedClip.left + dx,
            fixedClip.top + dy,
            fixedClip.right + dx,
            fixedClip.bottom + dy,
        )
    }
}
