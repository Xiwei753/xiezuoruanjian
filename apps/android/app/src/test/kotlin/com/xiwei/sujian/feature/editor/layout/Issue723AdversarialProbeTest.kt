package com.xiwei.sujian.feature.editor.layout

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * Issue #723 评论 5748592923 对抗式探针：直接攻击补丁核心 [EditorSoftBreakProjection]
 * 的 caret 专用映射 [EditorSoftBreakProjection.wedgeStart] /
 * [EditorSoftBreakProjection.wedgeEnd] /
 * [EditorSoftBreakProjection.rawToDisplayCaret] 的边界、幂等、affinity 一致性。
 *
 * 这不是"问题已消除"的结构断言，而是对补丁新引入数学函数的边界值/不变量攻击。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [33])
class Issue723AdversarialProbeTest {
    private companion object {
        const val RAW_LENGTH_TEN = 10
        const val RAW_LENGTH_FIFTY_ONE = 51
    }

    // ===== 边界 1：空 insertPoints → identity =====

    @Test
    fun probe_emptyInserts_wedgeStartIsIdentity() {
        val p = EditorSoftBreakProjection(rawLength = RAW_LENGTH_TEN, insertPoints = emptyList())
        for (i in 0..RAW_LENGTH_TEN) {
            assertEquals("空插入 wedgeStart($i) 应为 identity", i, p.wedgeStart(i))
        }
    }

    @Test
    fun probe_emptyInserts_wedgeEndIsIdentity() {
        val p = EditorSoftBreakProjection(rawLength = RAW_LENGTH_TEN, insertPoints = emptyList())
        for (i in 0..RAW_LENGTH_TEN) {
            assertEquals("空插入 wedgeEnd($i) 应为 identity", i, p.wedgeEnd(i))
        }
    }

    @Test
    fun probe_emptyInserts_rawToDisplayCaretIsIdentity() {
        val p = EditorSoftBreakProjection(rawLength = RAW_LENGTH_TEN, insertPoints = emptyList())
        for (i in 0..RAW_LENGTH_TEN) {
            assertEquals(i, p.rawToDisplayCaret(i, EditorSoftBreakProjection.CaretAffinity.Start))
            assertEquals(i, p.rawToDisplayCaret(i, EditorSoftBreakProjection.CaretAffinity.End))
        }
    }

    // ===== 边界 2：rawOffset = 0 =====

    /**
     * rawOffset=0：wedgeStart(0) = 0 + count(insertPoint < 0) = 0
     * （没有 insertPoint < 0，所以始终为 0，caret 落在所有 U+200B 之前）。
     */
    @Test
    fun probe_offsetZero_wedgeStartAlwaysZero() {
        val p = EditorSoftBreakProjection(rawLength = RAW_LENGTH_TEN, insertPoints = listOf(0, 3, 7))
        assertEquals("wedgeStart(0) 始终为 0（无 insertPoint < 0）", 0, p.wedgeStart(0))
    }

    /**
     * rawOffset=0 且 insertPoint=0：wedgeEnd(0) = 0 + count(insertPoint <= 0) = 1
     * （insertPoint 0 <= 0 计入，caret 落在 U+200B 之后）。
     */
    @Test
    fun probe_offsetZero_insertAtZero_wedgeEndIsOne() {
        val p = EditorSoftBreakProjection(rawLength = RAW_LENGTH_TEN, insertPoints = listOf(0))
        assertEquals("wedgeEnd(0) 应为 1（insertPoint 0<=0 计入）", 1, p.wedgeEnd(0))
        assertEquals("wedgeStart(0) 应为 0（insertPoint 0<0 不计入）", 0, p.wedgeStart(0))
        // affinity 区分在边界 insertPoint=0 处仍然成立
        assertTrue(
            "wedgeStart(0) != wedgeEnd(0) 当 insertPoint=0 存在",
            p.wedgeStart(0) != p.wedgeEnd(0),
        )
    }

    // ===== 边界 3：rawOffset = rawLength（文末）=====

    /**
     * rawOffset=rawLength：wedgeEnd(rawLength) = rawLength + count(insertPoint <= rawLength)
     * = rawLength + insertPoints.size = displayLength（所有插入点都 <= rawLength）。
     */
    @Test
    fun probe_offsetRawLength_wedgeEndEqualsDisplayLength() {
        val p = EditorSoftBreakProjection(rawLength = RAW_LENGTH_TEN, insertPoints = listOf(3, 7))
        assertEquals(
            "wedgeEnd(rawLength) 应为 displayLength",
            p.displayLength,
            p.wedgeEnd(RAW_LENGTH_TEN),
        )
    }

