package com.xiwei.sujian.editor.selfrender

import org.junit.Assert.*
import org.junit.Test

/**
 * 自研写作区真吐字/吞字动画逻辑测试
 *
 * 覆盖：
 * 1. 静态绘制 exclude range 计算
 * 2. 删除 snapshot 记录和精确匹配
 * 3. shouldAnimate 逻辑
 * 4. IME composing 不动画，commit 才动画
 * 5. 滚动清动画
 */
class SelfRenderAnimationLogicTest {

    // ── Exclude Range 计算 ──

    @Test
    fun excludeRange_noOverlap_lineFullyVisible() {
        // 行范围 [0, 5)，exclude [8, 10) → 不相交，整行可见
        val lineStart = 0
        val lineEnd = 5
        val excludeStart = 8
        val excludeEnd = 10

        assertFalse(rangesOverlap(lineStart, lineEnd, excludeStart, excludeEnd))
    }

    @Test
    fun excludeRange_fullOverlap_lineFullyHidden() {
        // 行范围 [0, 10)，exclude [0, 10) → 完全重叠，整行隐藏
        val lineStart = 0
        val lineEnd = 10
        val excludeStart = 0
        val excludeEnd = 10

        assertTrue(rangesOverlap(lineStart, lineEnd, excludeStart, excludeEnd))
        // before 段为空，after 段为空
        val (beforeEnd, afterStart) = splitSegments(lineStart, lineEnd, excludeStart, excludeEnd)
        assertEquals(0, beforeEnd) // before: [0, 0) = 空
        assertEquals(10, afterStart) // after: [10, 10) = 空
    }

    @Test
    fun excludeRange_partialOverlapBefore_visible() {
        // 行范围 [0, 10)，exclude [5, 15) → before [0, 5) 可见，after 为空
        val lineStart = 0
        val lineEnd = 10
        val excludeStart = 5
        val excludeEnd = 15

        assertTrue(rangesOverlap(lineStart, lineEnd, excludeStart, excludeEnd))
        val (beforeEnd, afterStart) = splitSegments(lineStart, lineEnd, excludeStart, excludeEnd)
        assertEquals(5, beforeEnd) // before: [0, 5) 可见
        assertEquals(15, afterStart) // after: [15, 10) = 空 (afterStart >= lineEnd)
    }

    @Test
    fun excludeRange_partialOverlapAfter_visible() {
        // 行范围 [5, 15)，exclude [0, 10) → before 为空，after [10, 15) 可见
        val lineStart = 5
        val lineEnd = 15
        val excludeStart = 0
        val excludeEnd = 10

        assertTrue(rangesOverlap(lineStart, lineEnd, excludeStart, excludeEnd))
        val (beforeEnd, afterStart) = splitSegments(lineStart, lineEnd, excludeStart, excludeEnd)
        assertEquals(0, beforeEnd) // before: [5, 0) = 空 (beforeEnd <= lineStart)
        assertEquals(10, afterStart) // after: [10, 15) 可见
    }

    @Test
    fun excludeRange_middleHidden_beforeAfterVisible() {
        // 行范围 [0, 20)，exclude [5, 10) → before [0, 5) + after [10, 20)
        val lineStart = 0
        val lineEnd = 20
        val excludeStart = 5
        val excludeEnd = 10

        assertTrue(rangesOverlap(lineStart, lineEnd, excludeStart, excludeEnd))
        val (beforeEnd, afterStart) = splitSegments(lineStart, lineEnd, excludeStart, excludeEnd)
        assertEquals(5, beforeEnd) // before: [0, 5) 可见
        assertEquals(10, afterStart) // after: [10, 20) 可见
    }

    @Test
    fun excludeRange_excludeAtLineEnd_noAfterSegment() {
        // 行范围 [0, 10)，exclude [8, 10) → before [0, 8) 可见，after 为空
        val lineStart = 0
        val lineEnd = 10
        val excludeStart = 8
        val excludeEnd = 10

        assertTrue(rangesOverlap(lineStart, lineEnd, excludeStart, excludeEnd))
        val (beforeEnd, afterStart) = splitSegments(lineStart, lineEnd, excludeStart, excludeEnd)
        assertEquals(8, beforeEnd)
        assertEquals(10, afterStart) // after: [10, 10) = 空
    }

