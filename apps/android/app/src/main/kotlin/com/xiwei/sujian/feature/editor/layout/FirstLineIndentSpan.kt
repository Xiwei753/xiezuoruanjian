package com.xiwei.sujian.feature.editor.layout

import android.graphics.Canvas
import android.graphics.Paint
import android.text.Layout
import android.text.style.LeadingMarginSpan
import android.text.style.UpdateLayout

/**
 * #624 评论3：写作软件式“自动首行缩进”的显示层 span。
 *
 * 不能直接用 [android.text.style.LeadingMarginSpan.Standard]：Android 官方
 * `DynamicLayout` 会随 Editable 的**文本**变化自动 reflow，但它的 `SpanWatcher`
 * 对 span 的新增/删除/移动只在该 span 实现 [UpdateLayout] 时触发 reflow。
 * `LeadingMarginSpan.Standard` 只实现 paragraph style，不实现 `UpdateLayout`；
 * 若在正文修改后再 removeSpan/setSpan，换行、合段后 margin span 虽然变了，
 * DynamicLayout 不一定重新计算对应段落宽度。
 *
 * 本 span 同时实现 [LeadingMarginSpan] + [UpdateLayout]，配合
 * [Spanned.SPAN_PARAGRAPH] 使用：
 * - span 变化（增删/端点移动）会让 DynamicLayout 重新 reflow 对应段落；
 * - `SPAN_PARAGRAPH` 保证端点必须在正文边界或 `\n` 后，删除锚点换行时端点
 *   自动拉到下一个 `\n`（或文末），与 [ParagraphStyleProjection] 的增量重同步
 *   互补。
 *
 * 只作用于显示层：不插入空格、不改变 UTF-8/UTF-16 offset、不污染保存和同步正文。
 */
internal class FirstLineIndentSpan(
    private val firstLinePx: Int,
) : LeadingMarginSpan, UpdateLayout {
    override fun getLeadingMargin(first: Boolean): Int = if (first) firstLinePx else 0

    override fun drawLeadingMargin(
        c: Canvas,
        p: Paint,
        x: Int,
        dir: Int,
        top: Int,
        baseline: Int,
        bottom: Int,
        text: CharSequence,
        start: Int,
        end: Int,
        first: Boolean,
        layout: Layout?,
    ) = Unit
}