    /**
     * rawOffset=rawLength 且 insertPoint=rawLength：wedgeEnd(rawLength) = rawLength + 1。
     */
    @Test
    fun probe_offsetRawLength_insertAtRawLength_wedgeEnd() {
        val p = EditorSoftBreakProjection(rawLength = RAW_LENGTH_TEN, insertPoints = listOf(RAW_LENGTH_TEN))
        assertEquals(
            "wedgeEnd(10) 应为 11（insertPoint 10<=10 计入）",
            11,
            p.wedgeEnd(RAW_LENGTH_TEN),
        )
        assertEquals(
            "wedgeStart(10) 应为 10（insertPoint 10<10 不计入）",
            10,
            p.wedgeStart(RAW_LENGTH_TEN),
        )
    }

    // ===== 边界 4：rawOffset 超出范围 → coerceIn(0, rawLength) =====

    @Test
    fun probe_negativeOffset_coercedToZero() {
        val p = EditorSoftBreakProjection(rawLength = RAW_LENGTH_TEN, insertPoints = listOf(5))
        assertEquals("负 offset 应 coerce 到 0", p.wedgeStart(0), p.wedgeStart(-1))
        assertEquals("负 offset 应 coerce 到 0", p.wedgeEnd(0), p.wedgeEnd(-100))
    }

    @Test
    fun probe_overflowOffset_coercedToRawLength() {
        val p = EditorSoftBreakProjection(rawLength = RAW_LENGTH_TEN, insertPoints = listOf(5))
        assertEquals(
            "超限 offset 应 coerce 到 rawLength",
            p.wedgeStart(RAW_LENGTH_TEN),
            p.wedgeStart(100),
        )
        assertEquals(
            "超限 offset 应 coerce 到 rawLength",
            p.wedgeEnd(RAW_LENGTH_TEN),
            p.wedgeEnd(Int.MAX_VALUE),
        )
    }

    // ===== 不变量 1：wedgeStart(rawOffset) <= wedgeEnd(rawOffset) =====

    /**
     * 对所有 rawOffset，wedgeStart <= wedgeEnd（Start 计数 < 的数量 <= End 计数 <= 的数量）。
     */
    @Test
    fun probe_invariant_wedgeStartLeWedgeEnd() {
        val p = EditorSoftBreakProjection(rawLength = RAW_LENGTH_TEN, insertPoints = listOf(0, 3, 5, 7, 10))
        for (i in 0..RAW_LENGTH_TEN) {
            assertTrue(
                "wedgeStart($i) <= wedgeEnd($i) 必须成立",
                p.wedgeStart(i) <= p.wedgeEnd(i),
            )
        }
    }

    // ===== 不变量 2：rawToDisplay == wedgeEnd（range 映射用 wedge End 语义）=====

    @Test
    fun probe_invariant_rawToDisplayEqualsWedgeEnd() {
        val p = EditorSoftBreakProjection(rawLength = RAW_LENGTH_TEN, insertPoints = listOf(3, 7))
        for (i in 0..RAW_LENGTH_TEN) {
            assertEquals(
                "rawToDisplay($i) 应等于 wedgeEnd($i)（range 映射用 wedge End 语义）",
                p.rawToDisplay(i),
                p.wedgeEnd(i),
            )
        }
    }

    // ===== 不变量 3：rawToDisplayCaret 一致性 =====

    @Test
    fun probe_invariant_rawToDisplayCaretConsistency() {
        val p = EditorSoftBreakProjection(rawLength = RAW_LENGTH_TEN, insertPoints = listOf(3, 7))
        for (i in 0..RAW_LENGTH_TEN) {
            assertEquals(
                "rawToDisplayCaret($i, Start) 应等于 wedgeStart($i)",
                p.wedgeStart(i),
                p.rawToDisplayCaret(i, EditorSoftBreakProjection.CaretAffinity.Start),
            )
            assertEquals(
                "rawToDisplayCaret($i, End) 应等于 wedgeEnd($i)",
                p.wedgeEnd(i),
                p.rawToDisplayCaret(i, EditorSoftBreakProjection.CaretAffinity.End),
            )
        }
    }

    // ===== 不变量 4：wedgeStart/wedgeEnd 单调不减 =====

