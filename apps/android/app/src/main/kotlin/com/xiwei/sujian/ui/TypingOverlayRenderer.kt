package com.xiwei.sujian.ui

import android.graphics.Canvas
import android.graphics.Paint

import java.util.concurrent.CopyOnWriteArrayList
import kotlin.math.sqrt

/**
 * TypingOverlayRenderer — 打字动画覆盖层渲染器
 *
 * 在文本上方绘制打字动画效果（字符弹出、渐变等），增强写作体验。
 *
 * ## 架构定位
 * - EditorRenderLayer → TypingOverlayRenderer → Canvas 绘制
 * - 实现 EditorAnimationRuntime.Animatable 接口
 *
 * ## 职责边界
 * - **做**：打字动画的计算和绘制
 * - **不做**：文本内容管理（由 EditText 负责）
 *
 * ## 动画参数
 * - Insert: opacity 0→0.75→0, scale 0.72→1.0, duration 80~180ms
 * - Delete: opacity 0.75→0, scale 1.0→0.45, duration 80~180ms
 * - 静态正文永远由系统完整绘制，动画只做 overlay（附加绘制）
 */

data class OverlayAnim(
    val insertedStart: Int,
    val insertedText: String,
    val startX: Float,
    val startY: Float,
    val endX: Float = -1f,
    val endY: Float = -1f,
    var progress: Float = 0f,
    var startTimeNanos: Long = -1L,
    val durationMs: Long,

    val isDeletion: Boolean = false
) {
    val codePoints: List<Int> = buildList {
        var i = 0
        while (i < insertedText.length) {
            val cp = Character.codePointAt(insertedText, i)
            add(cp)
            i += Character.charCount(cp)
        }
    }

    val cachedStrings: List<String> = codePoints.map { cp ->
        String(Character.toChars(cp))
    }
}

class TypingOverlayRenderer(private val editText: WriterEditText) : EditorAnimationRuntime.Animatable {
    private val DEBUG_ANIM = false
    private val TAG = "WriterEditorAnim"
    private val activeAnims = CopyOnWriteArrayList<OverlayAnim>()
    private val MAX_ANIMATIONS = 24
    private var pausedForScroll = false

    fun setPausedForScroll(paused: Boolean) {
        if (pausedForScroll == paused) return
        pausedForScroll = paused
        if (paused) {
            clear()
        }
    }

    fun addAnim(anim: OverlayAnim) {
        if (pausedForScroll) {
            return
        }
        if (activeAnims.size >= MAX_ANIMATIONS) {
            activeAnims.removeAt(0)
        }
        activeAnims.add(anim)
        editText.animationRuntime?.register(this)
    }

    fun removeAnim(anim: OverlayAnim) {
        activeAnims.remove(anim)
        if (activeAnims.isEmpty()) {
            editText.animationRuntime?.unregister(this)
        }
    }

    fun clear() {

        activeAnims.clear()
        editText.animationRuntime?.unregister(this)
    }

    fun onEditorResume() {
        if (pausedForScroll) return
        if (activeAnims.isNotEmpty()) {
            editText.animationRuntime?.register(this)
            editText.postInvalidateOnAnimation()
        }
    }

    override fun onAnimationStep(frameTimeNanos: Long): Boolean {
        if (pausedForScroll) {
            clear()
            return false
        }
        if (activeAnims.isEmpty()) return false

        val iterator = activeAnims.iterator()
        var hasMore = false

        while (iterator.hasNext()) {
            val anim = iterator.next()
            if (anim.startTimeNanos == -1L) {
                anim.startTimeNanos = frameTimeNanos
            }
            val elapsedNanos = frameTimeNanos - anim.startTimeNanos
            val animDurationNanos = anim.durationMs * 1_000_000f
            if (animDurationNanos <= 0) {
                anim.progress = 1f
            } else {
                anim.progress = (elapsedNanos / animDurationNanos).coerceIn(0f, 1f)
            }

            if (anim.progress >= 1f) {
                activeAnims.remove(anim)
            } else {
                hasMore = true
            }
        }

        // Static text is always fully drawn by the system; overlay is purely additive.

        return hasMore || activeAnims.isNotEmpty()
    }

