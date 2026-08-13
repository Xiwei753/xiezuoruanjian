package com.xiwei.sujian.feature.editor.layout

import android.text.Spannable
import android.text.TextPaint
import android.text.TextUtils
import android.text.style.LeadingMarginSpan

/**
 * #624 评论3：写作软件式“自动首行缩进”的显示层投影 — 只负责平台显示样式，
 * 不改正文字符串。
 *
 * 每个真实段落（正文开头 + 每个 `\n` 之后的下一个字符位置）应用
 * [LeadingMarginSpan.Standard]：第一视觉行按全角字符宽度 × [widthChars] 缩进，
 * 自动换行后的第二、第三视觉行从正常左边界开始（后续行 margin 为 0）。
 * Span 只作用于显示层，不插入两个空格、不改变 UTF-8/UTF-16 offset、
 * 不污染保存和同步正文。
 *
 * 维护策略（配合 [AndroidLayoutEngine]）：
 * - 布局配置变化（开关/宽度）或整篇重载时做整篇重同步（[applyFirstLineIndent]）；
 * - 正文编辑影响段落结构时（插入/删除包含 `\n`、替换选区、段落边界插入）
 *   做受影响段落区域的增量重同步（[resyncParagraphIndent]）；
 * - 普通按键（段落内部增删字符）不触碰 span，段落起点的 span 用
 *   SPAN_INCLUSIVE_EXCLUSIVE 锚定：在段落起点插入文本时 span 起点不漂移，
 *   在段落末尾（`\n` 处）插入文本时 span 不越过下一个段落起点。
 *
 * 每个段落使用独立的 span 实例（复用同一实例会让 setSpan 只更新一个 range，
 * 其余 range 保持旧边界，段落缩进会错位）。
 */
class ParagraphStyleProjection {
    /**
     * 全文档重同步：移除全部首行缩进 span，再按当前段落结构重新应用。
     * [enabled] 为 false 或 [widthChars] <= 0 时只做移除（清空历史 span）。
     */
    fun applyFirstLineIndent(
        text: Spannable,
        enabled: Boolean,
        widthChars: Float,
        textPaint: TextPaint,
    ) {
        removeParagraphIndentSpans(text, 0, text.length)
        if (!enabled || widthChars <= 0f) return
        applyParagraphIndent(text, 0, text.length, firstLineIndentPx(textPaint, widthChars))
    }

    /**
     * 受影响段落区域的增量重同步：移除区域（自动扩展到区域起点所在段落起点、
     * 区域终点所在段落终点）内的全部首行缩进 span，再重新应用该区域内每个
     * 段落起点的缩进。区域外的 span 原样保留 — 普通按键（段落内部增删字符）
     * 不经过本方法，不产生任何 span 抖动。
     */
    fun resyncParagraphIndent(
        text: Spannable,
        regionStart: Int,
        regionEnd: Int,
        enabled: Boolean,
        firstLinePx: Float,
    ) {
        if (regionStart < 0 || regionEnd < regionStart || text.length == 0) return
        val start = paragraphStartOf(text, regionStart.coerceAtMost(text.length))
        // 纯删除（regionStart == regionEnd）时被删字符位于该位置，扩展一位以覆盖
        // 合并后的整个段落；否则从变更区域的末尾继续到所在段落终点。
        val endOffset =
            if (regionEnd > regionStart) {
                regionEnd.coerceAtMost(text.length)
            } else {
                (regionEnd + 1).coerceAtMost(text.length)
            }
        val end = paragraphEndExclusive(text, endOffset)
        if (start >= end) return
        removeParagraphIndentSpans(text, start, end)
        if (!enabled || firstLinePx <= 0f) return
        applyParagraphIndent(text, start, end, firstLinePx)
    }

    /** 移除整篇文本的全部首行缩进 span。 */
    fun clearParagraphIndent(text: Spannable) {
        removeParagraphIndentSpans(text, 0, text.length)
    }

    /** 全角字符宽度 × 字符数 = 首行缩进像素。用典型全角字符 `中` 度量。 */
    fun firstLineIndentPx(
        textPaint: TextPaint,
        widthChars: Float,
    ): Float = textPaint.measureText(FULL_WIDTH_PROBE) * widthChars

    /** [offset] 所在段落的起点（offset 在段落内任意位置均回到段落开头）。 */
    fun paragraphStartOf(
        text: Spannable,
        offset: Int,
    ): Int {
        if (offset <= 0) return 0
        val prevBreak = TextUtils.lastIndexOf(text, '\n', offset - 1)
        return if (prevBreak < 0) 0 else prevBreak + 1
    }

    /** [offset] 所在段落的终点（不含）— 段落文本 + 结尾 `\n`（若有）。 */
    fun paragraphEndExclusive(
        text: Spannable,
        offset: Int,
    ): Int {
        val nextBreak = TextUtils.indexOf(text, '\n', offset)
        return if (nextBreak < 0) text.length else nextBreak + 1
    }

    private fun removeParagraphIndentSpans(
        text: Spannable,
        start: Int,
        end: Int,
    ) {
        val spans = text.getSpans(start, end, LeadingMarginSpan.Standard::class.java)
        for (span in spans) {
            text.removeSpan(span)
        }
    }

    private fun applyParagraphIndent(
        text: Spannable,
        regionStart: Int,
        regionEnd: Int,
        firstLinePx: Float,
    ) {
        if (regionStart >= regionEnd || text.length == 0) return
        // 段落起点 = 文本开头 + 每个 '\n' 之后的位置。regionStart 已是段落起点
        // （调用方保证），从它开始逐段应用。
        var position = regionStart
        while (position in regionStart until regionEnd) {
            text.setSpan(
                LeadingMarginSpan.Standard(firstLinePx.toInt(), 0),
                position,
                paragraphEndExclusive(text, position),
                Spannable.SPAN_INCLUSIVE_EXCLUSIVE,
            )
            val nextBreak = TextUtils.indexOf(text, '\n', position)
            if (nextBreak < 0 || nextBreak + 1 >= regionEnd) break
            position = nextBreak + 1
        }
    }

    private companion object {
        const val FULL_WIDTH_PROBE = "中"
    }
}