    @Test
    fun probe_invariant_wedgeMonotonicNonDecreasing() {
        val p = EditorSoftBreakProjection(rawLength = RAW_LENGTH_TEN, insertPoints = listOf(3, 7))
        var prevStart = p.wedgeStart(0)
        var prevEnd = p.wedgeEnd(0)
        for (i in 1..RAW_LENGTH_TEN) {
            val curStart = p.wedgeStart(i)
            val curEnd = p.wedgeEnd(i)
            assertTrue(
                "wedgeStart 单调不减：wedgeStart($i)=$curStart >= wedgeStart(${i - 1})=$prevStart",
                curStart >= prevStart,
            )
            assertTrue(
                "wedgeEnd 单调不减：wedgeEnd($i)=$curEnd >= wedgeEnd(${i - 1})=$prevEnd",
                curEnd >= prevEnd,
            )
            prevStart = curStart
            prevEnd = curEnd
        }
    }

    // ===== 幂等性：displayToRaw(rawToDisplay(x)) == x（range 映射往返）=====

    @Test
    fun probe_idempotent_displayToRawRoundTrip() {
        val p = EditorSoftBreakProjection(rawLength = RAW_LENGTH_TEN, insertPoints = listOf(3, 7))
        for (i in 0..RAW_LENGTH_TEN) {
            val display = p.rawToDisplay(i)
            val back = p.displayToRaw(display)
            assertEquals(
                "displayToRaw(rawToDisplay($i)) 应恢复 $i（range 映射往返幂等）",
                i,
                back,
            )
        }
    }

    // ===== 诊断包证据场景：51 长度，insertPoint=50 =====

    /**
     * 诊断包：selectionEnd=51 自绘 caret y=287，selectionEnd=50 y=212。
     * 补丁后 wedgeStart(50)=50（Start affinity，caret 落在 U+200B 之前），
     * wedgeEnd(50)=51（End affinity，caret 落在 U+200B 之后）。
     * 本地输入产生的 collapsed caret 使用 Start，不再统一走 wedge End 跳行。
     */
    @Test
    fun probe_diagnosticBundle_wedgeStartPreventsLineJump() {
        val p = EditorSoftBreakProjection(rawLength = RAW_LENGTH_FIFTY_ONE, insertPoints = listOf(50))
        assertEquals(52, p.displayLength)
        // Start affinity：caret 落在 U+200B 之前，不跳行
        assertEquals(
            "wedgeStart(50)=50（Start affinity，caret 在 U+200B 前，不跳行）",
            50,
            p.wedgeStart(50),
        )
        // End affinity：caret 落在 U+200B 之后（旧行为，导致跳行）
        assertEquals(
            "wedgeEnd(50)=51（End affinity，caret 在 U+200B 后，旧行为跳行）",
            51,
            p.wedgeEnd(50),
        )
        // 补丁核心：rawToDisplayCaret 用 Start affinity，避免跳行
        assertEquals(
            "rawToDisplayCaret(50, Start)=50，本地输入 collapsed caret 不跳行",
            50,
            p.rawToDisplayCaret(50, EditorSoftBreakProjection.CaretAffinity.Start),
        )
    }

    // ===== 多插入点累积 =====

    @Test
    fun probe_multipleInserts_cumulativeCounting() {
        val p = EditorSoftBreakProjection(rawLength = 20, insertPoints = listOf(5, 10, 15))
        assertEquals(23, p.displayLength)
        // wedgeStart: count(insertPoint < rawOffset)
        assertEquals("wedgeStart(0)=0", 0, p.wedgeStart(0))
        assertEquals("wedgeStart(5)=5（5<5 false）", 5, p.wedgeStart(5))
        assertEquals("wedgeStart(6)=7（5<6 true, 10<6 false → +1）", 7, p.wedgeStart(6))
        assertEquals("wedgeStart(10)=11（5<10 true, 10<10 false, 15<10 false → +1）", 11, p.wedgeStart(10))
        assertEquals("wedgeStart(20)=23（全部 < 20 → +3）", 23, p.wedgeStart(20))
        // wedgeEnd: count(insertPoint <= rawOffset)
        assertEquals("wedgeEnd(5)=6（5<=5 true → +1）", 6, p.wedgeEnd(5))
        assertEquals("wedgeEnd(10)=12（5<=10, 10<=10, 15<=10 false → +2）", 12, p.wedgeEnd(10))
        assertEquals("wedgeEnd(20)=23（全部 <= 20 → +3）", 23, p.wedgeEnd(20))
    }
}