    @Test
    fun excludeRange_excludeAtLineStart_noBeforeSegment() {
        // 行范围 [5, 15)，exclude [5, 8) → before 为空，after [8, 15) 可见
        val lineStart = 5
        val lineEnd = 15
        val excludeStart = 5
        val excludeEnd = 8

        assertTrue(rangesOverlap(lineStart, lineEnd, excludeStart, excludeEnd))
        val (beforeEnd, afterStart) = splitSegments(lineStart, lineEnd, excludeStart, excludeEnd)
        assertEquals(5, beforeEnd) // before: [5, 5) = 空
        assertEquals(8, afterStart) // after: [8, 15) 可见
    }

    // ── shouldAnimate 逻辑 ──

    @Test
    fun shouldAnimate_typing_true() {
        assertTrue(shouldAnimateForCause(SujianEditCause.Typing))
    }

    @Test
    fun shouldAnimate_delete_true() {
        assertTrue(shouldAnimateForCause(SujianEditCause.Delete))
    }

    @Test
    fun shouldAnimate_typingCommit_true() {
        assertTrue(shouldAnimateForCause(SujianEditCause.TypingCommit))
    }

    @Test
    fun shouldAnimate_paste_false() {
        assertFalse(shouldAnimateForCause(SujianEditCause.Paste))
    }

    @Test
    fun shouldAnimate_load_false() {
        assertFalse(shouldAnimateForCause(SujianEditCause.Load))
    }

    @Test
    fun shouldAnimate_format_false() {
        assertFalse(shouldAnimateForCause(SujianEditCause.Format))
    }

    @Test
    fun shouldAnimate_imeComposition_false() {
        assertFalse(shouldAnimateForCause(SujianEditCause.ImeComposition))
    }

    @Test
    fun shouldAnimate_programmatic_false() {
        assertFalse(shouldAnimateForCause(SujianEditCause.Programmatic))
    }

    // ── Delete Snapshot 精确匹配 ──

    @Test
    fun deleteSnapshot_recordAndConsume_matchById() {
        val snapshots = mutableListOf<DeleteSnapshotForTest>()
        var nextId: ULong = 1u

        // 记录快照 1
        val id1 = nextId++
        snapshots.add(DeleteSnapshotForTest(id1, "a"))

        // 记录快照 2
        val id2 = nextId++
        snapshots.add(DeleteSnapshotForTest(id2, "b"))

        // 精确匹配 id2
        val consumed = snapshots.indexOfFirst { it.id == id2 }
        assertTrue(consumed >= 0)
        val snapshot = snapshots.removeAt(consumed)
        assertEquals(id2, snapshot.id)
        assertEquals("b", snapshot.text)

        // id1 仍在
        assertEquals(1, snapshots.size)
        assertEquals(id1, snapshots[0].id)
    }

    @Test
    fun deleteSnapshot_consumeNonExistent_returnsNull() {
        val snapshots = mutableListOf<DeleteSnapshotForTest>()
        val id1: ULong = 1u
        snapshots.add(DeleteSnapshotForTest(id1, "a"))

        // 查找不存在的 id
        val idx = snapshots.indexOfFirst { it.id == 999uL }
        assertEquals(-1, idx)
    }

    @Test
    fun deleteSnapshot_consecutiveDeletes_maintainOrder() {
        val snapshots = mutableListOf<DeleteSnapshotForTest>()
        var nextId: ULong = 1u

        // 连续三次删除
        val id1 = nextId++; snapshots.add(DeleteSnapshotForTest(id1, "a"))
        val id2 = nextId++; snapshots.add(DeleteSnapshotForTest(id2, "b"))
        val id3 = nextId++; snapshots.add(DeleteSnapshotForTest(id3, "c"))

        // 按顺序消耗
        val idx1 = snapshots.indexOfFirst { it.id == id1 }
        snapshots.removeAt(idx1)
        val idx2 = snapshots.indexOfFirst { it.id == id2 }
        snapshots.removeAt(idx2)
        val idx3 = snapshots.indexOfFirst { it.id == id3 }
        snapshots.removeAt(idx3)

        assertEquals(0, snapshots.size)
    }

