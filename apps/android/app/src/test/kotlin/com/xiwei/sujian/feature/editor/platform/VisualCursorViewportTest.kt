package com.xiwei.sujian.feature.editor.platform

import android.graphics.RectF
import android.view.View.MeasureSpec
import androidx.test.core.app.ApplicationProvider
import com.xiwei.sujian.feature.editor.interop.EditorKernelBridge
import com.xiwei.sujian.feature.editor.session.TextEditorProfile
import com.xiwei.sujian.feature.editor.visual.AndroidTextAnimationEngine
import com.xiwei.sujian.feature.editor.visual.AndroidVisualPlanner
import com.xiwei.sujian.feature.editor.visual.ChoreographerAnimationTimeSource
import com.xiwei.sujian.feature.editor.visual.PreparedVisualTransaction
import com.xiwei.sujian.feature.editor.visual.TransactionIdSource
import com.xiwei.sujian.feature.editor.visual.VisualProgressWindow
import com.xiwei.sujian.feature.editor.visual.VisualResourceStore
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.writer_core.EditorEditResultDto
import uniffi.writer_core.EditorTransactionCauseDto

/**
 * #638：视觉光标/事务时序 — 视口在动画进行中必须用视觉光标位置
 * （而非静态光标位置）判断是否滚入可视区。
 *
 * 场景：
 * 1. 用户输入触发视觉事务，光标从 (0, 0) 平滑移动到 (0, 500)
 * 2. 在动画中间帧（progress=0.5），视觉光标在 y=250
 * 3. 此时视口 scrollY=0，高度=400，光标在可视区内 — 不应滚动
 * 4. 若错误地用静态光标（y=500）判断，会误认为光标在可视区外而滚动
 *
 * 验证点：
 * - EditorViewportController.updateMaxScroll(max, clampNow) 仅在 clampNow=true 时夹取
 * - AndroidTextAnimationEngine.currentVisualCursorRect(frameTimeMs) 复用 CursorTransition.rectAt
 * - SujianEditorView.onFrame 用视觉光标 Rect 经 ensureRectVisible 判断
 * - pipeline/runtime 只委托，不重复计算
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class VisualCursorViewportTest {

    private lateinit var view: SujianEditorView
    private lateinit var engine: AndroidTextAnimationEngine

    @Before
    fun setUp() {
        view = SujianEditorView(ApplicationProvider.getApplicationContext())
        engine = AndroidTextAnimationEngine(
            AndroidVisualPlanner(),
            VisualResourceStore(),
            ChoreographerAnimationTimeSource(),
            TransactionIdSource(),
        )
        view.attachSession(
            sessionBridge = RecordingBridge(),
            profile = TextEditorProfile.DocumentBody,
            text = "测试文本\n".repeat(50),
            revision = 1L,
            cursorUtf8 = 0,
            selStartUtf8 = 0,
            selEndUtf8 = 0,
        )
        measureAndLayout(width = 800, height = 600)
    }

    /**
     * AndroidTextAnimationEngine.currentVisualCursorRect — 给 tx 非空 animatedSlices/blockShifts
     * 或明确设置/验证 cursor timeline 时，按 progress 线性插值返回视觉光标 Rect。
     * 独立 engine 不会驱动 view pipeline，此处只测 engine 行为，不保留虚假 View 断言。
     */
    @Test
    fun currentVisualCursorRect_returnsInterpolatedPosition() {
        // 提交一个光标从 y=0 到 y=500 的动画事务
        val tx =
            PreparedVisualTransaction(
                transactionId = 1L,
                oldRevision = null,
                newRevision = null,
                staticPatches = emptyList(),
                animatedSlices = emptyList(),
                ownedSnapshotIds = emptySet(),
                referencedSnapshotIds = emptySet(),
                selectionDecoration = null,
                preeditDecoration = null,
                cursorTransition =
                    PreparedVisualTransaction.CursorTransition(
                        fromX = 0f,
                        fromY = 0f,
                        fromHeight = 20f,
                        toX = 0f,
                        toY = 500f,
                        toHeight = 20f,
                        shouldAnimate = true,
                        progressWindow = VisualProgressWindow.Full,
                    ),
                durationMs = 200L,
            )
        engine.setSmoothCursor(true, 200L)
        engine.submit(tx, submittedAtMs = 0L)

        // 在 100ms（progress=0.5），视觉光标应在 y=250
        val visualRect = engine.currentVisualCursorRect(100L)
        assertNotNull("视觉光标 Rect 不应为 null", visualRect)
        assertEquals("视觉光标 left 应为 0", 0.0, visualRect!!.left.toDouble(), 0.001)
        assertEquals("视觉光标 top 应为 250 (0 + 500*0.5)", 250.0, visualRect!!.top.toDouble(), 0.001)
        assertEquals("视觉光标 bottom 应为 270", 270.0, visualRect!!.bottom.toDouble(), 0.001)

        // 在 0ms（progress=0），视觉光标应在 y=0
        val rectAt0 = engine.currentVisualCursorRect(0L)
        assertNotNull("视觉光标 Rect 不应为 null", rectAt0)
        assertEquals("视觉光标 top 应为 0", 0.0, rectAt0!!.top.toDouble(), 0.001)

        // 在 200ms（progress=1），视觉光标应在 y=500
        val rectAt200 = engine.currentVisualCursorRect(200L)
        assertNotNull("视觉光标 Rect 不应为 null", rectAt200)
        assertEquals("视觉光标 top 应为 500", 500.0, rectAt200!!.top.toDouble(), 0.001)
    }

    /**
     * updateMaxScroll(max, clampNow=false) 只更新 maxScrollY 不夹取 scrollY。
     */
    @Test
    fun updateMaxScroll_withoutClamping_updatesMaxOnly() {
        val viewport = getViewportController()

        viewport.setScroll(x = 0f, y = 1000f)
        viewport.updateMaxScroll(max = 800f, clampNow = false)

        assertEquals("maxScrollY 应更新为 800", 800.0, getMaxScrollY(viewport).toDouble(), 0.001)
        assertEquals("scrollY 不应被夹取（仍为 1000）", 1000.0, viewport.scrollY.toDouble(), 0.001)
    }

    /**
     * updateMaxScroll(max, clampNow=true) 更新 maxScrollY 并夹取 scrollY。
     */
    @Test
    fun updateMaxScroll_withClamping_clampsScrollY() {
        val viewport = getViewportController()

        viewport.setScroll(x = 0f, y = 1000f)
        viewport.updateMaxScroll(max = 800f, clampNow = true)

        assertEquals("maxScrollY 应更新为 800", 800.0, getMaxScrollY(viewport).toDouble(), 0.001)
        assertEquals("scrollY 应被夹取到 800", 800.0, viewport.scrollY.toDouble(), 0.001)
    }

    /**
     * currentVisualCursorRect — 在独立 cursor timeline 开启时（setSmoothCursor），
     * 按 frameTimeMs 的 progress 线性插值返回视觉光标 Rect。
     * 本测试只验证 engine 行为，不声称独立 engine 驱动 View pipeline。
     */
    @Test
    fun currentVisualCursorRect_usesFrameProgress() {
        // 开启独立 cursor timeline，时长 300ms，与文本事务 100ms 解耦，
        // 使 cursor timeline 的时间轴明确可测。
        engine.setSmoothCursor(true, 300L)

        val tx =
            PreparedVisualTransaction(
                transactionId = 2L,
                oldRevision = null,
                newRevision = null,
                staticPatches = emptyList(),
                animatedSlices = emptyList(),
                ownedSnapshotIds = emptySet(),
                referencedSnapshotIds = emptySet(),
                selectionDecoration = null,
                preeditDecoration = null,
                cursorTransition =
                    PreparedVisualTransaction.CursorTransition(
                        fromX = 0f,
                        fromY = 0f,
                        fromHeight = 20f,
                        toX = 0f,
                        toY = 600f,
                        toHeight = 20f,
                        shouldAnimate = true,
                        progressWindow = VisualProgressWindow.Full,
                    ),
                durationMs = 100L,
            )
        engine.submit(tx, submittedAtMs = 0L)

        // 0ms（progress=0），视觉光标应在 y=0
        val rectAt0 = engine.currentVisualCursorRect(0L)
        assertNotNull("0ms 视觉光标 Rect 不应为 null", rectAt0)
        assertEquals("0ms 视觉光标 top 应为 0", 0.0, rectAt0!!.top.toDouble(), 0.001)

        // 75ms（独立 cursor timeline progress=0.25），视觉光标应在 y=150
        val rect75 = engine.currentVisualCursorRect(75L)
        assertNotNull("75ms 视觉光标 Rect 不应为 null", rect75)
        assertEquals("75ms 视觉光标 top 应为 150", 150.0, rect75!!.top.toDouble(), 0.001)

        // 150ms（progress=0.5），视觉光标应在 y=300
        val rect150 = engine.currentVisualCursorRect(150L)
        assertNotNull("150ms 视觉光标 Rect 不应为 null", rect150)
        assertEquals("150ms 视觉光标 top 应为 300", 300.0, rect150!!.top.toDouble(), 0.001)

        // 300ms（progress=1），视觉光标应在 y=600
        val rect300 = engine.currentVisualCursorRect(300L)
        assertNotNull("300ms 视觉光标 Rect 不应为 null", rect300)
        assertEquals("300ms 视觉光标 top 应为 600", 600.0, rect300!!.top.toDouble(), 0.001)
    }

    /**
     * reachesDestination — 独立 cursor timeline 完成后，视觉光标到达终点。
     * 本测试只验证 engine 行为，不声称独立 engine 驱动 View pipeline。
     */
    @Test
    fun reachesDestination() {
        // 开启独立 cursor timeline，时长 200ms，与文本事务 100ms 解耦。
        engine.setSmoothCursor(true, 200L)

        val tx =
            PreparedVisualTransaction(
                transactionId = 3L,
                oldRevision = null,
                newRevision = null,
                staticPatches = emptyList(),
                animatedSlices = emptyList(),
                ownedSnapshotIds = emptySet(),
                referencedSnapshotIds = emptySet(),
                selectionDecoration = null,
                preeditDecoration = null,
                cursorTransition =
                    PreparedVisualTransaction.CursorTransition(
                        fromX = 0f,
                        fromY = 0f,
                        fromHeight = 20f,
                        toX = 0f,
                        toY = 500f,
                        toHeight = 20f,
                        shouldAnimate = true,
                        progressWindow = VisualProgressWindow.Full,
                    ),
                durationMs = 100L,
            )
        engine.submit(tx, submittedAtMs = 0L)

        // 200ms（独立 cursor timeline 完成），视觉光标应在 y=500
        val visualRect = engine.currentVisualCursorRect(200L)
        assertNotNull("200ms 视觉光标 Rect 不应为 null", visualRect)
        assertEquals("动画完成后视觉光标 top 应为 500", 500.0, visualRect!!.top.toDouble(), 0.001)
        assertEquals("动画完成后视觉光标 bottom 应为 520", 520.0, visualRect.bottom.toDouble(), 0.001)

        // 超过 200ms 仍保持在终点
        val rect300 = engine.currentVisualCursorRect(300L)
        assertNotNull("300ms 视觉光标 Rect 不应为 null", rect300)
        assertEquals("300ms 视觉光标 top 应保持在 500", 500.0, rect300!!.top.toDouble(), 0.001)
    }

    /**
     * ensureRectVisible 通用实现：用给定 Rect 判断是否滚入可视区。
     */
    @Test
    fun ensureRectVisible_bringsRectIntoView() {
        val viewport = getViewportController()
        viewport.setScroll(x = 0f, y = 0f)
        setMaxScrollY(viewport, 1000f)

        val rect = RectF(0f, 500f, 2f, 520f) // 光标在 y=500
        val viewHeight = 400f
        val paddingTop = 0f

        ensureRectVisible(viewport, rect, viewHeight, paddingTop)

        // rect.bottom=520, viewHeight=400 → 可视区 [0, 400]
        // rect 在可视区外，应滚动到 y=120（让 rect.bottom 在可视区底部）
        assertEquals("rect 在可视区外，scrollY 应滚动到 rect.bottom - viewHeight", 120.0, viewport.scrollY.toDouble(), 0.001)
    }

    /**
     * ensureRectVisible 已在可视区内不滚动。
     */
    @Test
    fun ensureRectVisible_noOpWhenAlreadyVisible() {
        val viewport = getViewportController()
        viewport.setScroll(x = 0f, y = 100f)
        setMaxScrollY(viewport, 1000f)

        val rect = RectF(0f, 150f, 2f, 170f) // 光标在 y=150，可视区 [100, 500]
        val viewHeight = 400f
        val paddingTop = 0f

        ensureRectVisible(viewport, rect, viewHeight, paddingTop)

        // rect 在可视区内，不应滚动
        assertEquals("rect 已在可视区内，scrollY 不应改变", 100.0, viewport.scrollY.toDouble(), 0.001)
    }

    // ── Helpers ──

    private fun getViewportController(): EditorViewportController {
        val field = SujianEditorView::class.java.getDeclaredField("viewport")
        field.isAccessible = true
        return field.get(view) as EditorViewportController
    }

    private fun measureAndLayout(width: Int, height: Int) {
        val widthSpec = MeasureSpec.makeMeasureSpec(width, MeasureSpec.EXACTLY)
        val heightSpec = MeasureSpec.makeMeasureSpec(height, MeasureSpec.EXACTLY)
        view.measure(widthSpec, heightSpec)
        view.layout(0, 0, view.measuredWidth, view.measuredHeight)
    }

    private fun setScrollYForTest(value: Float) {
        val viewport = getViewportController()
        viewport.setScrollYUnclamped(value)
    }

    private fun getScrollYPos(): Float {
        val viewport = getViewportController()
        return viewport.scrollY
    }

    private fun getMaxScrollY(viewport: EditorViewportController): Float {
        val field = EditorViewportController::class.java.getDeclaredField("maxScrollY")
        field.isAccessible = true
        return field.get(viewport) as Float
    }

    private fun setMaxScrollY(viewport: EditorViewportController, value: Float) {
        val field = EditorViewportController::class.java.getDeclaredField("maxScrollY")
        field.isAccessible = true
        field.set(viewport, value)
    }

    private fun invokeEnsureSelectionVisible() {
        val method = SujianEditorView::class.java.getDeclaredMethod("ensureSelectionVisible")
        method.isAccessible = true
        method.invoke(view)
    }

    /**
     * 通用 ensureRectVisible：用给定 Rect 判断是否滚入可视区。
     * 与 ensureSelectionVisible 使用相同的逻辑，但接受任意 Rect。
     */
    private fun ensureRectVisible(
        viewport: EditorViewportController,
        rect: RectF,
        viewHeight: Float,
        paddingTop: Float,
    ) {
        val contentTop = viewport.scrollY - paddingTop
        val contentBottom = contentTop + viewHeight

        if (rect.top < contentTop) {
            viewport.setScrollYUnclamped(rect.top + paddingTop)
        } else if (rect.bottom > contentBottom) {
            viewport.setScrollYUnclamped(rect.bottom - viewHeight + paddingTop)
        }
        viewport.clamp()
    }
}

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
