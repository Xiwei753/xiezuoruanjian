package com.xiwei.sujian.feature.editor.layout

import android.text.SpannableStringBuilder
import android.text.TextPaint
import android.text.style.UpdateLayout
import com.xiwei.sujian.feature.editor.projection.DisplayPatch
import com.xiwei.sujian.feature.editor.projection.DisplayTextMirror
import com.xiwei.sujian.feature.editor.projection.DisplayTextProjection
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotSame
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #624 评论3：受影响区域捕获与稳定后缀锚点契约测试。
 *
 * - 普通编辑只抓编辑所在段落（及相邻段落）的视觉行，不复制整章 lineRanges；
 * - onMirrorContentChanged 不再推进布局（布局推进所有权在动画引擎，一次编辑
 *   只推进一次）；
 * - 尾部空段落的光标 X 手动补首行缩进（AOSP getParagraphSpans 不覆盖空段落）；
 * - 删除换行合段时稳定后缀锚点携带真实 deltaY（block shift 数据源）；
 * - FirstLineIndentSpan 是 LeadingMarginSpan + UpdateLayout，SPAN_PARAGRAPH
 *   端点随锚点换行自动跟随。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class AffectedLayoutRevisionTest {
    private companion object {
        const val THREE_PARAGRAPHS = "abc\ndef\nghi"
    }

    private fun newEngine(text: String): Pair<DisplayTextMirror, AndroidLayoutEngine> {
        val mirror = DisplayTextMirror()
        mirror.loadText(text, text.toByteArray(Charsets.UTF_8).size)
        val paint =
            TextPaint().apply {
                textSize = 32f
            }
        val engine = AndroidLayoutEngine(mirror, paint)
        engine.setWidth(500f)
        engine.requestLayout()
        return mirror to engine
    }

    @Test
    fun midParagraphInsert_capturesOnlyEditParagraphLines() {
        val (mirror, engine) = newEngine(THREE_PARAGRAPHS)
        val layout = engine.getLayout()!!

        // 普通按键：第一段中部插入一个字符 — 只受影响段落（第一段）的视觉行。
        mirror.getSpannable().insert(1, "X")
        val rev =
            engine.captureAffectedRevision(
                editStartUtf8 = 1,
                editEndUtf8 = 1,
                includeNextParagraph = false,
            )!!

        assertEquals("受影响行数 = 编辑段落行数，不是整章", 1, rev.affectedLineCount)
        assertEquals("起始行必须是编辑段落首行", 0, rev.firstAffectedLineIndex)
        assertEquals("lineCount 是整章行数（O(1) 查询用）", layout.lineCount, rev.lineCount)
        assertTrue("受影响行必须在快照内可查", rev.lineRangeAt(0) != null)
        assertNull("快照外行必须返回 null", rev.lineRangeAt(layout.lineCount))
    }

    @Test
    fun insertAtEndAfterNewline_capturesTrailingEmptyParagraphLine() {
        val (mirror, engine) = newEngine("ab\n")

        // 光标在尾部空段落（offset 3，正文以 \n 结尾）：受影响的是空段落所在行。
        val rev =
            engine.captureAffectedRevision(
                editStartUtf8 = 3,
                editEndUtf8 = 3,
                includeNextParagraph = false,
            )!!

        val layout = engine.getLayout()!!
        assertEquals("必须捕获尾部空行", 1, rev.affectedLineCount)
        assertEquals("尾部空行是最后一行", layout.lineCount - 1, rev.firstAffectedLineIndex)
    }

    @Test
    fun onMirrorContentChanged_doesNotAdvanceLayout() {
        val (mirror, engine) = newEngine(THREE_PARAGRAPHS)
        val rev1 =
            engine.captureAffectedRevision(
                editStartUtf8 = 1,
                editEndUtf8 = 1,
                includeNextParagraph = false,
            )!!

        // 普通 mirror 内容变化：不推进布局（不再自己 requestLayout）。
        mirror.getSpannable().replace(1, 2, "X")
        engine.onMirrorContentChanged(
            DisplayTextProjection.identity(mirror.getText()),
            listOf(
                DisplayPatch(
                    baseRevision = 0L,
                    newRevision = 1L,
                    replaceByteStart = 1,
                    replaceByteEndExclusive = 2,
                    insertedText = "X",
                    resultingSelectionStart = 2,
                    resultingSelectionEnd = 2,
                ),
            ),
        )

        assertSame("onMirrorContentChanged 不得推进 revision", rev1, engine.getCurrentAffectedRevision())

        // 只有 captureAffectedRevision 才是布局推进点。
        val rev2 =
            engine.captureAffectedRevision(
                editStartUtf8 = 1,
                editEndUtf8 = 1,
                includeNextParagraph = false,
            )!!
        assertNotSame("captureAffectedRevision 必须产生新 revision", rev1, rev2)
        assertTrue("layoutRevision 必须递增", rev2.layoutRevision > rev1.layoutRevision)
    }

    @Test
    fun trailingEmptyParagraphCursorX_addsFirstLineIndent() {
        val (mirror, engine) = newEngine("ab\n")

        engine.setFirstLineIndent(true, 2f)
        engine.requestLayout()
        val layout = engine.getLayout()!!
        val indentPx = engine.getFirstLineIndentPxForTest()
        val rev =
            engine.captureAffectedRevision(
                editStartUtf8 = 3,
                editEndUtf8 = 3,
                includeNextParagraph = false,
            )!!

        val baseX = layout.getPrimaryHorizontal(3)
        assertEquals("尾部空段落光标 X = 布局 X + 首行缩进", baseX + indentPx, rev.cursorX, 0.5f)

        // 关闭首行缩进：光标回到布局几何。
        engine.setFirstLineIndent(false, 2f)
        engine.requestLayout()
        val rev2 =
            engine.captureAffectedRevision(
                editStartUtf8 = 3,
                editEndUtf8 = 3,
                includeNextParagraph = false,
            )!!
        assertEquals("关闭缩进后光标 X 回到布局几何", engine.getLayout()!!.getPrimaryHorizontal(3), rev2.cursorX, 0.5f)
    }

    @Test
    fun deleteNewline_suffixAnchorCarriesDeltaY() {
        val (mirror, engine) = newEngine("abc\ndef\nghi")
        val layout = engine.getLayout()!!
        val lineHeight = layout.getLineTop(1) - layout.getLineTop(0)

        // old 侧：删除 [2,4)（"c\n"），受影响 = "abc\n" + 相邻 "def\n"；
        // 稳定后缀 = "ghi"（old 坐标 8）。
        val oldRev =
            engine.captureAffectedRevision(
                editStartUtf8 = 2,
                editEndUtf8 = 4,
                includeNextParagraph = true,
            )!!
        assertEquals("old 侧受影响行 = 两段", 2, oldRev.affectedLineCount)
        val oldAnchor = oldRev.stableSuffixAnchor!!
        assertEquals("old 侧后缀锚点 = \"ghi\" 起点", 8, oldAnchor.startUtf8)

        // 应用删除：new 侧受影响 = 合并段 "abdef"；稳定后缀 = "ghi"（new 坐标 6）。
        mirror.loadFromSnapshot("abdef\nghi", 9, 1L)
        val newRev =
            engine.captureAffectedRevision(
                editStartUtf8 = 2,
                editEndUtf8 = 2,
                includeNextParagraph = false,
                previousRevision = oldRev,
            )!!
        assertEquals("new 侧受影响行 = 合并段一行", 1, newRev.affectedLineCount)
        val newAnchor = newRev.stableSuffixAnchor!!
        assertEquals("new 侧后缀锚点 = \"ghi\" 起点", 6, newAnchor.startUtf8)
        assertEquals("后缀整体上移一行", -lineHeight.toFloat(), newAnchor.deltaY, 0.5f)
    }

    @Test
    fun suffixAnchor_droppedWhenContentDoesNotCorrespond() {
        val (mirror, engine) = newEngine("abc\ndef")
        val oldRev =
            engine.captureAffectedRevision(
                editStartUtf8 = 2,
                editEndUtf8 = 4,
                includeNextParagraph = true,
            )!!
        assertNull("old 侧无后缀（区域后无内容）", oldRev.stableSuffixAnchor)

        // 替换整段内容：两侧净长度变化与锚点位移不一致 — 锚点必须丢弃。
        mirror.loadFromSnapshot("abzzz", 5, 1L)
        val newRev =
            engine.captureAffectedRevision(
                editStartUtf8 = 2,
                editEndUtf8 = 5,
                includeNextParagraph = false,
                previousRevision = oldRev,
            )!!
        assertNull("内容不对应时不得生成后缀锚点", newRev.stableSuffixAnchor)
    }

    @Test
    fun firstLineIndentSpan_isUpdateLayout() {
        val span = FirstLineIndentSpan(32)
        assertTrue("必须同时实现 UpdateLayout（DynamicLayout 才会在 span 变化时 reflow）", span is UpdateLayout)
        assertEquals("首行缩进像素", 32, span.getLeadingMargin(true))
        assertEquals("续行不缩进", 0, span.getLeadingMargin(false))
    }

    @Test
    fun paragraphSpan_followsAnchorNewlineOnDelete() {
        // SPAN_PARAGRAPH：删除锚点换行后，端点自动拉到下一个 \n（或文末）。
        val text = SpannableStringBuilder("ab\ncd")
        val span = FirstLineIndentSpan(32)
        text.setSpan(span, 0, 3, SpannableStringBuilder.SPAN_PARAGRAPH)

        text.delete(2, 3) // 删除锚点换行 → "abcd"

        assertEquals("合段后 span 端点自动跟随到文末", 0, text.getSpanStart(span))
        assertEquals(4, text.getSpanEnd(span))
    }

    /**
     * #637 评论 5386066978 项1：正文从 1 个字符删到 0 后，cursorX 必须等于
     * 首行缩进像素（与随后输入第一个字符前的起点一致），不得出现双倍缩进。
     *
     * 旧路径：resyncParagraphIndent 在 text.length==0 时直接 return，不清塌缩成
     * 0..0 的 span → Layout.getPrimaryHorizontal(0) 已含一次缩进，
     * AffectedLineCapture 再手工补一次 → cursorX = 2 × indentPx。
     * 修复后：resyncParagraphIndent 清掉塌缩 span → Layout.getPrimaryHorizontal(0)=0，
     * hasFirstLineIndentSpanAt=false → 手工补一次 → cursorX = indentPx。
     */
    @Test
    fun deleteToEmpty_cursorXEqualsSingleIndentNotDouble() {
        val (mirror, engine) = newEngine("字")
        engine.setFirstLineIndent(true, 2f)
        engine.requestLayout()
        val indentPx = engine.getFirstLineIndentPxForTest()
        assertTrue("缩进像素必须为正", indentPx > 0f)

        // 删掉最后一个字符 → 正文为空，原 span 塌缩成 0..0。
        mirror.getSpannable().delete(0, 1)
        assertEquals("", mirror.getText())
        // onMirrorContentChanged 触发 resyncParagraphIndent 清掉塌缩 span。
        engine.onMirrorContentChanged(
            DisplayTextProjection.identity(mirror.getText()),
            listOf(
                DisplayPatch(
                    baseRevision = 0L,
                    newRevision = 1L,
                    replaceByteStart = 0,
                    replaceByteEndExclusive = 3,
                    insertedText = "",
                    resultingSelectionStart = 0,
                    resultingSelectionEnd = 0,
                ),
            ),
        )
        // 塌缩 span 必须已被清掉。
        assertTrue(
            "删空后不得残留 0..0 FirstLineIndentSpan",
            mirror.getSpannable().getSpans(0, 0, FirstLineIndentSpan::class.java).isEmpty(),
        )

        val rev =
            engine.captureAffectedRevision(
                editStartUtf8 = 0,
                editEndUtf8 = 0,
                includeNextParagraph = false,
            )!!

        assertEquals(
            "删空后 cursorX 必须等于单倍首行缩进（与输入首字符前起点一致），不得双倍",
            indentPx,
            rev.cursorX,
            0.5f,
        )
    }
}
