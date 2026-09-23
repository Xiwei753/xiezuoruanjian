package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.sp
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.layout.cursorRect
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * Issue #737 评论 5784705864 缺口1测试 — layout 先到、fact 后到时的 pending presentation ownership。
 *
 * 场景：第一笔 "" → "a" 正常完成后，第二笔 "a" → "ab" 的 layout 先到、fact 未到。
 * 此时 BasicTextField 已是 "ab" 正文，drawContent() 会裸画最终态。
 *
 * 期望（缺口1修复）：onAuthoritativeLayout("ab") 在 FrameUpdate.Empty 分支建立 pending presentation —
 * drawSnapshot().motionSample != null、hiddenRanges 覆盖新字 "b" 的 range [1,2)。
 * fact 到达后 pendingPresentation 原子升级成 prepared motion（pendingPresentation == null）。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
@Suppress("StringLiteralDuplication", "MaxLineLength")
class ComposeVisualStateLayoutFirstPendingPresentationTest {
    @get:Rule
    val composeRule = createComposeRule()

    /**
     * layout-first 时 drawSnapshot 应携带 pending presentation ownership，
     * fact 到达后 pendingPresentation 清空、prepared motion 接管。
     */
    @Test
    fun layoutFirst_factPending_drawSnapshotHasPendingOwnership() {
        val layouts = captureLayouts("", "a", "ab")
        val state = ComposeEditorVisualState(targetId = "737-layout-first")

        // === 第一笔："" -> "a"（正常完成，建立 baseline presentation）===
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onEditFact(
            makeInsertIntent(
                coreTxnId = 1L,
                baseRev = 0L,
                newRev = 1L,
                oldText = "",
                newText = "a",
                newRange = TextRange(0, 1),
                replaceBounds = VisualReplaceBounds(0, 0, 0, 1),
            ),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        // 第一笔完成后 drawSnapshotState.layout = "a" layout（applyPreparedMotionFromPatch 设）
        val snapAfterFirst = state.drawSnapshot()
        assertNotNull("第一笔完成后 drawSnapshot.layout 应非 null", snapAfterFirst.layout)

        // === 第二笔 layout-first："a" -> "ab" 的 layout 先到，fact 未到 ===
        state.onAuthoritativeLayout(layouts[2], TextRange(2, 2), 0)
        // 触发缺口1修复：Empty 分支用 drawSnapshotState.layout（"a" layout）当 oldLayout，
        // buildPendingPresentation("a", "ab") 建立 pending presentation。
        val snap = state.drawSnapshot()
        assertNotNull("layout-first 时 motionSample 应非 null（pending presentation 接管）", snap.motionSample)
        assertTrue("pending sample 应有效", snap.motionSample!!.isValid)
        assertTrue(
            "hiddenRanges 应非空（新字 'b' 的 range 被裁）",
            snap.motionSample!!.hiddenRanges.isNotEmpty(),
        )
        assertEquals(
            "hiddenRanges 应覆盖新字 'b' 的 range [1,2)",
            TextRange(1, 2),
            snap.motionSample!!.hiddenRanges.first(),
        )
        // 反射断言 pendingPresentation != null
        val pendingBeforeFact = readPendingPresentation(state)
        assertNotNull("fact 到达前 pendingPresentation 应非 null", pendingBeforeFact)

        // === fact 到达："a" -> "ab" 配对生成 NewPatch，原子升级成 prepared motion ===
        state.onEditFact(
            makeInsertIntent(
                coreTxnId = 2L,
                baseRev = 1L,
                newRev = 2L,
                oldText = "a",
                newText = "ab",
                newRange = TextRange(1, 2),
                replaceBounds = VisualReplaceBounds(1, 1, 1, 2),
                offsetMap =
                    VisualOffsetMap(
                        entries = listOf(VisualOffsetMapEntry(0, 0, 1, VisualOffsetMapKind.IDENTITY)),
                    ),
            ),
        )
        // 反射断言 pendingPresentation == null（已升级成 prepared motion）
        val pendingAfterFact = readPendingPresentation(state)
        assertNull("fact 到达后 pendingPresentation 应清空（升级成 prepared motion）", pendingAfterFact)
        // prepared motion 的 progress=0 sample 应非 null
        val snap2 = state.drawSnapshot()
        assertNotNull("fact 到达后 motionSample 应非 null（prepared motion 接管）", snap2.motionSample)

        // === drain：start prepared motion ===
        state.drainPendingPatchesAtFrame(0L)
        val scene = state.sampleVisualScene(0L)
        assertNotNull("drain + sample 后应有 motion sample", scene)
        assertTrue("motion sample 应有效", scene!!.isValid)
    }

    /**
     * Issue #737 评论 5785295971 问题1测试 — 首次 layout 后 drawSnapshot 必须立即有 layout + resting caret。
     *
     * 旧实现：FrameUpdate.Empty 把"初始 baseline"和"等 fact"混在一起，首次 layout 时
     * drawSnapshotState.layout == null，什么都不发布，自定义 caret 不显示。
     *
     * 期望（修复后）：首次 onAuthoritativeLayout 返回 FrameUpdate.LayoutOnly，
     * applyFrameUpdate 直接静态发布新 layout + resting caret。
     */
    @Test
    fun firstLayout_drawSnapshotHasLayoutAndRestingCaret() {
        val layouts = captureLayouts("hello")
        val state = ComposeEditorVisualState(targetId = "737-first-layout")

        // 首次 layout — 应返回 LayoutOnly，直接静态发布
        state.onAuthoritativeLayout(layouts[0], TextRange(5, 5), 0)

        val snap = state.drawSnapshot()
        assertNotNull("首次 layout 后 drawSnapshot.layout 应非 null", snap.layout)
        assertNotNull("首次 layout 后 restingCaretRect 应非 null", snap.restingCaretRect)
        // caret 应在 selection.end = 5 的位置
        val expectedCaret = ComposeLayoutSnapshot(layouts[0], TextRange(5, 5), 0).cursorRect(5)
        assertEquals("首次 layout 后 restingCaretRect 应在 selection.end 位置", expectedCaret, snap.restingCaretRect)
        // 无 motionSample（纯静态发布）
        assertNull("首次 layout 后 motionSample 应为 null（纯静态发布）", snap.motionSample)
    }

    /**
     * Issue #737 评论 5785295971 问题1测试 — text 不变但几何变化时 drawSnapshot 必须切到新 layout/caret。
     *
     * 旧实现：FrameUpdate.Empty 把"纯几何变化"和"等 fact"混在一起，
     * text 不变时 buildPendingPresentation 返回 null，draw snapshot 保留旧几何，caret 停在旧坐标。
     *
     * 期望（修复后）：text 不变但几何变化时返回 FrameUpdate.LayoutOnly，
     * applyFrameUpdate 直接静态发布新 layout + 新 resting caret。
     */
    @Test
    fun geometryChange_textSame_drawSnapshotSwitchesToNewLayoutAndCaret() {
        val (layoutWide, layoutNarrow) = captureWideAndNarrowLayouts("ab")

        val state = ComposeEditorVisualState(targetId = "737-geom-change")

        // 先建立 baseline：宽容器 layout
        state.onAuthoritativeLayout(
            result = layoutWide,
            selection = TextRange(2, 2),
            scrollY = 0,
        )
        val snapAfterWide = state.drawSnapshot()
        assertNotNull("宽容器 layout 后 drawSnapshot.layout 应非 null", snapAfterWide.layout)

        // text 不变但几何变化：窄容器 layout（同一文本 "ab"，不同宽度导致不同行几何）
        state.onAuthoritativeLayout(
            result = layoutNarrow,
            selection = TextRange(2, 2),
            scrollY = 0,
        )
        val snapAfterNarrow = state.drawSnapshot()
        assertNotNull("窄容器 layout 后 drawSnapshot.layout 应非 null", snapAfterNarrow.layout)
        assertNotNull(
            "窄容器 layout 后 restingCaretRect 应非 null",
            snapAfterNarrow.restingCaretRect,
        )
        // 关键断言：layout 已切换到新几何
        assertNotEquals("几何变化后 layout 应更新（fingerprint 不同）", layoutWide, layoutNarrow)
        // 关键断言：restingCaretRect 来自新 layout 的 cursorRect
        val expectedNarrowCaret = ComposeLayoutSnapshot(layoutNarrow, TextRange(2, 2), 0).cursorRect(2)
        assertEquals(
            "几何变化后 restingCaretRect 应从新 layout 计算",
            expectedNarrowCaret,
            snapAfterNarrow.restingCaretRect,
        )
        // 无 motionSample（纯几何变化，不需要 pending ownership）
        assertNull("几何变化后 motionSample 应为 null（纯静态发布）", snapAfterNarrow.motionSample)
    }

    /**
     * Issue #737 评论 5785295971 问题2测试 — P1 running motion 未结束时 P2 layout-first 到达，
     * sampleVisualScene 后 drawSnapshot 仍然是 P2 pending sample，不会恢复成 P1 sample。
     *
     * 旧实现：sampleVisualScene 用 `motionSample ?: pendingPresentation?.sample`，
     * P1 activeMotion 还活着时 motionSample 非 null，P1 sample 优先于 P2 pending sample，
     * draw snapshot 变成 "P2 layout + P1 sample" — 正是要消灭的 "old sample + new layout"。
     *
     * 期望（修复后）：onAuthoritativeLayout 建立 P2 pending presentation 时先 settle P1 running motion
     * （activeMotion = null），sampleVisualScene 只能采样当前 owner（pending presentation），
     * 不会恢复成 P1 sample。
     */
    @Test
    fun p1Running_p2LayoutFirst_sampleVisualSceneStaysP2Pending() {
        val layouts = captureLayouts("", "a", "ab")
        val state = ComposeEditorVisualState(targetId = "737-mutex-owner")

        // === 第一笔："" -> "a"（正常完成，建立 baseline）===
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onEditFact(
            makeInsertIntent(
                coreTxnId = 1L,
                baseRev = 0L,
                newRev = 1L,
                oldText = "",
                newText = "a",
                newRange = TextRange(0, 1),
                replaceBounds = VisualReplaceBounds(0, 0, 0, 1),
            ),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        // drain + sample 让 P1 motion 开始 running
        state.drainPendingPatchesAtFrame(0L)
        state.sampleVisualScene(50L * 1_000_000L) // 50ms into animation, P1 still running

        // 确认 P1 motion 正在 running
        val activeMotionBefore = readActiveMotion(state)
        assertNotNull("P1 motion 应正在 running", activeMotionBefore)

        // === 第二笔 layout-first："a" -> "ab" 的 layout 先到，fact 未到 ===
        // onAuthoritativeLayout 应先 settle P1 running motion，再建立 P2 pending presentation
        state.onAuthoritativeLayout(layouts[2], TextRange(2, 2), 0)

        // 关键断言：P1 activeMotion 已被 settle（清空）
        val activeMotionAfter = readActiveMotion(state)
        assertNull("P2 pending 接管后 P1 activeMotion 应已 settle（null）", activeMotionAfter)

        // 关键断言：pendingPresentation 已建立
        val pendingAfter = readPendingPresentation(state)
        assertNotNull("P2 pending presentation 应已建立", pendingAfter)

        // === 调用 sampleVisualScene — 应返回 P2 pending sample，不是 P1 sample ===
        val sample = state.sampleVisualScene(60L * 1_000_000L) // 60ms
        val snap = state.drawSnapshot()
        assertNotNull("sampleVisualScene 应返回非 null sample（P2 pending）", sample)
        assertNotNull("drawSnapshot.motionSample 应非 null（P2 pending sample）", snap.motionSample)
        // P2 pending sample 的 hiddenRanges 应覆盖 "ab" 中的 "b" range [1,2)
        assertTrue("P2 pending hiddenRanges 应非空", snap.motionSample!!.hiddenRanges.isNotEmpty())
        assertEquals(
            "P2 pending hiddenRanges 应覆盖新字 'b' 的 range [1,2)",
            TextRange(1, 2),
            snap.motionSample!!.hiddenRanges.first(),
        )
    }

    /**
     * Issue #737 评论 5786405265 漏洞1测试 — P1 prepared motion 未 drain 时 P2 layout-first 到达，
     * AwaitingFact 分支应 settle prepared motion 并一并结算其 pending patch + 重置 preparedMotionSequence。
     *
     * 场景（按评论顺序 1～6）：
     * 1. P1 patch 已生成（"" -> "a"）
     * 2. pendingPatches = [P1]
     * 3. activeMotion = prepared(P1)
     * 4. 第一只 frame 还没来（不调 drainPendingPatchesAtFrame）
     * 5. P2 的 layout 先到（"a" -> "ab" 的 layout）、P2 fact 还没到
     * 6. onAuthoritativeLayout 进入 AwaitingFact
     *
     * 旧实现（漏洞1）：AwaitingFact 只清 activeMotion，没结算 prepared motion 对应的 pending patch，
     * 也没重置 preparedMotionSequence。P1 唤醒的 frame 先执行 drainPendingPatchesAtFrame 时取出 P1 patch，
     * 因 activeMotion == null 重新从 P1 构造 running motion，sampleVisualScene 把 P1 sample 写回 →
     * layout=P2, motionSample=P1。
     *
     * 期望（修复后）：AwaitingFact 分支 settle prepared motion 时一并清掉 P1 patch +
     * 重置 preparedMotionSequence。之后 drainPendingPatchesAtFrame 不会取出 P1 patch
     * 重新构造 running motion，sampleVisualScene 保持 P2 pending sample。
     */
    @Test
    fun p1Prepared_notDrained_p2LayoutFirst_p1PatchSettledNotRevived() {
        val layouts = captureLayouts("", "a", "ab")
        val state = ComposeEditorVisualState(targetId = "737-prepared-settle")

        // === 第一笔："" -> "a"（正常完成，建立 baseline + prepared motion）===
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onEditFact(
            makeInsertIntent(
                coreTxnId = 1L,
                baseRev = 0L,
                newRev = 1L,
                oldText = "",
                newText = "a",
                newRange = TextRange(0, 1),
                replaceBounds = VisualReplaceBounds(0, 0, 0, 1),
            ),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        // 此时：pendingPatches = [P1], activeMotion = prepared(P1), preparedMotionSequence = 0
        // 关键：不调 drainPendingPatchesAtFrame，prepared motion 还没 start
        assertEquals("P1 patch 应在队列中", 1, readPendingPatchesSize(state))
        assertNotNull("P1 prepared motion 应存在", readActiveMotion(state))
        assertEquals("preparedMotionSequence 应为 0", 0L, readPreparedMotionSequence(state))

        // === 第二笔 layout-first："a" -> "ab" 的 layout 先到，fact 未到 ===
        // onAuthoritativeLayout 进入 AwaitingFact 分支，应 settle prepared motion +
        // 一并结算 P1 patch + 重置 preparedMotionSequence
        state.onAuthoritativeLayout(layouts[2], TextRange(2, 2), 0)

        // 关键断言：P1 patch 已被一并结算（不会复活）
        assertEquals("P1 patch 应已结算（pendingPatches 空）", 0, readPendingPatchesSize(state))
        assertNull("P1 prepared motion 应已 settle（activeMotion null）", readActiveMotion(state))
        assertEquals("preparedMotionSequence 应已重置为 -1L", -1L, readPreparedMotionSequence(state))

        // 关键断言：P2 pending presentation 已建立
        assertNotNull("P2 pending presentation 应已建立", readPendingPresentation(state))

        // === 调一次 drainPendingPatchesAtFrame + sampleVisualScene ===
        // drainPendingPatchesAtFrame：pendingPatches 空，不会取出 P1 patch 重新构造 running motion
        state.drainPendingPatchesAtFrame(0L)
        val scene = state.sampleVisualScene(0L)

        // 关键断言：P2 pending sample 仍然是当前 owner
        val snap = state.drawSnapshot()
        assertNotNull("sampleVisualScene 应返回非 null sample（P2 pending）", scene)
        assertNotNull("drawSnapshot.motionSample 应非 null（P2 pending sample）", snap.motionSample)
        assertTrue("P2 pending hiddenRanges 应非空", snap.motionSample!!.hiddenRanges.isNotEmpty())
        assertEquals(
            "P2 pending hiddenRanges 应覆盖新字 'b' 的 range [1,2)",
            TextRange(1, 2),
            snap.motionSample!!.hiddenRanges.first(),
        )

        // P1 不会复活
        assertEquals("drain 后 pendingPatches 仍应空（P1 不复活）", 0, readPendingPatchesSize(state))
        assertNull("drain 后 activeMotion 仍应 null（P1 不复活）", readActiveMotion(state))
        assertEquals("drain 后 preparedMotionSequence 仍应 -1L", -1L, readPreparedMotionSequence(state))
    }

    /**
     * Issue #737 评论 5786405265 漏洞2测试 — LayoutOnly 分支应统一结算旧 owner 状态。
     *
     * 场景：
     * 1. "" -> "ab"（第一笔完成，建立 prepared motion + pendingPatches = [P1]）
     * 2. "ab" 纯几何变化（窄容器，text 不变、fingerprint 变化）→ LayoutOnly
     * 3. LayoutOnly 分支调用 settleToStaticOwner() 清掉 prepared motion + pendingPatches + pendingPresentation
     *
     * 旧实现（漏洞2）：LayoutOnly 只改 drawSnapshot，没清 activeMotion / pendingPresentation /
     * preparedMotionSequence / pending patch。旧 prepared motion 的 patch 会在下一帧
     * drainPendingPatchesAtFrame 重新激活，旧 pendingPresentation 会在 sampleVisualScene 复活。
     *
     * 期望（修复后）：pendingPresentation == null、activeMotion == null、
     * preparedMotionSequence == -1L、pendingPatches 空。sampleVisualScene 后 motionSample == null。
     */
    @Test
    fun layoutOnly_clearsPendingPresentation() {
        val layouts = captureLayouts("", "a")
        val state = ComposeEditorVisualState(targetId = "737-layout-only-settle")

        // === 第一笔："" -> "a"（正常完成，建立 prepared motion + pendingPatches）===
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onEditFact(
            makeInsertIntent(
                coreTxnId = 1L,
                baseRev = 0L,
                newRev = 1L,
                oldText = "",
                newText = "a",
                newRange = TextRange(0, 1),
                replaceBounds = VisualReplaceBounds(0, 0, 0, 1),
            ),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        // 此时：pendingPatches = [P1], activeMotion = prepared(P1), preparedMotionSequence = 0
        assertEquals("P1 patch 应在队列中", 1, readPendingPatchesSize(state))
        assertNotNull("P1 prepared motion 应存在", readActiveMotion(state))
        assertEquals("preparedMotionSequence 应为 0", 0L, readPreparedMotionSequence(state))

        // === 触发 applyFrameUpdate(LayoutOnly) ===
        // Robolectric 的 TextMeasurer 不真正按 maxWidth 换行，无法通过 onAuthoritativeLayout
        // 的纯几何变化触发 LayoutOnly。直接反射调用 applyFrameUpdate(LayoutOnly) 验证
        // LayoutOnly 分支调用 settleToStaticOwner() 清掉所有旧 owner 状态。
        val snapshot = ComposeLayoutSnapshot(layouts[1], TextRange(1, 1), 0)
        val layoutOnlyUpdate = FrameUpdate.LayoutOnly(snapshot)
        invokeApplyFrameUpdate(state, layoutOnlyUpdate)

        // 关键断言：settleToStaticOwner() 已清掉所有旧 owner 状态
        assertNull("LayoutOnly 后 pendingPresentation 应为 null", readPendingPresentation(state))
        assertNull("LayoutOnly 后 activeMotion 应为 null", readActiveMotion(state))
        assertEquals("LayoutOnly 后 preparedMotionSequence 应为 -1L", -1L, readPreparedMotionSequence(state))
        assertEquals("LayoutOnly 后 pendingPatches 应空", 0, readPendingPatchesSize(state))

        // === 再调一次 sampleVisualScene，motionSample 应为 null（旧 pending 不复活）===
        val scene = state.sampleVisualScene(0L)
        val snap = state.drawSnapshot()
        assertNull("sampleVisualScene 应返回 null（无 active motion / pending presentation）", scene)
        assertNull("drawSnapshot.motionSample 应为 null（旧 pending 不复活）", snap.motionSample)
    }

    // ==================== 辅助方法 ====================

    /** 反射读取 ComposeEditorVisualState 的 private activeMotion 字段。 */
    private fun readActiveMotion(state: ComposeEditorVisualState): Any? {
        val field = ComposeEditorVisualState::class.java.getDeclaredField("activeMotion")
        field.isAccessible = true
        return field.get(state)
    }

    private fun makeInsertIntent(
        coreTxnId: Long,
        baseRev: Long,
        newRev: Long,
        oldText: String,
        newText: String,
        newRange: TextRange,
        replaceBounds: VisualReplaceBounds,
        offsetMap: VisualOffsetMap? = null,
    ): EditorEditFact =
        EditorEditFact(
            cause = uniffi.writer_core.EditorTransactionCauseDto.PROGRAMMATIC,
            operationKind = uniffi.writer_core.EditorOperationKindDto.REPLACE,
            coreTransactionId = coreTxnId,
            baseRevision = baseRev,
            newRevision = newRev,
            animationMode = AnimationMode.CLUSTER_ANIMATION,
            durationMs = 100L,
            offsetMap = offsetMap,
            oldRanges = emptyList(),
            newRanges = listOf(newRange),
            textKind = TextVisualKind.Insert,
            replaceBounds = replaceBounds,
            expectedOldText = oldText,
            expectedNewText = newText,
        )

    private fun captureLayouts(vararg texts: String): List<TextLayoutResult> {
        val results = mutableListOf<TextLayoutResult>()
        composeRule.setContent {
            val textMeasurer = androidx.compose.ui.text.rememberTextMeasurer()
            texts.forEach { text ->
                results.add(
                    textMeasurer.measure(
                        text = AnnotatedString(text),
                        style = TextStyle(fontSize = 14f.sp),
                        constraints = Constraints(maxWidth = 1000),
                    ),
                )
            }
        }
        return results
    }

    /** 捕获同一文本在宽/窄容器中的 layout（行几何不同），用于测试纯几何变化。 */
    private fun captureWideAndNarrowLayouts(text: String): Pair<TextLayoutResult, TextLayoutResult> {
        var wide: TextLayoutResult? = null
        var narrow: TextLayoutResult? = null
        composeRule.setContent {
            val textMeasurer = androidx.compose.ui.text.rememberTextMeasurer()
            wide =
                textMeasurer.measure(
                    text = AnnotatedString(text),
                    style = TextStyle(fontSize = 14f.sp),
                    constraints = Constraints(maxWidth = 1000),
                )
            narrow =
                textMeasurer.measure(
                    text = AnnotatedString(text),
                    style = TextStyle(fontSize = 14f.sp),
                    constraints = Constraints(maxWidth = 20),
                )
        }
        return wide!! to narrow!!
    }

    /** 反射读取 ComposeEditorVisualState 的 private pendingPresentation 字段。 */
    private fun readPendingPresentation(state: ComposeEditorVisualState): Any? {
        val field = ComposeEditorVisualState::class.java.getDeclaredField("pendingPresentation")
        field.isAccessible = true
        return field.get(state)
    }

    /** 反射读取 ComposeEditorVisualState 的 private pendingPatches 字段大小。 */
    private fun readPendingPatchesSize(state: ComposeEditorVisualState): Int {
        val field = ComposeEditorVisualState::class.java.getDeclaredField("pendingPatches")
        field.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        val deque = field.get(state) as kotlin.collections.ArrayDeque<*>
        return deque.size
    }

    /** 反射读取 ComposeEditorVisualState 的 private preparedMotionSequence 字段。 */
    private fun readPreparedMotionSequence(state: ComposeEditorVisualState): Long {
        val field = ComposeEditorVisualState::class.java.getDeclaredField("preparedMotionSequence")
        field.isAccessible = true
        return field.getLong(state)
    }

    /** 反射调用 ComposeEditorVisualState 的 private applyFrameUpdate 方法。 */
    private fun invokeApplyFrameUpdate(
        state: ComposeEditorVisualState,
        update: FrameUpdate,
    ) {
        val method = ComposeEditorVisualState::class.java.getDeclaredMethod("applyFrameUpdate", FrameUpdate::class.java)
        method.isAccessible = true
        method.invoke(state, update)
    }
}
