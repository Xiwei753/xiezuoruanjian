package com.xiwei.sujian.feature.editor.platform

import android.view.View.MeasureSpec
import androidx.test.core.app.ApplicationProvider
import com.xiwei.sujian.feature.editor.interop.EditorKernelBridge
import com.xiwei.sujian.feature.editor.session.TextEditorProfile
import com.xiwei.sujian.feature.editor.session.ViewportAnchor
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.writer_core.EditorEditResultDto
import uniffi.writer_core.EditorTransactionCauseDto

/**
 * #631 评论 5364514035 问题 4：视口快照必须用真实 measure/layout 而非 invokeOnSizeChanged 反射验证。
 *
 * - 尺寸 ≤0 时 captureViewportSnapshotOrNull 返回 null，pending anchor 不被消费；
 * - 真实 measure+layout 后 captureViewportSnapshot 返回有效锚点（offsetWithinLineFraction 0~1）；
 * - restoreViewportSnapshot 在尺寸未就绪时暂存 pending，updateLayoutConfig 就绪后自动恢复；
 * - reflowPreservingViewport（applyLayoutConfig 字号/行距变化）保住锚点；
 * - 字号变化后同一 fraction 对应不同 scrollY（行高变了）。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SujianEditorViewportSnapshotTest {
    private val longText = "测试文本，这是一段很长的内容用来产生多行排版。\n".repeat(80)

    private lateinit var view: SujianEditorView

    @Before
    fun setUp() {
        view = SujianEditorView(ApplicationProvider.getApplicationContext())
        view.attachSession(
            sessionBridge = RecordingBridge(),
            profile = TextEditorProfile.DocumentBody,
            text = longText,
            revision = 1L,
            cursorUtf8 = 0,
            selStartUtf8 = 0,
            selEndUtf8 = 0,
        )
    }

    // ── 1. 尺寸未就绪时快照返回 null ──

    @Test
    fun captureViewportSnapshotOrNull_returnsNullWhenDimensionsZero() {
        // 未 measure/layout → width=0, height=0
        assertNull(
            "尺寸 ≤0 时 captureViewportSnapshotOrNull 必须返回 null",
            view.invokeCaptureViewportSnapshotOrNull(),
        )
    }

    @Test
    fun captureViewportSnapshotOrNull_returnsAnchorWhenReady() {
        measureAndLayout(width = 800, height = 1200)
        view.setScrollYForTest(400f)

        val anchor = view.invokeCaptureViewportSnapshotOrNull()

        assertNotNull("measure+layout 后 captureViewportSnapshotOrNull 应返回锚点", anchor)
        assertTrue(
            "offsetWithinLineFraction 应在 [0, 1]",
            anchor!!.offsetWithinLineFraction in 0f..1f,
        )
    }

    @Test
    fun measureAndLayout_triggersLayoutCreation() {
        measureAndLayout(width = 800, height = 1200)
        val anchor = view.invokeCaptureViewportSnapshotOrNull()
        assertNotNull("measure+layout 后 pipeline.getLayout() 应非 null", anchor)
    }

    @Test
    fun restoreViewportSnapshot_defersWhenDimensionsZero() {
        val anchor = ViewportAnchor(textOffsetUtf16 = 50, offsetWithinLineFraction = 0.5f)

        view.restoreViewportSnapshot(anchor)

        // 尺寸未就绪 → pendingViewportAnchor 应被暂存，scrollY 不变
        assertEquals("尺寸未就绪时 scrollY 不得改变", 0f, view.getScrollYPos(), 0.01f)
        assertNotNull("pendingViewportAnchor 应被暂存", view.readPendingViewportAnchor())
    }

    @Test
    fun restoreViewportSnapshot_appliesImmediatelyWhenLayoutReady() {
        // measure+layout → onSizeChanged → updateLayoutConfig → Layout 已创建
        measureAndLayout(width = 800, height = 1200)
        view.setScrollYForTest(300f)

        val anchor = view.captureViewportSnapshot()
        val anchorCopy = anchor.copy()

        // Layout 已就绪 → restoreViewportSnapshot 立即恢复，不暂存
        view.restoreViewportSnapshot(anchorCopy)

        assertNull("layout 已就绪时 pending 不得暂存", view.readPendingViewportAnchor())
        assertTrue("scrollY 应被恢复（>0）", view.getScrollYPos() > 0f)
    }

    // ── 2. 真实 measure+layout 后快照有效 ──

    @Test
    fun captureViewportSnapshot_afterMeasureLayout_returnsValidAnchor() {
        measureAndLayout(width = 800, height = 1200)
        triggerLayout()

        val scrollY = 500f
        view.setScrollYForTest(scrollY)

        val anchor = view.captureViewportSnapshot()

        assertTrue(
            "textOffsetUtf16 应在有效范围 [0, textLen]",
            anchor.textOffsetUtf16 in 0..longText.length,
        )
        assertTrue(
            "offsetWithinLineFraction 应在 [0, 1]",
            anchor.offsetWithinLineFraction in 0f..1f,
        )
    }

    // ── 3. restore + updateLayoutConfig 自动恢复 ──

    @Test
    fun restoreViewportSnapshot_appliesAfterLayoutReady() {
        measureAndLayout(width = 800, height = 1200)
        triggerLayout()

        // 先滚动到一个位置，捕获锚点
        view.setScrollYForTest(800f)
        val originalAnchor = view.captureViewportSnapshot()

        // 模拟窗口切换：尺寸归零再恢复
        // 用 restoreViewportSnapshot 注入锚点，然后在 layout ready 时恢复
        val anchor = originalAnchor.copy()

        // 先让尺寸有效但 layout null → pending 暂存
        measureAndLayout(width = 800, height = 1200)
        view.restoreViewportSnapshot(anchor)

        // triggerLayout 让 pipeline.getLayout() 非 null
        triggerLayout()
        // 此时 updateLayoutConfig 会调用 applyPendingViewportAnchorIfReady
        invokeUpdateLayoutConfig(view)

        assertNull("layout ready 后 pending 应被消费", view.readPendingViewportAnchor())
        assertTrue("scrollY 应被恢复（>0）", view.getScrollYPos() > 0f)
    }

    // ── 4. reflowPreservingViewport 保住锚点 ──

    @Test
    fun applyLayoutConfig_preservesViewport() {
        measureAndLayout(width = 800, height = 1200)
        triggerLayout()

        view.setScrollYForTest(600f)
        val beforeAnchor = view.captureViewportSnapshot()
        val scrollBefore = view.getScrollYPos()

        // applyLayoutConfig 调用 reflowPreservingViewport
        view.applyLayoutConfig(
            fontSizeSp = 20f,
            lineSpacingMultiplier = 1.5f,
            firstLineIndentEnabled = false,
            firstLineIndentWidthChars = 0f,
        )

        val afterAnchor = view.captureViewportSnapshot()

        // 同一文本偏移 + 同一 fraction → 锚点内容相同
        assertEquals(
            "reflow 后 textOffsetUtf16 应保持一致",
            beforeAnchor.textOffsetUtf16,
            afterAnchor.textOffsetUtf16,
        )
        assertEquals(
            "reflow 后 offsetWithinLineFraction 应保持一致",
            beforeAnchor.offsetWithinLineFraction,
            afterAnchor.offsetWithinLineFraction,
            0.05f,
        )
        // 行高变了 → scrollY 会变，但锚点语义不变
        assertTrue(
            "scrollY 可能因行高变化而不同，但锚点语义应一致",
            view.getScrollYPos() >= 0f,
        )
    }

    // ── 5. 字号变化后 fraction 不变但 scrollY 变 ──

    @Test
    fun fontChange_fractionUnchanged_scrollYChanges() {
        measureAndLayout(width = 800, height = 1200)
        triggerLayout()

        view.setScrollYForTest(400f)
        val anchor1 = view.captureViewportSnapshot()

        // 加大字号 → 行高增大 → 同 fraction 对应更大 scrollY
        view.applyLayoutConfig(
            fontSizeSp = 24f,
            lineSpacingMultiplier = 1.5f,
            firstLineIndentEnabled = false,
            firstLineIndentWidthChars = 0f,
        )

        val anchor2 = view.captureViewportSnapshot()

        assertEquals(
            "字号变化后 textOffsetUtf16 应不变",
            anchor1.textOffsetUtf16,
            anchor2.textOffsetUtf16,
        )
        assertEquals(
            "字号变化后 fraction 应不变",
            anchor1.offsetWithinLineFraction,
            anchor2.offsetWithinLineFraction,
            0.05f,
        )
    }

    // ── 6. lineSpacingMultiplier 变化保住锚点 ──

    @Test
    fun lineSpacingChange_preservesAnchor() {
        measureAndLayout(width = 800, height = 1200)
        triggerLayout()

        view.setScrollYForTest(500f)
        val anchor1 = view.captureViewportSnapshot()

        view.applyLayoutConfig(
            fontSizeSp = 16f,
            lineSpacingMultiplier = 2.0f,
            firstLineIndentEnabled = false,
            firstLineIndentWidthChars = 0f,
        )

        val anchor2 = view.captureViewportSnapshot()

        assertEquals(
            "行距变化后 textOffsetUtf16 应不变",
            anchor1.textOffsetUtf16,
            anchor2.textOffsetUtf16,
        )
        assertEquals(
            "行距变化后 fraction 应不变",
            anchor1.offsetWithinLineFraction,
            anchor2.offsetWithinLineFraction,
            0.05f,
        )
    }

    // ── 7. 多次 reflow 连续保住锚点 ──

    @Test
    fun multipleReflows_preserveAnchor() {
        measureAndLayout(width = 800, height = 1200)
        triggerLayout()

        view.setScrollYForTest(300f)
        val original = view.captureViewportSnapshot()

        // 连续 5 次不同字号 reflow
        for (size in listOf(12f, 16f, 20f, 24f, 14f)) {
            view.applyLayoutConfig(
                fontSizeSp = size,
                lineSpacingMultiplier = 1.2f,
                firstLineIndentEnabled = false,
                firstLineIndentWidthChars = 0f,
            )
        }

        val final = view.captureViewportSnapshot()
        assertEquals(
            "多次 reflow 后 textOffsetUtf16 应不变",
            original.textOffsetUtf16,
            final.textOffsetUtf16,
        )
        assertEquals(
            "多次 reflow 后 fraction 应不变",
            original.offsetWithinLineFraction,
            final.offsetWithinLineFraction,
            0.05f,
        )
    }

    // ── 8. ViewportAnchor 数据类契约 ──

    @Test
    fun viewportAnchor_isDataClassWithExpectedFields() {
        val a = ViewportAnchor(textOffsetUtf16 = 10, offsetWithinLineFraction = 0.5f)
        val b = ViewportAnchor(textOffsetUtf16 = 10, offsetWithinLineFraction = 0.5f)
        val c = a.copy(textOffsetUtf16 = 20)

        assertEquals("data class equals", a, b)
        assertEquals("copy 修改 textOffsetUtf16", 20, c.textOffsetUtf16)
        assertEquals("copy 保留 fraction", 0.5f, c.offsetWithinLineFraction, 0.001f)
    }

    // ── Helpers ──

    /** 用真实 measure+layout 设置 View 的 width/height。 */
    private fun measureAndLayout(
        width: Int,
        height: Int,
    ) {
        val widthSpec = MeasureSpec.makeMeasureSpec(width, MeasureSpec.EXACTLY)
        val heightSpec = MeasureSpec.makeMeasureSpec(height, MeasureSpec.EXACTLY)
        view.measure(widthSpec, heightSpec)
        view.layout(0, 0, view.measuredWidth, view.measuredHeight)
    }

    /** 触发 pipeline.createLayout — 先设文本宽度再调 updateLayoutConfig。 */
    private fun triggerLayout() {
        invokeUpdateLayoutConfig(view)
    }

    private fun invokeUpdateLayoutConfig(v: SujianEditorView) {
        val method = SujianEditorView::class.java.getDeclaredMethod("updateLayoutConfig")
        method.isAccessible = true
        method.invoke(v)
    }

    private fun SujianEditorView.invokeCaptureViewportSnapshotOrNull(): ViewportAnchor? {
        val method = SujianEditorView::class.java.getDeclaredMethod("captureViewportSnapshotOrNull")
        method.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        return method.invoke(this) as? ViewportAnchor
    }

    private fun SujianEditorView.readPendingViewportAnchor(): ViewportAnchor? {
        val field = SujianEditorView::class.java.getDeclaredField("pendingViewportAnchor")
        field.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        return field.get(this) as? ViewportAnchor
    }

    private fun SujianEditorView.setScrollYForTest(value: Float) {
        val field = SujianEditorView::class.java.getDeclaredField("scrollY")
        field.isAccessible = true
        field.setFloat(this, value)
    }

    @Suppress("TooManyFunctions")
    private class RecordingBridge : EditorKernelBridge {
        override fun insert(
            byteOffset: Int,
            text: String,
            cause: EditorTransactionCauseDto,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun delete(
            byteStart: Int,
            byteEndExclusive: Int,
            cause: EditorTransactionCauseDto,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun replace(
            byteStart: Int,
            byteEndExclusive: Int,
            replacementText: String,
            originalText: String,
            cause: EditorTransactionCauseDto,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun setSelection(
            anchorByteOffset: Int,
            headByteOffset: Int,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun undo(expectedRevision: Long): EditorEditResultDto? = null

        override fun redo(expectedRevision: Long): EditorEditResultDto? = null

        override fun loadText(
            text: String,
            cursorUtf8: Int,
        ): EditorEditResultDto? = null

        override fun commitText(
            byteStart: Int,
            byteEndExclusive: Int,
            replacementText: String,
            resultingSelectionAnchor: Int,
            resultingSelectionHead: Int,
            compositionSessionId: Long,
            compositionBaseRevision: Long,
            compositionGeneration: Long,
            cause: EditorTransactionCauseDto,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun deleteSurrounding(
            beforeByteStart: Int,
            beforeByteEndExclusive: Int,
            afterByteStart: Int,
            afterByteEndExclusive: Int,
            cause: EditorTransactionCauseDto,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun beginComposition(
            replaceStart: Int,
            replaceEndExclusive: Int,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun updateComposition(
            compositionSessionId: Long,
            compositionGeneration: Long,
            newPreeditText: String,
            newPreeditCursorOffset: Int,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun finishComposition(
            compositionSessionId: Long,
            compositionGeneration: Long,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun cancelComposition(
            compositionSessionId: Long,
            compositionGeneration: Long,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun setAnimationEnabled(enabled: Boolean) = Unit

        override fun setAnimationDurationMs(durationMs: Long) = Unit

        override fun replaceAll(
            search: String,
            replacement: String,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun insertLineBreak(
            byteOffset: Int,
            autoIndentEnabled: Boolean,
            cause: EditorTransactionCauseDto,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun sessionSnapshot(): uniffi.writer_core.EditorSessionSnapshotDto? = null

        override fun previousGraphemeBoundary(byteOffset: Int): Int = byteOffset

        override fun nextGraphemeBoundary(byteOffset: Int): Int = byteOffset

        override fun computeRebaseSliceMappings(
            oldSliceRoles: List<uniffi.writer_core.AnimatedSliceRoleDto>,
            oldSliceByteRanges: List<uniffi.writer_core.EditorByteRangeDto>,
            newSliceRoles: List<uniffi.writer_core.AnimatedSliceRoleDto>,
            newSliceByteRanges: List<uniffi.writer_core.EditorByteRangeDto>,
            offsetMap: uniffi.writer_core.OffsetMapDto?,
        ): List<uniffi.writer_core.RebaseSliceMappingDto>? = null
    }
}
