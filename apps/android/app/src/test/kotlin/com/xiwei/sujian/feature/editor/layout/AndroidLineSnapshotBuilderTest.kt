package com.xiwei.sujian.feature.editor.layout

import android.text.Layout
import android.text.StaticLayout
import android.text.TextPaint
import com.xiwei.sujian.feature.editor.projection.DisplayTextMirror
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #605 评论4 问题4: AndroidLineSnapshotBuilder 混排行尾 caretEndX 契约测试 —
 * 验证行尾 cluster 的 caretEndX 取当前 cluster 自身方向（isRtlCharAt），
 * 不再看段落方向（getParagraphDirection / lineStart）。
 *
 * 旧行为：行尾 cluster 的 caretEndX 看 lineStart（段落方向）。
 * 混排场景（LTR 段落 RTL 结尾 / RTL 段落 LTR 结尾）会判错整行方向，
 * 把行尾 cluster 的逻辑终点写到错误的一侧。
 *
 * 新行为：probe = clusterEndDisplayUtf16 - 1，clusterIsRtl = layout.isRtlCharAt(probe)。
 * RTL cluster → getLineLeft；LTR cluster → getLineRight。
 *
 * 这两个测试用真实 Android StaticLayout（Robolectric）构造混排文本，
 * 验证行尾 cluster 的 caretEndX 与 cluster 方向一致，与段落方向相反。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class AndroidLineSnapshotBuilderTest {
    private val eps = 0.001f
    private val builder = AndroidLineSnapshotBuilder()

    /** 构造单行 AndroidLayoutRevision，lineRange 几何来自 [layout]。 */
    private fun makeRevisionForLine(
        text: String,
        layout: Layout,
        revisionId: Long = 1,
    ): AndroidLayoutRevision {
        val utf8Len = text.toByteArray(Charsets.UTF_8).size
        return AndroidLayoutRevision(
            revisionId = revisionId,
            editorRevision = revisionId,
            widthFingerprint = layout.width.toFloat(),
            fontFingerprint = "fp",
            lineCount = 1,
            lineRanges =
                listOf(
                    AndroidLayoutRevision.LineRange(
                        startUtf8 = 0,
                        endUtf8 = utf8Len,
                        startUtf16 = 0,
                        endUtf16 = text.length,
                        top = 0f,
                        bottom = layout.getLineBottom(0).toFloat(),
                        baseline = layout.getLineBaseline(0).toFloat(),
                        left = 0f,
                        right = layout.width.toFloat(),
                    ),
                ),
            cursorUtf8 = utf8Len,
            cursorUtf16 = text.length,
            cursorX = 0f,
            cursorY = 0f,
            cursorHeight = 20f,
            selectionAnchorUtf8 = utf8Len,
            selectionHeadUtf8 = utf8Len,
            selectionAnchorUtf16 = text.length,
            selectionHeadUtf16 = text.length,
            compositionStartUtf16 = 0,
            compositionEndUtf16 = 0,
            snapshotHandles = emptyList(),
        )
    }

    private fun buildLayout(text: String): StaticLayout {
        val paint =
            TextPaint().apply {
                textSize = 20f
                // 显式启用 bidi 测量（默认即支持，这里只是确保 paint 状态确定）
            }
        return StaticLayout(
            text,
            0,
            text.length,
            paint,
            400,
            Layout.Alignment.ALIGN_NORMAL,
            1f,
            0f,
            false,
        )
    }

    /**
     * LTR 行尾 RTL cluster: 一行以 LTR 开头、以 RTL cluster 结尾。
     * 段落方向 LTR，行尾 cluster 方向 RTL。
     *
     * 新行为：clusterIsRtl=true → caretEndX = getLineLeft。
     * 旧行为：看段落 LTR → caretEndX = getLineRight（错误）。
     */
    @Test
    fun ltrLineEndingWithRtlClusterCaretEndXUsesClusterDirection() {
        // 'a','b' (LTR) + 希伯来 alef א, bet ב (RTL)
        val text = "abאב"
        val layout = buildLayout(text)
        val mirror = DisplayTextMirror()
        mirror.loadText(text, text.toByteArray(Charsets.UTF_8).size)
        val revision = makeRevisionForLine(text, layout)

        val snapshot = builder.buildSnapshotForLineWithClusters(layout, 0, revision, mirror)
        assertNotNull("应返回非 null snapshot", snapshot)
        val clusters = snapshot!!.clusters
        assertTrue("应至少有 1 个 cluster，实际 ${clusters.size}", clusters.isNotEmpty())

        val lastCluster = clusters.last()
        val lineLeft = layout.getLineLeft(0)
        val lineRight = layout.getLineRight(0)
        assertTrue(
            "lineLeft($lineLeft) 应 != lineRight($lineRight)，否则无法区分新旧行为",
            kotlin.math.abs(lineLeft - lineRight) > eps,
        )

        // 验证行尾 cluster 确实是 RTL（probe = lineEnd - 1）
        val lineEnd = layout.getLineEnd(0)
        val lineStart = layout.getLineStart(0)
        val probe = (lineEnd - 1).coerceAtLeast(lineStart)
        val textLen = layout.text?.length ?: 0
        assertTrue("probe($probe) 应在文本范围内", probe >= 0 && probe < textLen)
        assertTrue("行尾 cluster 应是 RTL (isRtlCharAt($probe))", layout.isRtlCharAt(probe))

        // 新行为：RTL cluster → caretEndX = getLineLeft
        assertEquals(
            "LTR 行尾 RTL cluster: caretEndX 应取 getLineLeft($lineLeft)，实际 ${lastCluster.caretEndX}",
            lineLeft,
            lastCluster.caretEndX,
            eps,
        )
        // 旧行为会取 getLineRight — 验证新行为与之不同
        assertFalse(
            "LTR 行尾 RTL cluster: caretEndX 不应取 getLineRight($lineRight)（旧行为，看段落方向）",
            kotlin.math.abs(lineRight - lastCluster.caretEndX) < eps,
        )
    }

    /**
     * RTL 行尾 LTR cluster: 一行以 RTL 开头、以 LTR cluster 结尾。
     * 段落方向 RTL，行尾 cluster 方向 LTR。
     *
     * 新行为：clusterIsRtl=false → caretEndX = getLineRight。
     * 旧行为：看段落 RTL → caretEndX = getLineLeft（错误）。
     */
    @Test
    fun rtlLineEndingWithLtrClusterCaretEndXUsesClusterDirection() {
        // 希伯来 alef א, bet ב (RTL) + 'a','b' (LTR)
        val text = "אבab"
        val layout = buildLayout(text)
        val mirror = DisplayTextMirror()
        mirror.loadText(text, text.toByteArray(Charsets.UTF_8).size)
        val revision = makeRevisionForLine(text, layout)

        val snapshot = builder.buildSnapshotForLineWithClusters(layout, 0, revision, mirror)
        assertNotNull("应返回非 null snapshot", snapshot)
        val clusters = snapshot!!.clusters
        assertTrue("应至少有 1 个 cluster，实际 ${clusters.size}", clusters.isNotEmpty())

        val lastCluster = clusters.last()
        val lineLeft = layout.getLineLeft(0)
        val lineRight = layout.getLineRight(0)
        assertTrue(
            "lineLeft($lineLeft) 应 != lineRight($lineRight)，否则无法区分新旧行为",
            kotlin.math.abs(lineLeft - lineRight) > eps,
        )

        // 验证行尾 cluster 确实是 LTR
        val lineEnd = layout.getLineEnd(0)
        val lineStart = layout.getLineStart(0)
        val probe = (lineEnd - 1).coerceAtLeast(lineStart)
        val textLen = layout.text?.length ?: 0
        assertTrue("probe($probe) 应在文本范围内", probe >= 0 && probe < textLen)
        assertFalse("行尾 cluster 应是 LTR (isRtlCharAt($probe) == false)", layout.isRtlCharAt(probe))

        // 新行为：LTR cluster → caretEndX = getLineRight
        assertEquals(
            "RTL 行尾 LTR cluster: caretEndX 应取 getLineRight($lineRight)，实际 ${lastCluster.caretEndX}",
            lineRight,
            lastCluster.caretEndX,
            eps,
        )
        // 旧行为会取 getLineLeft — 验证新行为与之不同
        assertFalse(
            "RTL 行尾 LTR cluster: caretEndX 不应取 getLineLeft($lineLeft)（旧行为，看段落方向）",
            kotlin.math.abs(lineLeft - lastCluster.caretEndX) < eps,
        )
    }
}
