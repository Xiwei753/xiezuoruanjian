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
}
