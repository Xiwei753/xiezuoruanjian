package com.xiwei.sujian.feature.editor.pipeline

import android.text.Layout
import android.text.TextPaint
import com.xiwei.sujian.feature.editor.layout.AffectedLayoutRevision
import com.xiwei.sujian.feature.editor.layout.AndroidLayoutEngine
import com.xiwei.sujian.feature.editor.layout.LayoutRevisionSource
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

    /**
     * #624 评论4: 长期 identity projection — 引用 mirror 的增量 index 和 spannable，
     * 不每键重建。index 由 mirror.applyPatches 增量更新，spannable 自动反映最新内容。
     */
    private var identityProjection: DisplayTextProjection =
        DisplayTextProjection.identityFromIndex(mirror.getTextOffsetIndex(), mirror.getSpannable())
    private var currentProjection: DisplayTextProjection = identityProjection
    private var secretDisplayMode: Boolean = false

    /** 全量布局推进（配置变化/加载路径）。 */
    fun requestLayout() {
        layoutEngine.requestLayout()
    }

    /** 布局配置推进 — 不构建 revision（普通编辑路径由动画引擎推进）。 */
    fun ensureLayoutConfig() {
        layoutEngine.ensureLayoutConfig()
    }

    fun getLayout(): Layout? = layoutEngine.getLayout()

    fun getCurrentRevision(): LayoutRevisionSource? = layoutEngine.getCurrentRevision()

    fun getCurrentAffectedRevision(): AffectedLayoutRevision? = layoutEngine.getCurrentAffectedRevision()

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
     * #624 评论3/7：普通 mirror 内容变化 — 不再每次重建 display projection，
     * 也**不再自己推进布局**（布局推进所有权在动画引擎
     * [AndroidTextAnimationEngine.prepareAndSubmit]，一次编辑只推进一次；
     * 无动画兜底路径由调用方显式 [requestLayout]）。
     *
     * - 普通正文：更新 identity projection 的 revision/offset 映射，继续复用
     *   现有 DynamicLayout；只有影响段落结构的编辑才增量维护首行缩进 span。
     * - secret 显示模式：真正改变显示文本的投影，重建 masked projection。
     */
    fun onMirrorContentChanged(patches: List<DisplayPatch> = emptyList()) {
        if (secretDisplayMode) {
            rebuildProjectionContent()
        } else {
            // #624 评论4: 复用 identityProjection — index 已由 mirror.applyPatches 增量更新，
            // spannable 自动反映最新内容，不每键 DisplayTextProjection.identity(getText())。
            currentProjection = identityProjection
            layoutEngine.onMirrorContentChanged(currentProjection, patches)
        }
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
                // #624 评论4: 非 secret 路径复用 identityProjection，不拷贝整章。
                identityProjection
            }
        layoutEngine.applyDisplaySource(
            if (currentProjection.isMasked) currentProjection.displayText.toString() else null,
            currentProjection,
        )
    }

    /**
     * #624 评论3：受影响区域捕获（普通编辑路径）— 只抓编辑所在段落及相邻段落
     * 的视觉行 + 光标/选区几何 + 稳定后缀锚点。new 侧传入 old 侧快照计算后缀
     * deltaY。捕获即布局推进（revisionCounter 递增）。
     */
    fun captureAffectedRevision(
        editStartUtf8: Int,
        editEndUtf8: Int,
        includeNextParagraph: Boolean,
        previousRevision: AffectedLayoutRevision? = null,
    ): AffectedLayoutRevision? =
        layoutEngine.captureAffectedRevision(
            editStartUtf8,
            editEndUtf8,
            includeNextParagraph,
            previousRevision,
        )

    fun captureLineBitmapSnapshotsWithClusters(
        revision: AffectedLayoutRevision,
    ): Map<Int, com.xiwei.sujian.feature.editor.layout.AndroidLineSnapshot> =
        layoutEngine.captureLineBitmapSnapshotsWithClusters(revision)
}
