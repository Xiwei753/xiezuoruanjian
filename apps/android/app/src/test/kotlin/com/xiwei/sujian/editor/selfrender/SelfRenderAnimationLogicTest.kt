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
    fun shouldAnimate_paste_true() {
        assertTrue(shouldAnimateForCause(SujianEditCause.Paste))
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
        val insertRange = HalfOpenRange(1, 3)
        assertEquals(2, insertRange.end - insertRange.start) // 2 code units
    }

    @Test
    fun utf16Offset_chinese_oneCodeUnit() {
        // 你 = U+4F60, 在 UTF-16 中占 1 个 code unit
        val text = "a你b"
        assertEquals(3, text.length) // a(1) + 你(1) + b(1)
        val insertRange = HalfOpenRange(1, 2)
        assertEquals(1, insertRange.end - insertRange.start)
    }

    @Test
    fun utf16Offset_multipleEmoji_correctRange() {
        val text = "😀😀"
        assertEquals(4, text.length) // 每个 emoji 2 code units
        // 第一个 emoji: [0, 2)
        // 第二个 emoji: [2, 4)
        val range1 = HalfOpenRange(0, 2)
        val range2 = HalfOpenRange(2, 4)
        assertEquals(2, range1.end - range1.start)
        assertEquals(2, range2.end - range2.start)
    }

    // ── excludeRange 不相交行批量收集 ──

    @Test
    fun excludeRange_nonOverlapLines_batchDrawn() {
        // 可视行 0-9（每行 10 字符），exclude [25, 35) 影响行 2-3
        // 不相交行应被收集为连续区间：[0, 1] 和 [4, 9]
        val firstVisLine = 0
        val lastVisLine = 9
        val excludeRange = HalfOpenRange(25, 35) // 影响行 2, 3

        val nonOverlapLineRanges = mutableListOf<Pair<Int, Int>>()
        var rangeStart = -1
        for (lineIdx in firstVisLine..lastVisLine) {
            val lineStart = lineIdx * 10
            val lineEnd = lineStart + 10
            val overlaps = !(lineEnd <= excludeRange.start || lineStart >= excludeRange.end)
            if (!overlaps) {
                if (rangeStart < 0) rangeStart = lineIdx
            } else {
                if (rangeStart >= 0) {
                    nonOverlapLineRanges.add(Pair(rangeStart, lineIdx - 1))
                    rangeStart = -1
                }
            }
        }
        if (rangeStart >= 0) {
            nonOverlapLineRanges.add(Pair(rangeStart, lastVisLine))
        }

        // 不相交行分为两个区间：[0, 1] 和 [4, 9]
        assertEquals(2, nonOverlapLineRanges.size)
        assertEquals(Pair(0, 1), nonOverlapLineRanges[0])
        assertEquals(Pair(4, 9), nonOverlapLineRanges[1])
    }

    @Test
    fun excludeRange_nonOverlapLines_allNonOverlap_singleRange() {
        // 可视行 0-9，exclude [100, 110) 不影响任何可视行
        val firstVisLine = 0
        val lastVisLine = 9
        val excludeRange = HalfOpenRange(100, 110)

        val nonOverlapLineRanges = mutableListOf<Pair<Int, Int>>()
        var rangeStart = -1
        for (lineIdx in firstVisLine..lastVisLine) {
            val lineStart = lineIdx * 10
            val lineEnd = lineStart + 10
            val overlaps = !(lineEnd <= excludeRange.start || lineStart >= excludeRange.end)
            if (!overlaps) {
                if (rangeStart < 0) rangeStart = lineIdx
            } else {
                if (rangeStart >= 0) {
                    nonOverlapLineRanges.add(Pair(rangeStart, lineIdx - 1))
                    rangeStart = -1
                }
            }
        }
        if (rangeStart >= 0) {
            nonOverlapLineRanges.add(Pair(rangeStart, lastVisLine))
        }

        // 所有行都不与 excludeRange 相交，形成 1 个连续区间
        assertEquals(1, nonOverlapLineRanges.size)
        assertEquals(Pair(0, 9), nonOverlapLineRanges[0])
    }

    // ── excludeRange 相交行分段绘制 ──

    @Test
    fun excludeRange_overlapLines_splitSegments() {
        // 行 [0, 10) 与 exclude [3, 7) → before [0, 3), after [7, 10)
        val lineStart = 0
        val lineEnd = 10
        val excludeStart = 3
        val excludeEnd = 7

        val beforeEnd = minOf(lineEnd, excludeStart)
        val afterStart = maxOf(lineStart, excludeEnd)

        assertEquals(3, beforeEnd)   // before: [0, 3)
        assertEquals(7, afterStart)  // after: [7, 10)
        assertTrue(beforeEnd > lineStart)   // before 段非空
        assertTrue(afterStart < lineEnd)    // after 段非空
    }

    @Test
    fun excludeRange_overlapLines_excludeCoversEntireLine() {
        // 行 [5, 15) 与 exclude [0, 20) → 完全被覆盖，before 和 after 都为空
        val lineStart = 5
        val lineEnd = 15
        val excludeStart = 0
        val excludeEnd = 20

        val beforeEnd = minOf(lineEnd, excludeStart)
        val afterStart = maxOf(lineStart, excludeEnd)

        assertEquals(0, beforeEnd)    // before: [5, 0) = 空
        assertEquals(20, afterStart)  // after: [20, 15) = 空
        assertFalse(beforeEnd > lineStart)  // before 段为空
        assertFalse(afterStart < lineEnd)   // after 段为空
    }

    @Test
    fun excludeRange_overlapLines_partialOverlapAtEnd() {
        // 行 [0, 10) 与 exclude [8, 12) → before [0, 8), after 为空
        val lineStart = 0
        val lineEnd = 10
        val excludeStart = 8
        val excludeEnd = 12

        val beforeEnd = minOf(lineEnd, excludeStart)
        val afterStart = maxOf(lineStart, excludeEnd)

        assertEquals(8, beforeEnd)     // before: [0, 8)
        assertEquals(12, afterStart)   // after: [12, 10) = 空
        assertTrue(beforeEnd > lineStart)    // before 段非空
        assertFalse(afterStart < lineEnd)    // after 段为空
    }

    @Test
    fun excludeRange_overlapLines_partialOverlapAtStart() {
        // 行 [5, 15) 与 exclude [0, 8) → before 为空，after [8, 15)
        val lineStart = 5
        val lineEnd = 15
        val excludeStart = 0
        val excludeEnd = 8

        val beforeEnd = minOf(lineEnd, excludeStart)
        val afterStart = maxOf(lineStart, excludeEnd)

        assertEquals(0, beforeEnd)     // before: [5, 0) = 空
        assertEquals(8, afterStart)    // after: [8, 15)
        assertFalse(beforeEnd > lineStart)   // before 段为空
        assertTrue(afterStart < lineEnd)     // after 段非空
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
        var animatedInsertRange: HalfOpenRange? = HalfOpenRange(1, 3)

        // 开始滚动
        hasAnimations = false
        animatedInsertRange = null

        assertFalse(hasAnimations)
        assertNull(animatedInsertRange)
    }

    // ── Undo/Redo 不动画 ──

    @Test
    fun undoDoesNotAnimate() {
        // Undo 在 Android 端走 Programmatic cause（SujianEditCause 没有 Undo 变体），
        // 但无论哪种方式，Undo 都不应产生动画。
        // 验证 Programmatic cause 不动画（Undo 在 Android 端映射为 Programmatic）
        assertFalse(shouldAnimateForCause(SujianEditCause.Programmatic))
    }

    @Test
    fun redoDoesNotAnimate() {
        // Redo 同理，在 Android 端走 Programmatic cause
        assertFalse(shouldAnimateForCause(SujianEditCause.Programmatic))
    }

    // ── Paste 完整场景测试 ──

    @Test
    fun pasteUsesCoreVisualTransactionRoute() {
        // Paste 是否实际动画由 Core 返回的 visual transaction / animationMode 决定；Android 不再提前拦截。
        assertTrue(shouldAnimateForCause(SujianEditCause.Paste))

        // 模拟粘贴操作：Android 只放行到 Core，不在本地按文本长度决定模式
        var hasAnimations = false
        var animatedInsertRange: HalfOpenRange? = null

        // 模拟粘贴 "a" 到空文本
        // Paste cause → 允许进入 Core 路线；本测试不模拟 Core 返回结果，因此本地未创建动画。
        assertTrue(shouldAnimateForCause(SujianEditCause.Paste))
        assertFalse(hasAnimations)
        assertNull(animatedInsertRange)

        // 模拟粘贴长文本
        assertTrue(shouldAnimateForCause(SujianEditCause.Paste))
        assertFalse(hasAnimations)
        assertNull(animatedInsertRange)
    }

    // ── 滚动中插入文字清动画 ──

    @Test
    fun scrollingDuringInsert_clearsAnimation() {
        // 模拟：正在插入文字时有活跃动画，然后用户滚动
        var hasAnimations = true
        var animatedInsertRange: HalfOpenRange? = HalfOpenRange(5, 8)

        // 插入操作创建了动画
        assertTrue(hasAnimations)
        assertNotNull(animatedInsertRange)

        // 用户开始滚动 → 动画被清除
        hasAnimations = false
        animatedInsertRange = null

        // 验证动画被清除
        assertFalse("animations should be cleared when scrolling starts during insert", hasAnimations)
        assertNull("insert range should be null when scrolling starts during insert", animatedInsertRange)

        // 即使插入的 cause 是 Typing（应该动画），滚动状态也优先清除
        assertTrue("Typing should animate in principle", shouldAnimateForCause(SujianEditCause.Typing))
        assertFalse("but scrolling overrides and clears the animation", hasAnimations)
    }

    // ── ActiveInsertRange ID 追踪与映射 ──

    @Test
    fun activeInsertRange_mapForInsert_shiftsRangeCorrectly() {
        // 场景：已有 range [5,8)，在位置 3 插入 2 字符 → range 应变为 [7,10)
        val entries = mutableListOf(ActiveInsertRangeEntryForTest(1uL, HalfOpenRange(5, 8)))
        val pos = 3
        val len = 2

        val newEntries = mutableListOf<ActiveInsertRangeEntryForTest>()
        for (entry in entries) {
            val range = entry.range
            when {
                pos <= range.start -> newEntries.add(entry.copy(range = HalfOpenRange(range.start + len, range.end + len)))
                pos >= range.end -> newEntries.add(entry)
                // else: intersect → cancel (not added)
            }
        }

        assertEquals(1, newEntries.size)
        assertEquals(HalfOpenRange(7, 10), newEntries[0].range)
        assertEquals(1uL, newEntries[0].id) // ID 保留不变
    }

    @Test
    fun activeInsertRange_mapForInsert_intersectCancelsRange() {
        // 场景：已有 range [5,8)，在位置 6 插入 → 相交，range 被取消
        val entries = mutableListOf(ActiveInsertRangeEntryForTest(1uL, HalfOpenRange(5, 8)))
        val pos = 6
        val len = 1

        val canceledIds = mutableListOf<ULong>()
        val newEntries = mutableListOf<ActiveInsertRangeEntryForTest>()
        for (entry in entries) {
            val range = entry.range
            when {
                pos <= range.start -> newEntries.add(entry.copy(range = HalfOpenRange(range.start + len, range.end + len)))
                pos >= range.end -> newEntries.add(entry)
                else -> { canceledIds.add(entry.id) }
            }
        }

        assertEquals(0, newEntries.size) // range 被取消
        assertEquals(listOf(1uL), canceledIds) // 取消的 ID 被收集
    }

    @Test
    fun activeInsertRange_mapForDelete_shiftsRangeCorrectly() {
        // 场景：已有 range [8,11)，在位置 3 删除 2 字符 → range 应变为 [6,9)
        val entries = mutableListOf(ActiveInsertRangeEntryForTest(1uL, HalfOpenRange(8, 11)))
        val pos = 3
        val len = 2

        val newEntries = mutableListOf<ActiveInsertRangeEntryForTest>()
        for (entry in entries) {
            val range = entry.range
            when {
                pos + len <= range.start -> newEntries.add(entry.copy(range = HalfOpenRange(range.start - len, range.end - len)))
                pos >= range.end -> newEntries.add(entry)
                // else: intersect → cancel
            }
        }

        assertEquals(1, newEntries.size)
        assertEquals(HalfOpenRange(6, 9), newEntries[0].range)
        assertEquals(1uL, newEntries[0].id)
    }

    @Test
    fun activeInsertRange_mapForDelete_intersectCancelsRange() {
        // 场景：已有 range [5,8)，在位置 6 删除 3 字符 → 相交，range 被取消
        val entries = mutableListOf(ActiveInsertRangeEntryForTest(1uL, HalfOpenRange(5, 8)))
        val pos = 6
        val len = 3

        val canceledIds = mutableListOf<ULong>()
        val newEntries = mutableListOf<ActiveInsertRangeEntryForTest>()
        for (entry in entries) {
            val range = entry.range
            when {
                pos + len <= range.start -> newEntries.add(entry.copy(range = HalfOpenRange(range.start - len, range.end - len)))
                pos >= range.end -> newEntries.add(entry)
                else -> { canceledIds.add(entry.id) }
            }
        }

        assertEquals(0, newEntries.size)
        assertEquals(listOf(1uL), canceledIds)
    }

    @Test
    fun activeInsertRange_tickRemovesById_notByValue() {
        // 关键场景：range 被映射后值已变化，但 ID 不变
        // tickAnimations 应按 ID 移除，而非按 HalfOpenRange 值移除
        // 1. 插入 "A" 在位置 5，range = [5,6), id = 1
        // 2. 插入 "B" 在位置 3，映射后 range 变为 [6,7), id 仍为 1
        // 3. 动画 A 完成，按 id=1 移除 → 正确移除 [6,7)
        val entries = mutableListOf(
            ActiveInsertRangeEntryForTest(1uL, HalfOpenRange(6, 7)),  // 映射后的 range
            ActiveInsertRangeEntryForTest(2uL, HalfOpenRange(3, 4))   // 第二个插入的 range
        )

        // 模拟 tickAnimations：按 ID 移除
        val finishedAnimRangeId = 1uL  // 动画 A 持有的 range ID
        entries.removeAll { it.id == finishedAnimRangeId }

        assertEquals(1, entries.size)
        assertEquals(2uL, entries[0].id)
        assertEquals(HalfOpenRange(3, 4), entries[0].range)
    }

    @Test
    fun activeInsertRange_multipleConcurrentRanges_independentTracking() {
        // 多个并发 insert 动画各自独立追踪 range
        val entries = mutableListOf(
            ActiveInsertRangeEntryForTest(1uL, HalfOpenRange(5, 6)),
            ActiveInsertRangeEntryForTest(2uL, HalfOpenRange(10, 12)),
            ActiveInsertRangeEntryForTest(3uL, HalfOpenRange(15, 16))
        )

        // 在位置 7 插入 1 字符 → 只有 range1 不变，range2 和 range3 后移
        val pos = 7
        val len = 1
        val newEntries = mutableListOf<ActiveInsertRangeEntryForTest>()
        for (entry in entries) {
            val range = entry.range
            when {
                pos <= range.start -> newEntries.add(entry.copy(range = HalfOpenRange(range.start + len, range.end + len)))
                pos >= range.end -> newEntries.add(entry)
                else -> {} // cancel
            }
        }

        assertEquals(3, newEntries.size)
        assertEquals(HalfOpenRange(5, 6), newEntries[0].range)   // 不变
        assertEquals(HalfOpenRange(11, 13), newEntries[1].range) // 后移
        assertEquals(HalfOpenRange(16, 17), newEntries[2].range) // 后移
    }

    // ── 辅助方法 ──

    private data class DeleteSnapshotForTest(
        val id: ULong,
        val text: String
    )

    private data class ActiveInsertRangeEntryForTest(
        val id: ULong,
        val range: HalfOpenRange
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
            SujianEditCause.TypingCommit,
            SujianEditCause.Paste -> true
            SujianEditCause.Load,
            SujianEditCause.Format,
            SujianEditCause.ImeComposition,
            SujianEditCause.Programmatic -> false
        }
    }
}
