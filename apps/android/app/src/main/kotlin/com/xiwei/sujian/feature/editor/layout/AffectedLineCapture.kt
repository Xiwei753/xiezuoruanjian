package com.xiwei.sujian.feature.editor.layout

import android.text.Layout
import android.text.Spanned
import android.text.TextUtils
import com.xiwei.sujian.feature.editor.projection.DisplayTextMirror
import com.xiwei.sujian.feature.editor.projection.DisplayTextProjection

/**
 * #624 评论3：普通编辑路径的“受影响区域”行捕获 — 编辑前/后只抓编辑所在段落
 * （及删除/合段时的相邻段落）的视觉行，不遍历整章。
 *
 * 段落定位基于显示文本（masked 投影保留 `\n`，段落结构与真实正文一致）；
 * 行号使用 Layout 的绝对行号（`getLineForOffset()` 定位，O(log n)）。
 */
internal class AffectedLineCapture {
    /** 捕获上下文 — 一次编辑的布局/投影/缩进设置快照。 */
    class CaptureParams(
        val layout: Layout,
        val layoutText: CharSequence,
        val projection: DisplayTextProjection,
        val mirror: DisplayTextMirror,
        val firstLineIndentEnabled: Boolean,
        val firstLineIndentWidthChars: Float,
        val firstLineIndentPx: Float,
    )

    data class Result(
        val firstAffectedLineIndex: Int,
        val affectedLines: List<AndroidLayoutRevision.LineRange>,
        val affectedEndUtf16: Int,
        val cursorGeometry: CursorGeometry,
    )

    data class CursorGeometry(
        val cursorUtf8: Int,
        val cursorUtf16: Int,
        val cursorX: Float,
        val cursorY: Float,
        val cursorHeight: Float,
        val selectionAnchorUtf8: Int,
        val selectionHeadUtf8: Int,
        val selectionAnchorUtf16: Int,
        val selectionHeadUtf16: Int,
        val compositionStartUtf16: Int,
        val compositionEndUtf16: Int,
    )

    fun capture(
        params: CaptureParams,
        regionStartUtf16: Int,
        regionEndUtf16: Int,
        includeNextParagraph: Boolean,
    ): Result? {
        val l = params.layout
        val layoutText = params.layoutText
        val textLen = layoutText.length
        val safeStart = regionStartUtf16.coerceIn(0, textLen)
        val safeEnd = regionEndUtf16.coerceIn(0, textLen)

        val affectedParagraphStarts =
            collectAffectedParagraphStarts(layoutText, safeStart, safeEnd, includeNextParagraph)
        val (firstAffectedLineIndex, affectedLines) =
            buildAffectedLines(l, layoutText, params.projection, affectedParagraphStarts)
        if (affectedLines.isEmpty()) return null

        val affectedEndUtf16 = paragraphEndExclusiveUtf16(layoutText, affectedParagraphStarts.last())
        return Result(
            firstAffectedLineIndex = firstAffectedLineIndex,
            affectedLines = affectedLines,
            affectedEndUtf16 = affectedEndUtf16,
            cursorGeometry = buildCursorGeometry(params),
        )
    }

    /**
     * 受影响段落集合：从区域起点所在段落起逐段推进到区域终点，再加相邻段落
     * （old 侧删除/替换时区域后的下一段可能合段；new 侧区域终点紧跟 `\n` 时
     * 新段落内容来自旧编辑段）。
     */
    private fun collectAffectedParagraphStarts(
        layoutText: CharSequence,
        safeStart: Int,
        safeEnd: Int,
        includeNextParagraph: Boolean,
    ): List<Int> {
        val starts = sortedSetOf<Int>()
        var paraStart = paragraphStartOfUtf16(layoutText, safeStart)
        while (paraStart < safeEnd) {
            starts.add(paraStart)
            val nextBreak = TextUtils.indexOf(layoutText, '\n', paraStart)
            if (nextBreak < 0) break
            paraStart = nextBreak + 1
        }
        if (starts.isEmpty()) {
            // 区域为空区间：受影响段落 = 区域起点所在段落（空文档时为段落 0，
            // 光标位于尾部空段落时是该空段落本身）。
            starts.add(paragraphStartOfUtf16(layoutText, safeStart))
        }
        if (safeEnd < layoutText.length) {
            val nextParagraphStart = paragraphStartOfUtf16(layoutText, safeEnd + 1)
            if (nextParagraphStart !in starts &&
                (includeNextParagraph || layoutText[safeEnd] == '\n')
            ) {
                starts.add(nextParagraphStart)
            }
        }
        return starts.toList()
    }

