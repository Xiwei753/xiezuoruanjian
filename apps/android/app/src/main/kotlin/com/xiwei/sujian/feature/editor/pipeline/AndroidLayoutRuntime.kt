package com.xiwei.sujian.feature.editor.pipeline

import android.text.Layout
import android.text.TextPaint
import com.xiwei.sujian.feature.editor.layout.AndroidLayoutEngine
import com.xiwei.sujian.feature.editor.layout.AndroidLayoutRevision
import com.xiwei.sujian.feature.editor.projection.DisplayPatch
import com.xiwei.sujian.feature.editor.projection.DisplayTextMirror
import com.xiwei.sujian.feature.editor.projection.DisplayTextProjection

class AndroidLayoutRuntime(
    val mirror: DisplayTextMirror,
    val layoutEngine: AndroidLayoutEngine,
) {
    constructor(mirror: DisplayTextMirror, textPaint: TextPaint) : this(
        mirror,
        AndroidLayoutEngine(mirror, textPaint),
    )

    private var currentProjection: DisplayTextProjection = DisplayTextProjection.identity("")
    private var secretDisplayMode: Boolean = false

    fun requestLayout() {
        layoutEngine.requestLayout()
    }

    fun getLayout(): Layout? = layoutEngine.getLayout()

    fun getCurrentRevision(): AndroidLayoutRevision? = layoutEngine.getCurrentRevision()

    fun getWidth(): Float = layoutEngine.getWidth()

    fun setWidth(width: Float) {
        layoutEngine.setWidth(width)
    }

    fun setLineSpacingMultiplier(multiplier: Float) {
        layoutEngine.setLineSpacingMultiplier(multiplier)
    }

    /**
     * #624 评论3：首行缩进设置透传（开关 + 字符宽度）— 变化后当前正文立即重排。
     */
    fun setFirstLineIndent(
        enabled: Boolean,
        widthChars: Float,
    ) {
        layoutEngine.setFirstLineIndent(enabled, widthChars)
    }

    /**
     * #624 评论7：普通 mirror 内容变化 — 不再每次重建 display projection。
     *
     * - 普通正文：更新 identity projection 的 revision/offset 映射，继续复用
     *   现有 DynamicLayout；只有影响段落结构的编辑才增量维护首行缩进 span。
     * - secret 显示模式：真正改变显示文本的投影，重建 masked projection。
     */
    fun onMirrorContentChanged(patches: List<DisplayPatch> = emptyList()) {
        if (secretDisplayMode) {
            rebuildProjectionContent()
        } else {
            currentProjection = DisplayTextProjection.identity(mirror.getText())
            layoutEngine.onMirrorContentChanged(currentProjection, patches)
        }
        layoutEngine.requestLayout()
    }

    fun applyProjection(projection: DisplayTextProjection) {
        currentProjection = projection
        layoutEngine.applyDisplaySource(if (projection.isMasked) projection.displayText else null, projection)
        layoutEngine.requestLayout()
    }

    fun clearProjection() {
        currentProjection = DisplayTextProjection.identity(mirror.getText())
        layoutEngine.applyDisplaySource(null, currentProjection)
        layoutEngine.requestLayout()
    }

    fun getCurrentProjection(): DisplayTextProjection = currentProjection

    fun setSecretDisplayMode(enabled: Boolean) {
        if (secretDisplayMode != enabled) {
            secretDisplayMode = enabled
        }
    }

    fun isSecretDisplayMode(): Boolean = secretDisplayMode

    fun rebuildDisplayProjection() {
        rebuildProjectionContent()
        layoutEngine.requestLayout()
    }

    fun applySecretDisplayIfActive() {
        rebuildDisplayProjection()
    }

    fun applySecretDisplayIfActiveWithLayout() {
        rebuildDisplayProjection()
    }

    private fun rebuildProjectionContent() {
        val text = mirror.getText()
        currentProjection =
            if (secretDisplayMode) {
                val compRange = mirror.getCompositionRangeUtf16()
                if (compRange != null && compRange.first >= 0 && compRange.second > compRange.first) {
                    val compText = mirror.getSpannable().substring(compRange.first, compRange.second)
                    DisplayTextProjection.maskedWithComposition(text, compRange.first, compRange.second, compText)
                } else {
                    DisplayTextProjection.masked(text)
                }
            } else {
                DisplayTextProjection.identity(text)
            }
        layoutEngine.applyDisplaySource(
            if (currentProjection.isMasked) currentProjection.displayText else null,
            currentProjection,
        )
    }

    fun captureLineBitmapSnapshotsWithClusters(
        lineIndices: Set<Int>,
    ): Map<Int, com.xiwei.sujian.feature.editor.layout.AndroidLineSnapshot> =
        layoutEngine.captureLineBitmapSnapshotsWithClusters(lineIndices)

    fun getLineForUtf8(byteOffset: Int): Int = layoutEngine.getLineForUtf8(byteOffset)

    fun getCursorLine(): Int = layoutEngine.getCursorLine()

    fun getPrimaryHorizontalUtf8(byteOffset: Int): Float = layoutEngine.getPrimaryHorizontalUtf8(byteOffset)
}