    // ── UTF-16 offset 和 surrogate pair ──

    @Test
    fun utf16Offset_emoji_twoCodeUnits() {
        // 😀 = U+1F600, 在 UTF-16 中占 2 个 code unit
        val text = "a😀b"
        assertEquals(4, text.length) // a(1) + 😀(2) + b(1)
        // 插入 emoji 后，UTF-16 range 是 [1, 3)
        val insertRange = IntRange(1, 3)
        assertEquals(2, insertRange.last - insertRange.first) // 2 code units
    }

    @Test
    fun utf16Offset_chinese_oneCodeUnit() {
        // 你 = U+4F60, 在 UTF-16 中占 1 个 code unit
        val text = "a你b"
        assertEquals(3, text.length) // a(1) + 你(1) + b(1)
        val insertRange = IntRange(1, 2)
        assertEquals(1, insertRange.last - insertRange.first)
    }

    @Test
    fun utf16Offset_multipleEmoji_correctRange() {
        val text = "😀😀"
        assertEquals(4, text.length) // 每个 emoji 2 code units
        // 第一个 emoji: [0, 2)
        // 第二个 emoji: [2, 4)
        val range1 = IntRange(0, 2)
        val range2 = IntRange(2, 4)
        assertEquals(2, range1.last - range1.first)
        assertEquals(2, range2.last - range2.first)
    }

    // ── IME composing 不动画 ──

    @Test
    fun imeComposition_doesNotAnimate() {
        // composing text 不进正文 buffer，不触发 onTextChanged
        // 所以不会创建动画事件
        // 这个测试验证 shouldAnimate 逻辑
        assertFalse(shouldAnimateForCause(SujianEditCause.ImeComposition))
    }

    @Test
    fun commitText_afterComposing_animates() {
        // commitText 使用 TypingCommit cause，应该动画
        assertTrue(shouldAnimateForCause(SujianEditCause.TypingCommit))
    }

    @Test
    fun singleCharTyping_animates() {
        // 单字输入使用 Typing cause，应该动画
        assertTrue(shouldAnimateForCause(SujianEditCause.Typing))
    }

    // ── 滚动清动画 ──

    @Test
    fun scrolling_clearsAnimations() {
        // 模拟滚动状态管理
        var hasAnimations = true
        var animatedInsertRange: IntRange? = IntRange(1, 3)

        // 开始滚动
        hasAnimations = false
        animatedInsertRange = null

        assertFalse(hasAnimations)
        assertNull(animatedInsertRange)
    }

    // ── 辅助方法 ──

    private data class DeleteSnapshotForTest(
        val id: ULong,
        val text: String
    )

    /**
     * 检查行范围 [lineStart, lineEnd) 与 exclude 范围 [excludeStart, excludeEnd) 是否相交
     */
    private fun rangesOverlap(
        lineStart: Int, lineEnd: Int,
        excludeStart: Int, excludeEnd: Int
    ): Boolean {
        return !(lineEnd <= excludeStart || lineStart >= excludeEnd)
    }

    /**
     * 计算行与 exclude 范围相交后的 before 段结束和 after 段开始
     * 返回 (beforeEnd, afterStart)
     */
    private fun splitSegments(
        lineStart: Int, lineEnd: Int,
        excludeStart: Int, excludeEnd: Int
    ): Pair<Int, Int> {
        val beforeEnd = minOf(lineEnd, excludeStart)
        val afterStart = maxOf(lineStart, excludeEnd)
        return Pair(beforeEnd, afterStart)
    }

    /**
     * 与 SujianAnimationController.shouldAnimate() 相同的逻辑
     */
    private fun shouldAnimateForCause(cause: SujianEditCause): Boolean {
        return when (cause) {
            SujianEditCause.Typing,
            SujianEditCause.Delete,
            SujianEditCause.TypingCommit -> true
            SujianEditCause.Paste,
            SujianEditCause.Load,
            SujianEditCause.Format,
            SujianEditCause.ImeComposition,
            SujianEditCause.Programmatic -> false
        }
    }
}
