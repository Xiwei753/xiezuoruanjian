package com.xiwei.sujian.feature.editor.layout

import android.text.TextPaint
import android.text.style.LeadingMarginSpan
import com.xiwei.sujian.feature.editor.projection.DisplayPatch
import com.xiwei.sujian.feature.editor.projection.DisplayTextMirror
import com.xiwei.sujian.feature.editor.projection.DisplayTextProjection
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotSame
import org.junit.Assert.assertSame
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #624 评论4/7：DynamicLayout 复用契约测试 —
 *
 * - 普通 mirror 内容变化继续复用同一个 DynamicLayout（fingerprint 未变不重建）；
 * - 只有真实配置变化（宽度/首行缩进等）才重建；
 * - 每次编辑仍生成新的 AndroidLayoutRevision（revision 更新与 new DynamicLayout 分离）；
 * - 影响段落结构的编辑增量维护首行缩进 span，不触发整篇重建。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class AndroidLayoutEngineReuseTest {
    private companion object {
        const val TWO_PARAGRAPHS = "ab\ncd"
    }

    private fun newEngine(text: String): Pair<DisplayTextMirror, AndroidLayoutEngine> {
        val mirror = DisplayTextMirror()
        mirror.loadText(text, text.toByteArray(Charsets.UTF_8).size)
        val paint =
            TextPaint().apply {
                textSize = 32f
            }
        val engine = AndroidLayoutEngine(mirror, paint)
        return mirror to engine
    }

    @Test
    fun plainMirrorChange_reusesSameDynamicLayout() {
        val (mirror, engine) = newEngine(TWO_PARAGRAPHS)
        engine.setWidth(500f)
        engine.requestLayout()
        val layout1 = engine.getLayout()
        val revision1 = engine.getCurrentRevision()

        // 普通正文变化：同一 buffer 上的增量修改（段落内部增删字符，无结构变化）。
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
        engine.requestLayout()

        assertSame("普通编辑必须复用同一个 DynamicLayout", layout1, engine.getLayout())
        assertNotSame("revision 更新与 DynamicLayout 重建必须分离", revision1, engine.getCurrentRevision())
        assertEquals("布局必须反映新正文", "aX\ncd", mirror.getText())
    }

    @Test
    fun unchangedWidth_doesNotRebuild() {
        val (_, engine) = newEngine(TWO_PARAGRAPHS)
        engine.setWidth(500f)
        engine.requestLayout()
        val layout1 = engine.getLayout()

        engine.setWidth(500f)
        engine.requestLayout()

        assertSame("宽度没变不得重建布局", layout1, engine.getLayout())
    }

    @Test
    fun changedWidth_rebuildsLayout() {
        val (_, engine) = newEngine(TWO_PARAGRAPHS)
        engine.setWidth(500f)
        engine.requestLayout()
        val layout1 = engine.getLayout()

        engine.setWidth(600f)
        engine.requestLayout()

        assertNotSame("真实宽度变化必须重建布局", layout1, engine.getLayout())
    }

    @Test
    fun firstLineIndentChange_rebuildsAndAppliesParagraphSpans() {
        val (mirror, engine) = newEngine(TWO_PARAGRAPHS)
        engine.setWidth(500f)
        engine.requestLayout()
        val layout1 = engine.getLayout()

        engine.setFirstLineIndent(true, 2f)
        engine.requestLayout()

        assertNotSame("首行缩进配置变化必须重建 DynamicLayout", layout1, engine.getLayout())
        val text = mirror.getSpannable()
        val starts =
            text.getSpans(0, text.length, LeadingMarginSpan.Standard::class.java)
                .map { text.getSpanStart(it) }
                .sorted()
        assertEquals("每个段落起点都必须有首行缩进 span", listOf(0, 3), starts)
        assertEquals("正文文本不得被 span 改动", TWO_PARAGRAPHS, mirror.getText())
    }

    @Test
    fun newlineInsert_reappliesParagraphSpansWithoutFullRebuild() {
        val (mirror, engine) = newEngine(TWO_PARAGRAPHS)
        engine.setWidth(500f)
        engine.setFirstLineIndent(true, 2f)
        engine.requestLayout()
        val layout1 = engine.getLayout()

        // Enter：在光标处插入 \n — 段落结构变化。
        mirror.getSpannable().insert(2, "\n")
        engine.onMirrorContentChanged(
            DisplayTextProjection.identity(mirror.getText()),
            listOf(
                DisplayPatch(
                    baseRevision = 0L,
                    newRevision = 1L,
                    replaceByteStart = 2,
                    replaceByteEndExclusive = 2,
                    insertedText = "\n",
                    resultingSelectionStart = 3,
                    resultingSelectionEnd = 3,
                ),
            ),
        )
        engine.requestLayout()

        assertSame("结构性编辑也不得整篇重建 DynamicLayout", layout1, engine.getLayout())
        val text = mirror.getSpannable()
        val starts =
            text.getSpans(0, text.length, LeadingMarginSpan.Standard::class.java)
                .map { text.getSpanStart(it) }
                .sorted()
        assertEquals("新段落必须获得首行缩进 span", listOf(0, 3, 4), starts)
    }

    @Test
    fun multiParagraphSelectionReplace_keepsEveryParagraphSpanAligned() {
        val (mirror, engine) = newEngine("第一段\n第二段\n第三段\n第四段")
        engine.setWidth(500f)
        engine.setFirstLineIndent(true, 2f)
        engine.requestLayout()

        // 跨段选区替换：选中前两段（含两个 \n）一次替换为单行文本 —
        // 合并段落与后续段落的缩进 span 都必须对齐到真实段落起点。
        mirror.getSpannable().replace(0, 8, "新")
        engine.onMirrorContentChanged(
            DisplayTextProjection.identity(mirror.getText()),
            listOf(
                DisplayPatch(
                    baseRevision = 0L,
                    newRevision = 1L,
                    replaceByteStart = 0,
                    replaceByteEndExclusive = 8,
                    insertedText = "新",
                    resultingSelectionStart = 1,
                    resultingSelectionEnd = 1,
                ),
            ),
        )
        engine.requestLayout()

        assertEquals("正文必须是替换结果", "新第三段\n第四段", mirror.getText())
        val text = mirror.getSpannable()
        val spans =
            text.getSpans(0, text.length, LeadingMarginSpan.Standard::class.java)
                .sortedBy { text.getSpanStart(it) }
        assertEquals("合并后的两段都必须有缩进 span", 2, spans.size)
        assertEquals("第一段 span 起点", 0, text.getSpanStart(spans[0]))
        assertEquals("第二段 span 起点", 5, text.getSpanStart(spans[1]))
        assertEquals("第一段 span 必须止于自身段落末尾（含换行）", 5, text.getSpanEnd(spans[0]))
        assertEquals("第二段 span 必须覆盖到文末", 8, text.getSpanEnd(spans[1]))
    }

    @Test
    fun newlineInsert_midParagraph_atRegionEndKeepsFollowingParagraphSpan() {
        // 在段落中部插入换行且该位置恰为区域末尾 — 后续段落 span 不得被误删。
        val (mirror, engine) = newEngine("ab\ncd")
        engine.setWidth(500f)
        engine.setFirstLineIndent(true, 2f)
        engine.requestLayout()

        // 把第二段首字符前插入换行："ab\n\ncd" — 新空段落起点 3。
        mirror.getSpannable().insert(3, "\n")
        engine.onMirrorContentChanged(
            DisplayTextProjection.identity(mirror.getText()),
            listOf(
                DisplayPatch(
                    baseRevision = 0L,
                    newRevision = 1L,
                    replaceByteStart = 3,
                    replaceByteEndExclusive = 3,
                    insertedText = "\n",
                    resultingSelectionStart = 4,
                    resultingSelectionEnd = 4,
                ),
            ),
        )
        engine.requestLayout()

        val text = mirror.getSpannable()
        val starts =
            text.getSpans(0, text.length, LeadingMarginSpan.Standard::class.java)
                .map { text.getSpanStart(it) }
                .sorted()
        assertEquals("每个段落（含空段落）都必须有缩进 span", listOf(0, 3, 4), starts)
    }
}