    /** 逐段捕获视觉行（绝对行号 + LineRange），段落 id 在捕获范围内递增。 */
    private fun buildAffectedLines(
        l: Layout,
        layoutText: CharSequence,
        projection: DisplayTextProjection,
        affectedParagraphStarts: List<Int>,
    ): Pair<Int, List<AndroidLayoutRevision.LineRange>> {
        val affectedLines = mutableListOf<AndroidLayoutRevision.LineRange>()
        var firstAffectedLineIndex = -1
        var paragraphId = 0
        for (ps in affectedParagraphStarts) {
            val pe = paragraphEndExclusiveUtf16(layoutText, ps)
            val firstLine = l.getLineForOffset(ps)
            val lastOffset = (pe - 1).coerceAtLeast(ps)
            val lastLine = l.getLineForOffset(lastOffset)
            var localLineIndex = 0
            for (i in firstLine..lastLine) {
                val lineRange = buildLineRange(l, i, layoutText, projection, paragraphId, localLineIndex)
                if (firstAffectedLineIndex < 0) firstAffectedLineIndex = i
                affectedLines.add(lineRange)
                if (lineRange.endsWithHardBreak) {
                    paragraphId++
                    localLineIndex = 0
                } else {
                    localLineIndex++
                }
            }
        }
        return Pair(firstAffectedLineIndex, affectedLines)
    }

    fun paragraphStartOfUtf16(
        text: CharSequence,
        offset: Int,
    ): Int {
        if (offset <= 0) return 0
        val prevBreak = TextUtils.lastIndexOf(text, '\n', offset - 1)
        return if (prevBreak < 0) 0 else prevBreak + 1
    }

    fun paragraphEndExclusiveUtf16(
        text: CharSequence,
        offset: Int,
    ): Int {
        val nextBreak = TextUtils.indexOf(text, '\n', offset)
        return if (nextBreak < 0) text.length else nextBreak + 1
    }

    private fun buildLineRange(
        l: Layout,
        i: Int,
        layoutText: CharSequence,
        projection: DisplayTextProjection,
        paragraphId: Int,
        paragraphLocalLineIndex: Int,
    ): AndroidLayoutRevision.LineRange {
        val lineStartUtf16 = l.getLineStart(i)
        val lineEndUtf16 = l.getLineEnd(i)
        val startUtf8 = projection.displayUtf16ToRealUtf8(lineStartUtf16)
        val endUtf8 = projection.displayUtf16ToRealUtf8(lineEndUtf16)
        val endsWithHardBreak =
            lineEndUtf16 > 0 && lineEndUtf16 <= layoutText.length &&
                layoutText[lineEndUtf16 - 1] == '\n'
        return AndroidLayoutRevision.LineRange(
            startUtf8 = startUtf8,
            endUtf8 = endUtf8,
            startUtf16 = lineStartUtf16,
            endUtf16 = lineEndUtf16,
            top = l.getLineTop(i).toFloat(),
            bottom = l.getLineBottom(i).toFloat(),
            baseline = l.getLineBaseline(i).toFloat(),
            left = l.getLineLeft(i),
            right = l.getLineRight(i),
            endsWithHardBreak = endsWithHardBreak,
            paragraphId = paragraphId,
            paragraphLocalLineIndex = paragraphLocalLineIndex,
        )
    }

