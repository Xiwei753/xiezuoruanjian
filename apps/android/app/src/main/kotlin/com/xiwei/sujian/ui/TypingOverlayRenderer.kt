package com.xiwei.sujian.ui

import android.graphics.Canvas
import android.graphics.Paint

import java.util.concurrent.CopyOnWriteArrayList

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
    /**
     * 是否跳过 glyph 动画（ghost 文字绘制）。
     * 复杂字符（ZWJ emoji、组合音标、变体选择符、surrogate pair）只保留光标动画。
     */
    val skipGlyphAnimation: Boolean = run {
        if (insertedText.isEmpty()) false
        else containsComplexGrapheme(insertedText)
    }

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

    private fun containsComplexGrapheme(text: String): Boolean {
        var i = 0
        while (i < text.length) {
            val cp = text.codePointAt(i)
            val charCount = Character.charCount(cp)

            // Surrogate pair (non-BMP character like emoji)
            if (charCount == 2) return true

            // Zero Width Joiner
            if (cp == 0x200D) return true

            // Variation selectors (FE00-FE0F, E0100-E01EF)
            if (cp in 0xFE00..0xFE0F || cp in 0xE0100..0xE01EF) return true

            // Combining marks
            val type = Character.getType(cp)
            if (type == Character.NON_SPACING_MARK.toInt() ||
                type == Character.COMBINING_SPACING_MARK.toInt() ||
                type == Character.ENCLOSING_MARK.toInt()) return true

            i += charCount
        }
        return false
    }
}

/**
 * TypingOverlayRenderer — 旧版 ghost overlay 动画渲染器
 *
 * **已废弃**：ghost overlay 路线（路线 B）已废弃。
 * 正文完整绘制后叠 ghost 必然重影，这是架构缺陷。
 * 真吞吐只在 SujianEditorView 上实现（静态层跳过 range + overlay 层绘制）。
 * 此类只作为旧版编辑器无动画兜底使用，不再新增功能。
 *
 * 原路线说明：ghost overlay（路线 B）
 * - 插入动画：正文已完整绘制，overlay 从旧光标位置飞到目标位置（ghost 飞入效果）
 * - 删除动画：正文已删除，overlay 显示被删字符收缩/淡出（ghost 消失效果）
 * - 这不是真吐字/吞字：真吐字/吞字需要临时隐藏正文中的刚插入字符（路线 A），
 *   或走自绘正文/局部绘制接管。当前架构约束（系统 EditText 完整绘制正文、
 *   禁止注入 span 隐藏文字）决定了只能做 ghost overlay。
 */
class TypingOverlayRenderer(private val editText: WriterEditText) : EditorAnimationRuntime.Animatable {
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

        return hasMore || activeAnims.isNotEmpty()
    }

    fun onDraw(canvas: Canvas) {
        if (pausedForScroll) return
        if (activeAnims.isEmpty()) return

        val paint = editText.paint
        val originalAlpha = paint.alpha

        val padX = editText.compoundPaddingLeft.toFloat()
        val padY = editText.compoundPaddingTop.toFloat()
        val scrollX = editText.scrollX.toFloat()
        val scrollY = editText.scrollY.toFloat()

        for (anim in activeAnims) {
            val interpolatedProgress = 1f - (1f - anim.progress) * (1f - anim.progress)

            if (anim.skipGlyphAnimation) {
                // 复杂字符（ZWJ emoji、组合音标等）跳过 ghost 文字绘制，只保留光标动画
                continue
            }

            if (anim.isDeletion) {
                val destX = if (anim.endX >= 0f) anim.endX else anim.startX
                val destY = if (anim.endY >= 0f) anim.endY else anim.startY

                val currentX = anim.startX + (destX - anim.startX) * interpolatedProgress
                val currentY = anim.startY + (destY - anim.startY) * interpolatedProgress
                val scale = 1f - 0.55f * interpolatedProgress
                paint.alpha = (originalAlpha * 0.75f * (1f - interpolatedProgress)).toInt().coerceIn(0, 255)

                var offsetX = 0f
                for (idx in anim.codePoints.indices) {
                    val cp = anim.codePoints[idx]
                    val isNewline = (cp == '\n'.code || cp == '\r'.code)
                    if (isNewline) continue

                    val textToDraw = anim.cachedStrings[idx]
                    val drawX = currentX + offsetX + padX - scrollX
                    val drawY = currentY + padY - scrollY
                    canvas.save()
                    canvas.scale(scale, scale, drawX, drawY)
                    canvas.drawText(textToDraw, drawX, drawY, paint)
                    canvas.restore()
                    offsetX += paint.measureText(textToDraw)
                }
            } else {
                val destX = if (anim.endX >= 0f) anim.endX else anim.startX
                val destY = if (anim.endY >= 0f) anim.endY else anim.startY

                val sX = anim.startX
                val sY = anim.startY

                val currentX = sX + (destX - sX) * interpolatedProgress
                val currentY = sY + (destY - sY) * interpolatedProgress

                val scale = 0.72f + 0.28f * interpolatedProgress

                val opacity = if (interpolatedProgress < 0.5f) {
                    interpolatedProgress * 2f * 0.75f
                } else {
                    (1f - interpolatedProgress) * 2f * 0.75f
                }
                paint.alpha = (originalAlpha * opacity).toInt().coerceIn(0, 255)

                var offsetX = 0f
                for (idx in anim.codePoints.indices) {
                    val cp = anim.codePoints[idx]
                    val isNewline = (cp == '\n'.code || cp == '\r'.code)
                    if (isNewline) continue

                    val textToDraw = anim.cachedStrings[idx]
                    val drawX = currentX + offsetX + padX - scrollX
                    val drawY = currentY + padY - scrollY
                    canvas.save()
                    canvas.scale(scale, scale, drawX, drawY)
                    canvas.drawText(textToDraw, drawX, drawY, paint)
                    canvas.restore()
                    offsetX += paint.measureText(textToDraw)
                }
            }
        }

        paint.alpha = originalAlpha
    }
}