    fun onDraw(canvas: Canvas) {
        if (pausedForScroll) return
        if (activeAnims.isEmpty()) return

        val layout = editText.layout ?: return
        val paint = editText.paint
        val originalAlpha = paint.alpha
        val textLength = editText.text?.length ?: 0

        val padX = editText.compoundPaddingLeft.toFloat()
        val padY = editText.compoundPaddingTop.toFloat()

        for (anim in activeAnims) {
            var i = anim.insertedStart
            var drawnCodepoints = 0

            // Apply decelerate interpolation dynamically
            val interpolatedProgress = 1f - (1f - anim.progress) * (1f - anim.progress)

            if (anim.isDeletion) {
                // ── Delete animation ──
                // opacity: 0.75 → 0
                // scale: 1.0 → 0.45
                // position: glyph → cursor
                val destX = if (anim.endX >= 0f) anim.endX else anim.startX
                val destY = if (anim.endY >= 0f) anim.endY else anim.startY

                val currentX = anim.startX + (destX - anim.startX) * interpolatedProgress
                val currentY = anim.startY + (destY - anim.startY) * interpolatedProgress
                val scale = 1f - 0.55f * interpolatedProgress  // 1.0 → 0.45
                paint.alpha = (originalAlpha * 0.75f * (1f - interpolatedProgress)).toInt().coerceIn(0, 255)

                var offsetX = 0f
                for (idx in anim.codePoints.indices) {
                    val cp = anim.codePoints[idx]
                    val isNewline = (cp == '\n'.code || cp == '\r'.code)
                    if (isNewline) continue

                    val textToDraw = anim.cachedStrings[idx]
                    canvas.save()
                    canvas.scale(scale, scale, currentX + padX, currentY + padY)
                    canvas.drawText(
                        textToDraw,
                        currentX + offsetX + padX,
                        currentY + padY,
                        paint
                    )
                    canvas.restore()
                    offsetX += paint.measureText(textToDraw)
                    drawnCodepoints++
                }
            } else {
                // ── Insert animation ──
                // opacity: 0 → 0.75 → 0 (peak at midpoint)
                // scale: 0.72 → 1.0
                // position: cursor → glyph
                for (idx in anim.codePoints.indices) {
                    val cp = anim.codePoints[idx]
                    val charCount = Character.charCount(cp)
                    if (i + charCount > textLength) break

                    val isNewline = (cp == '\n'.code || cp == '\r'.code)

                    if (isNewline) {
                        i += charCount
                        continue
                    }

                    val textToDraw = anim.cachedStrings[idx]
                    val destX = layout.getPrimaryHorizontal(i)
                    val line = layout.getLineForOffset(i)
                    val destY = layout.getLineBaseline(line).toFloat()

                    var sX = anim.startX
                    var sY = anim.startY

                    if (sX < 0 || sY < 0) {
                        sX = destX
                        sY = destY
                    } else {
                        val dx = destX - sX
                        val dy = destY - sY
                        val distSq = dx * dx + dy * dy
                        val maxDist = 10f
                        if (distSq > maxDist * maxDist) {
                            val dist = sqrt(distSq.toDouble()).toFloat()
                            sX = destX - (dx / dist) * maxDist
                            sY = destY - (dy / dist) * maxDist
                        }
                    }

                    val currentX = sX + (destX - sX) * interpolatedProgress
                    val currentY = sY + (destY - sY) * interpolatedProgress

                    // Scale: 0.72 → 1.0
                    val scale = 0.72f + 0.28f * interpolatedProgress

                    // Opacity: 0 → 0.75 → 0 (peak at midpoint)
                    val opacity = if (interpolatedProgress < 0.5f) {
                        interpolatedProgress * 2f * 0.75f
                    } else {
                        (1f - interpolatedProgress) * 2f * 0.75f
                    }
                    paint.alpha = (originalAlpha * opacity).toInt().coerceIn(0, 255)

                    canvas.save()
                    canvas.scale(scale, scale, currentX + padX, currentY + padY)
                    canvas.drawText(
                        textToDraw,
                        currentX + padX,
                        currentY + padY,
                        paint
                    )
                    canvas.restore()

                    drawnCodepoints++
                    i += charCount
                }
            }

            if (DEBUG_ANIM) {
                android.util.Log.d(TAG, "onDraw - insertedStart: ${anim.insertedStart}, visibleAnimatedCodepoints: $drawnCodepoints, overlayCount: ${activeAnims.size}")
            }
        }

        paint.alpha = originalAlpha
    }
}