    private fun buildCursorGeometry(params: CaptureParams): CursorGeometry {
        val l = params.layout
        val layoutText = params.layoutText
        val projection = params.projection
        val mirror = params.mirror
        val cursorDisplayUtf16 = projection.realUtf8ToDisplayUtf16(mirror.getCursorUtf8())
        val cursorLine =
            if (cursorDisplayUtf16 in 0..layoutText.length) {
                l.getLineForOffset(cursorDisplayUtf16)
            } else {
                0
            }
        var cursorX =
            if (cursorDisplayUtf16 in 0..layoutText.length) {
                l.getPrimaryHorizontal(cursorDisplayUtf16)
            } else {
                0f
            }
        // #624 评论3：正文末尾的空段落（start == end && start > 0）在
        // `Layout.getParagraphSpans` 中拿不到任何 span（AOSP 明确不为尾部空段落
        // 返回段落 span，避免误继承上一段样式）。该空段落的光标显示 X 手动补上
        // 首行缩进；用户输入第一个字符后，真实段落 span 接管，光标重新完全由
        // Layout geometry 给出。不往正文塞空格或零宽字符。
        //
        // #637 评论 5386066978 项1：补之前先看当前位置是否已经存在
        // `FirstLineIndentSpan`（删空正文这一帧 Layout 可能已从残留 span 得到
        // 一次缩进，或 `ParagraphStyleProjection.resyncParagraphIndent` 已清掉
        // 残塌缩 span 后 Layout 还在过渡）。已有就信 Layout，只有缺失时才手工
        // 补一次 — 避免空正文光标 = Layout 缩进 + 手工缩进 = 两倍缩进。
        val firstLineIndentActive = params.firstLineIndentEnabled && params.firstLineIndentWidthChars > 0f
        if (firstLineIndentActive &&
            cursorAtEmptyTrailingParagraph(layoutText, cursorDisplayUtf16) &&
            !hasFirstLineIndentSpanAt(l, cursorDisplayUtf16)
        ) {
            cursorX += params.firstLineIndentPx
        }
        val cursorY = l.getLineTop(cursorLine).toFloat()
        val cursorHeight = (l.getLineBottom(cursorLine) - l.getLineTop(cursorLine)).toFloat()

        val compRange = mirror.getCompositionRangeUtf16()
        val compStartDisplayUtf16: Int
        val compEndDisplayUtf16: Int
        if (compRange != null && compRange.first >= 0 && compRange.second >= 0) {
            compStartDisplayUtf16 = projection.realUtf16ToDisplayUtf16(compRange.first)
            compEndDisplayUtf16 = projection.realUtf16ToDisplayUtf16(compRange.second)
        } else {
            compStartDisplayUtf16 = compRange?.first ?: -1
            compEndDisplayUtf16 = compRange?.second ?: -1
        }

        return CursorGeometry(
            cursorUtf8 = mirror.getCursorUtf8(),
            cursorUtf16 = cursorDisplayUtf16,
            cursorX = cursorX,
            cursorY = cursorY,
            cursorHeight = cursorHeight,
            selectionAnchorUtf8 = mirror.getSelectionAnchorUtf8(),
            selectionHeadUtf8 = mirror.getSelectionHeadUtf8(),
            selectionAnchorUtf16 = projection.realUtf8ToDisplayUtf16(mirror.getSelectionAnchorUtf8()),
            selectionHeadUtf16 = projection.realUtf8ToDisplayUtf16(mirror.getSelectionHeadUtf8()),
            compositionStartUtf16 = compStartDisplayUtf16,
            compositionEndUtf16 = compEndDisplayUtf16,
        )
    }

    /** 光标位于正文末尾的空段落起点（文本为空，或最后一个字符是 `\n`）。 */
    private fun cursorAtEmptyTrailingParagraph(
        layoutText: CharSequence,
        cursorDisplayUtf16: Int,
    ): Boolean {
        if (cursorDisplayUtf16 != layoutText.length) return false
        return layoutText.isEmpty() || layoutText[cursorDisplayUtf16 - 1] == '\n'
    }

    /**
     * #637 评论 5386066978 项1：当前位置是否已存在 [FirstLineIndentSpan]。
     *
     * 用 `Spanned.getSpans` 查询光标所在行范围的 paragraph span。
     * AOSP 对尾部空段落（start == end && start > 0）不返回任何段落 span，
     * 所以这种情况下返回 false — 调用方据此决定是否手工补一次缩进。
     * 对非空段落（包括刚删空但 span 尚未塌缩/清除的过渡帧）返回 true，
     * 让调用方信 Layout，不再叠加手工缩进。
     */
    private fun hasFirstLineIndentSpanAt(
        layout: Layout,
        cursorDisplayUtf16: Int,
    ): Boolean {
        if (cursorDisplayUtf16 < 0 || cursorDisplayUtf16 > layout.text.length) return false
        val line = layout.getLineForOffset(cursorDisplayUtf16)
        if (line < 0 || line >= layout.lineCount) return false
        val lineStart = layout.getLineStart(line)
        val lineEnd = layout.getLineEnd(line)
        // 尾部空段落（start == end）：AOSP getParagraphSpans 不返回任何段落 span，
        // 与之对齐返回 false，让调用方手工补一次缩进。
        if (lineStart == lineEnd) return false
        val spanned = layout.text as? Spanned ?: return false
        val spans = spanned.getSpans(lineStart, lineEnd, FirstLineIndentSpan::class.java)
        return spans.isNotEmpty()
    }
}
