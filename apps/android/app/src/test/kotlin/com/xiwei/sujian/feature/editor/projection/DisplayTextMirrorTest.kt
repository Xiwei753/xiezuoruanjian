package com.xiwei.sujian.feature.editor.projection

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
@Suppress("LargeClass")
class DisplayTextMirrorTest {
    @Test
    fun loadText_setsInitialContent() {
        val mirror = DisplayTextMirror()
        mirror.loadText("Hello", 5)
        assertEquals("Hello", mirror.getText())
        assertEquals(5, mirror.getCursorUtf8())
        assertEquals(0, mirror.getRevision())
    }

    @Test
    fun applyPatches_insertsText() {
        val mirror = DisplayTextMirror()
        mirror.loadText("ab", 2)

        val result =
            EditResult(
                outcome = uniffi.writer_core.EditorEditOutcomeDto.APPLIED,
                transactionId = 1,
                baseRevision = 0,
                newRevision = 1,
                displayPatches =
                    listOf(
                        DisplayPatch(
                            baseRevision = 0,
                            newRevision = 1,
                            replaceByteStart = 2,
                            replaceByteEndExclusive = 2,
                            insertedText = "c",
                            resultingSelectionStart = 3,
                            resultingSelectionEnd = 3,
                        ),
                    ),
                oldSelectionStart = 2,
                oldSelectionEnd = 2,
                newSelectionStart = 3,
                newSelectionEnd = 3,
                visualIntent =
                    VisualIntent(
                        cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
                        operationKind = uniffi.writer_core.EditorOperationKindDto.INSERT,
                        oldAffectedByteRanges = emptyList(),
                        newAffectedByteRanges = listOf(Pair(2, 3)),
                        animationMode = uniffi.writer_core.AnimationModeDto.GLYPH_ANIMATION,
                        durationMs = 160,
                        coordinatedCursor = CoordinatedCursor(2, 3, true),
                    ),
            )

        mirror.applyEditResult(result)
        assertEquals("abc", mirror.getText())
        assertEquals(3, mirror.getCursorUtf8())
        assertEquals(1, mirror.getRevision())
    }

    @Test
    fun applyPatches_deletesText() {
        val mirror = DisplayTextMirror()
        mirror.loadText("abc", 3)

        val result =
            EditResult(
                outcome = uniffi.writer_core.EditorEditOutcomeDto.APPLIED,
                transactionId = 1,
                baseRevision = 0,
                newRevision = 1,
                displayPatches =
                    listOf(
                        DisplayPatch(
                            baseRevision = 0,
                            newRevision = 1,
                            replaceByteStart = 2,
                            replaceByteEndExclusive = 3,
                            insertedText = "",
                            resultingSelectionStart = 2,
                            resultingSelectionEnd = 2,
                        ),
                    ),
                oldSelectionStart = 3,
                oldSelectionEnd = 3,
                newSelectionStart = 2,
                newSelectionEnd = 2,
                visualIntent =
                    VisualIntent(
                        cause = uniffi.writer_core.EditorTransactionCauseDto.DELETE,
                        operationKind = uniffi.writer_core.EditorOperationKindDto.DELETE,
                        oldAffectedByteRanges = listOf(Pair(2, 3)),
                        newAffectedByteRanges = emptyList(),
                        animationMode = uniffi.writer_core.AnimationModeDto.GLYPH_ANIMATION,
                        durationMs = 160,
                        coordinatedCursor = CoordinatedCursor(3, 2, true),
                    ),
            )

        mirror.applyEditResult(result)
        assertEquals("ab", mirror.getText())
        assertEquals(2, mirror.getCursorUtf8())
    }

    @Test
    fun applyPatches_replacesText() {
        val mirror = DisplayTextMirror()
        mirror.loadText("abc", 3)

        val result =
            EditResult(
                outcome = uniffi.writer_core.EditorEditOutcomeDto.APPLIED,
                transactionId = 1,
                baseRevision = 0,
                newRevision = 1,
                displayPatches =
                    listOf(
                        DisplayPatch(
                            baseRevision = 0,
                            newRevision = 1,
                            replaceByteStart = 1,
                            replaceByteEndExclusive = 2,
                            insertedText = "X",
                            resultingSelectionStart = 2,
                            resultingSelectionEnd = 2,
                        ),
                    ),
                oldSelectionStart = 3,
                oldSelectionEnd = 3,
                newSelectionStart = 2,
                newSelectionEnd = 2,
                visualIntent =
                    VisualIntent(
                        cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
                        operationKind = uniffi.writer_core.EditorOperationKindDto.REPLACE,
                        oldAffectedByteRanges = listOf(Pair(1, 2)),
                        newAffectedByteRanges = listOf(Pair(1, 2)),
                        animationMode = uniffi.writer_core.AnimationModeDto.GLYPH_ANIMATION,
                        durationMs = 160,
                        coordinatedCursor = CoordinatedCursor(3, 2, true),
                    ),
            )

        mirror.applyEditResult(result)
        assertEquals("aXc", mirror.getText())
    }

    @Test(expected = IllegalStateException::class)
    fun applyPatches_rejectsStaleRevisions() {
        val mirror = DisplayTextMirror()
        mirror.loadText("ab", 2)

        // 真正的 stale：patch.baseRevision (5) != mirror.currentRevision (0)。
        // 进入 batch 时只校验一次 base revision，不匹配即抛 revision discontinuity。
        // （旧实现用两个共享 revision 的 patch 借第二个 patch 触发 discontinuity，
        // 但在原子 batch 协议下那是合法 batch——故改为真正 base 不匹配。）
        val patches =
            listOf(
                DisplayPatch(5, 6, 2, 2, "c", 3, 3),
            )

        mirror.applyPatches(patches)
    }

    // ── Issue #624 评论 #10 第 3 项：原子 patch batch ──
    // 一个 EditorEditResult 是一个原子 batch：所有 patch 共享同一组 base→new revision，
    // batch 内 patch range 都使用 base 文档坐标（编辑前文本）。进入 batch 只校验一次
    // base revision，按 replaceByteStart 降序应用（右侧修改不改变左侧旧坐标），
    // 全部完成后只更新一次 currentRevision，不给 patch 人造中间 revision。

    /**
     * 原子 batch：两个 patch 共享 baseRevision=5→newRevision=6，降序应用。
     * 旧实现逐个校验 baseRevision，第二个 patch 会用旧 baseRevision 校验触发
     * revision discontinuity 异常。修复后应正常应用且 currentRevision==6。
     */
    @Test
    fun applyPatches_atomic_batch_shared_revision() {
        val mirror = DisplayTextMirror()
        // base 文本 "0123456789"（10 字节），currentRevision=5。
        mirror.loadFromSnapshot("0123456789", cursorUtf8 = 10, revision = 5)

        // 两个 patch 共享 baseRevision=5, newRevision=6（原子 batch）。
        // patch range 都用 base 文档坐标：
        //   patch A: [8,10)→"X"（替换 "89"）
        //   patch B: [3,4)→"Y"（替换 "3"）
        // 降序应用：先 A ([8,10)→"X") 再 B ([3,4)→"Y")。
        // 结果：base "0123456789" → A 后 "01234567X" → B 后 "012Y4567X"
        val patches =
            listOf(
                // patch B（replaceByteStart=3，降序后排第二）
                DisplayPatch(
                    baseRevision = 5,
                    newRevision = 6,
                    replaceByteStart = 3,
                    replaceByteEndExclusive = 4,
                    insertedText = "Y",
                    resultingSelectionStart = 4,
                    resultingSelectionEnd = 4,
                ),
                // patch A（replaceByteStart=8，降序后排第一）
                DisplayPatch(
                    baseRevision = 5,
                    newRevision = 6,
                    replaceByteStart = 8,
                    replaceByteEndExclusive = 10,
                    insertedText = "X",
                    resultingSelectionStart = 9,
                    resultingSelectionEnd = 9,
                ),
            )

        mirror.applyPatches(patches)

        // 降序局部应用：先 [8,10)→"X" 得 "01234567X"，再 [3,4)→"Y" 得 "012Y4567X"。
        assertEquals("012Y4567X", mirror.getText())
        // 整个 batch 完成后 currentRevision 一次更新为 6（不人造中间 revision）。
        assertEquals(6, mirror.getRevision())
    }

    /**
     * batch 进入时只校验一次 base revision：first patch 的 baseRevision 不匹配
     * currentRevision 即抛 revision discontinuity，不应用任何 patch。
     */
    @Test(expected = IllegalStateException::class)
    fun applyPatches_batch_validates_base_revision_once() {
        val mirror = DisplayTextMirror()
        mirror.loadFromSnapshot("0123456789", cursorUtf8 = 10, revision = 5)

        // batch 的 first patch baseRevision=7 != currentRevision=5 → 抛异常。
        val patches =
            listOf(
                DisplayPatch(7, 8, 8, 10, "X", 9, 9),
                DisplayPatch(7, 8, 3, 4, "Y", 4, 4),
            )

        mirror.applyPatches(patches)
    }

    /**
     * batch 内 patch 降序应用后 buffer 与逐个独立应用的一致性：相同 base 坐标的多个
     * 不重叠 replace，降序 batch 一次应用的结果应等于逐个独立应用的结果。
     */
    @Test
    fun applyPatches_batch_descending_equals_sequential_independent() {
        // 场景：base "abcdefghij"（10 字节），两个不重叠 replace：
        //   [8,10)→"XY"（替换 "ij"）
        //   [2,3)→"Z"（替换 "c"）
        // 降序 batch：先 [8,10)→"XY" 再 [2,3)→"Z" → "abZdefghXY"
        // 逐个独立：先 [8,10)→"XY"（rev 0→1）得 "abcdefghXY"，
        //   再 [2,3)→"Z"（rev 1→2）得 "abZdefghXY"。两者一致。
        val batchMirror = DisplayTextMirror()
        batchMirror.loadFromSnapshot("abcdefghij", cursorUtf8 = 10, revision = 0)
        val batchPatches =
            listOf(
                DisplayPatch(0, 1, 2, 3, "Z", 3, 3),
                DisplayPatch(0, 1, 8, 10, "XY", 10, 10),
            )
        batchMirror.applyPatches(batchPatches)

        val sequentialMirror = DisplayTextMirror()
        sequentialMirror.loadFromSnapshot("abcdefghij", cursorUtf8 = 10, revision = 0)
        sequentialMirror.applyPatches(listOf(DisplayPatch(0, 1, 8, 10, "XY", 10, 10)))
        sequentialMirror.applyPatches(listOf(DisplayPatch(1, 2, 2, 3, "Z", 3, 3)))

        assertEquals(sequentialMirror.getText(), batchMirror.getText())
        assertEquals("abZdefghXY", batchMirror.getText())
    }

    /**
     * #624 评论10 第4项补漏：deleteSurrounding 的 **undo** patch batch（最终文本坐标）。
     * 旧实现 after delta 的 new_range 用「仅删除 after」时刻的坐标，before 删除后该点
     * 左移，Android 降序应用时 after patch 插到错误位置，mirror 与 Core 分裂。
     *
     * 非紧邻："abYd" + undo patches [after (3,3)→"c", before (2,2)→"X"]
     * 降序应用 → "abXYcd"（Core undo snapshot）。
     */
    @Test
    fun applyPatches_deleteSurrounding_undo_batch_final_coords() {
        val mirror = DisplayTextMirror()
        // undo 前文本 = deleteSurrounding 后的最终文本 "abYd"，revision 5。
        mirror.loadFromSnapshot("abYd", cursorUtf8 = 2, revision = 5)

        val patches =
            listOf(
                // after delta：new_range=point(3)（最终坐标：as_=4 左移 before_deleted_len=1）。
                DisplayPatch(5, 6, 3, 3, "c", 3, 3),
                // before delta：new_range=point(2)。
                DisplayPatch(5, 6, 2, 2, "X", 3, 3),
            )

        mirror.applyPatches(patches)

        // 降序应用：先 [3,3)→"c" → "abYcd"，再 [2,2)→"X" → "abXYcd"。
        assertEquals("abXYcd", mirror.getText())
        assertEquals(6, mirror.getRevision())
    }

    /**
     * #624 评论10 第4项补漏：before/after 紧邻时两个 undo patch 退化为同一位置
     * point(bs)。列表顺序（after 在前）在 Android 稳定降序下保持，先插 "c" 后插
     * "b" → "abcd"；若顺序颠倒会得 "abdc"。
     */
    @Test
    fun applyPatches_deleteSurrounding_adjacent_undo_same_position_order() {
        val mirror = DisplayTextMirror()
        // undo 前文本 = "ad"（"abcd" 删除 before=[1,2) 与 after=[2,3) 之后）。
        mirror.loadFromSnapshot("ad", cursorUtf8 = 1, revision = 5)

        val patches =
            listOf(
                // after delta：new_range=point(2-1)=point(1)。
                DisplayPatch(5, 6, 1, 1, "c", 1, 1),
                // before delta：new_range=point(1)。
                DisplayPatch(5, 6, 1, 1, "b", 1, 1),
            )

        mirror.applyPatches(patches)

        assertEquals("abcd", mirror.getText())
        assertEquals(6, mirror.getRevision())
    }

    /**
     * #624 评论10 第4项：replace-all 的 undo patch 使用 undo 前文本（= 替换后
     * 最终文本）坐标。变长替换（X→YY）后第二处 new_range 右移（[4,6) 而不是
     * [3,4)）；batch 降序应用必须还原原文，只更新一次 revision。
     */
    @Test
    fun applyPatches_replaceAllUndo_batch_final_coords() {
        val mirror = DisplayTextMirror()
        // undo 前文本 = replace-all 后的 "aYYbYYc"，revision 5。
        mirror.loadFromSnapshot("aYYbYYc", cursorUtf8 = 7, revision = 5)

        val patches =
            listOf(
                // 第二处（新文本坐标 [4,6)，起点右移 cumulative_diff=1）→ "X"。
                DisplayPatch(5, 6, 4, 6, "X", 3, 3),
                // 第一处 [1,3) → "X"。
                DisplayPatch(5, 6, 1, 3, "X", 3, 3),
            )

        mirror.applyPatches(patches)

        // 降序应用：先 [4,6)→"X" → "aYYbXc"，再 [1,3)→"X" → "aXbXc"。
        assertEquals("aXbXc", mirror.getText())
        // 整个 batch 完成后只更新一次 revision。
        assertEquals(6, mirror.getRevision())
    }

    @Test
    fun applyEditResult_handlesChineseText() {
        val mirror = DisplayTextMirror()
        mirror.loadText("你好", 6)

        val result =
            EditResult(
                outcome = uniffi.writer_core.EditorEditOutcomeDto.APPLIED,
                transactionId = 1,
                baseRevision = 0,
                newRevision = 1,
                displayPatches =
                    listOf(
                        DisplayPatch(
                            baseRevision = 0,
                            newRevision = 1,
                            replaceByteStart = 6,
                            replaceByteEndExclusive = 6,
                            insertedText = "世",
                            resultingSelectionStart = 9,
                            resultingSelectionEnd = 9,
                        ),
                    ),
                oldSelectionStart = 6,
                oldSelectionEnd = 6,
                newSelectionStart = 9,
                newSelectionEnd = 9,
                visualIntent =
                    VisualIntent(
                        cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
                        operationKind = uniffi.writer_core.EditorOperationKindDto.INSERT,
                        oldAffectedByteRanges = emptyList(),
                        newAffectedByteRanges = listOf(Pair(6, 9)),
                        animationMode = uniffi.writer_core.AnimationModeDto.GLYPH_ANIMATION,
                        durationMs = 160,
                        coordinatedCursor = CoordinatedCursor(6, 9, true),
                    ),
            )

        mirror.applyEditResult(result)
        assertEquals("你好世", mirror.getText())
        assertEquals(9, mirror.getCursorUtf8())
    }

    @Test
    fun updateComposition_setsUnderline() {
        val mirror = DisplayTextMirror()
        mirror.loadText("ab", 2)

        mirror.updateComposition(2, 2, "c")
        val range = mirror.getCompositionRangeUtf16()
        assertNotNull(range)
        assertEquals("abc", mirror.getText())
    }

    @Test
    fun clearComposition_removesComposition() {
        val mirror = DisplayTextMirror()
        mirror.loadText("ab", 2)

        mirror.updateComposition(2, 2, "c")
        mirror.clearComposition()
        assertNull(mirror.getCompositionRangeUtf16())
    }

    @Test
    fun applyEditResult_clearsCompositionBeforeApplying() {
        val mirror = DisplayTextMirror()
        mirror.loadText("ab", 2)

        mirror.updateComposition(2, 2, "c")

        val result =
            EditResult(
                outcome = uniffi.writer_core.EditorEditOutcomeDto.APPLIED,
                transactionId = 1,
                baseRevision = 0,
                newRevision = 1,
                displayPatches =
                    listOf(
                        DisplayPatch(
                            baseRevision = 0,
                            newRevision = 1,
                            replaceByteStart = 2,
                            replaceByteEndExclusive = 2,
                            insertedText = "d",
                            resultingSelectionStart = 3,
                            resultingSelectionEnd = 3,
                        ),
                    ),
                oldSelectionStart = 2,
                oldSelectionEnd = 2,
                newSelectionStart = 3,
                newSelectionEnd = 3,
                visualIntent =
                    VisualIntent(
                        cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
                        operationKind = uniffi.writer_core.EditorOperationKindDto.INSERT,
                        oldAffectedByteRanges = emptyList(),
                        newAffectedByteRanges = listOf(Pair(2, 3)),
                        animationMode = uniffi.writer_core.AnimationModeDto.GLYPH_ANIMATION,
                        durationMs = 160,
                        coordinatedCursor = CoordinatedCursor(2, 3, true),
                    ),
            )

        mirror.restoreCompositionBeforePatch()
        mirror.applyEditResult(result)
        assertNull(mirror.getCompositionRangeUtf16())
    }

    // ── #624 评论3：增量 UTF-8 字节长度（热路径不做整章 toByteArray） ──

    private fun assertLengthMatches(mirror: DisplayTextMirror) {
        assertEquals(
            mirror.getText().toByteArray(Charsets.UTF_8).size,
            mirror.getTextLengthUtf8(),
        )
    }

    @Test
    fun textLengthUtf8_tracksLoadAndPatches() {
        val mirror = DisplayTextMirror()
        mirror.loadText("你好", 0)
        assertLengthMatches(mirror)

        // 插入多字节字符
        applySinglePatch(
            mirror,
            PatchSpec(
                0,
                1,
                uniffi.writer_core.EditorOperationKindDto.INSERT,
                emptyList(),
                listOf(Pair(6, 15)),
                6,
                15,
            ),
            DisplayPatch(0, 1, 6, 6, "，世界", 15, 15),
        )
        assertEquals("你好，世界", mirror.getText())
        assertLengthMatches(mirror)

        // 删除覆盖多字节区间
        applySinglePatch(
            mirror,
            PatchSpec(
                1,
                2,
                uniffi.writer_core.EditorOperationKindDto.DELETE,
                listOf(Pair(3, 12)),
                emptyList(),
                15,
                3,
            ),
            DisplayPatch(1, 2, 3, 12, "", 3, 3),
        )
        assertEquals("你界", mirror.getText())
        assertLengthMatches(mirror)

        // 替换（含换行）
        applySinglePatch(
            mirror,
            PatchSpec(
                2,
                3,
                uniffi.writer_core.EditorOperationKindDto.REPLACE,
                listOf(Pair(3, 6)),
                listOf(Pair(3, 4)),
                6,
                4,
            ),
            DisplayPatch(2, 3, 3, 6, "\n", 4, 4),
        )
        assertEquals("你\n", mirror.getText())
        assertLengthMatches(mirror)
    }

    private data class PatchSpec(
        val baseRevision: Long,
        val newRevision: Long,
        val kind: uniffi.writer_core.EditorOperationKindDto,
        val oldAffected: List<Pair<Int, Int>>,
        val newAffected: List<Pair<Int, Int>>,
        val oldCursor: Int,
        val newCursor: Int,
    )

    private fun applySinglePatch(
        mirror: DisplayTextMirror,
        spec: PatchSpec,
        patch: DisplayPatch,
    ) {
        mirror.applyEditResult(
            EditResult(
                outcome = uniffi.writer_core.EditorEditOutcomeDto.APPLIED,
                transactionId = spec.newRevision,
                baseRevision = spec.baseRevision,
                newRevision = spec.newRevision,
                displayPatches = listOf(patch),
                oldSelectionStart = spec.oldCursor,
                oldSelectionEnd = spec.oldCursor,
                newSelectionStart = spec.newCursor,
                newSelectionEnd = spec.newCursor,
                visualIntent =
                    VisualIntent(
                        cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
                        operationKind = spec.kind,
                        oldAffectedByteRanges = spec.oldAffected,
                        newAffectedByteRanges = spec.newAffected,
                        animationMode = uniffi.writer_core.AnimationModeDto.GLYPH_ANIMATION,
                        durationMs = 160,
                        coordinatedCursor = CoordinatedCursor(spec.oldCursor, spec.newCursor, true),
                    ),
            ),
        )
    }

    @Test
    fun textLengthUtf8_includesCompositionOverlay() {
        val mirror = DisplayTextMirror()
        mirror.loadText("ab", 2)
        assertLengthMatches(mirror)

        // 覆盖层替换 1 个 ASCII 字符为 2 个 CJK 字符
        mirror.updateComposition(replaceStartUtf8 = 1, replaceEndUtf8 = 2, preeditText = "中文")
        assertLengthMatches(mirror)
        assertEquals("a中文", mirror.getText())

        // 覆盖层更新
        mirror.updateComposition(replaceStartUtf8 = 1, replaceEndUtf8 = 2, preeditText = "中")
        assertLengthMatches(mirror)
        assertEquals("a中", mirror.getText())

        // 提交覆盖层（先清除再走 patch）
        mirror.clearComposition()
        assertLengthMatches(mirror)
        assertEquals("ab", mirror.getText())

        // 空文档 + 覆盖层插入
        val empty = DisplayTextMirror()
        empty.loadText("", 0)
        empty.updateComposition(replaceStartUtf8 = 0, replaceEndUtf8 = 0, preeditText = "你")
        assertLengthMatches(empty)
        assertEquals("你", empty.getText())
    }
    // ── #624 评论6：getCommittedTextLengthUtf8 / committedSliceUtf8 直接单元测试 ──
    // 这两个 API 是评论6新增的核心读取路径，之前无直接覆盖。下面覆盖无 composition
    // （ASCII/多字节/emoji/全范围/空区间/越界 coerce）和有 composition（覆盖区前/内/后/
    // 横跨三段/全范围等于 getCommittedText/多字节覆盖层）两类状态。

    @Test
    fun getCommittedTextLengthUtf8_noComposition_ascii() {
        // 无 composition 时已提交长度 = 全文 UTF-8 字节长度（ASCII）。
        val mirror = DisplayTextMirror()
        mirror.loadText("Hello", 5)
        assertEquals(5, mirror.getCommittedTextLengthUtf8())
    }

    @Test
    fun getCommittedTextLengthUtf8_noComposition_multibyte() {
        // 无 composition 时已提交长度 = 全文 UTF-8 字节长度（每中文 3 字节）。
        // 同时验证 getTextLengthUtf8 在无覆盖层时与 committed 一致。
        val mirror = DisplayTextMirror()
        mirror.loadText("你好", 2)
        assertEquals(6, mirror.getCommittedTextLengthUtf8())
        assertEquals(6, mirror.getTextLengthUtf8())
    }

    @Test
    fun getCommittedTextLengthUtf8_withComposition_excludesOverlay() {
        // 有 composition 时 committed 长度不含 preedit 覆盖层增量。
        // loadText("abc") committed=3；updateComposition(1,2,"你好") 覆盖 b，
        // committed 仍是 "abc"（3 字节），而 getTextLengthUtf8 含覆盖层增量 = 3-1+6 = 8。
        val mirror = DisplayTextMirror()
        mirror.loadText("abc", 3)
        mirror.updateComposition(replaceStartUtf8 = 1, replaceEndUtf8 = 2, preeditText = "你好")
        assertEquals(3, mirror.getCommittedTextLengthUtf8())
        assertEquals(8, mirror.getTextLengthUtf8())
    }

    @Test
    fun committedSliceUtf8_noComposition_ascii() {
        // 无 composition ASCII 局部切片：字节 1..4 = "ell"。
        val mirror = DisplayTextMirror()
        mirror.loadText("Hello", 5)
        assertEquals("ell", mirror.committedSliceUtf8(1, 4))
    }

    @Test
    fun committedSliceUtf8_noComposition_multibyte() {
        // 无 composition 多字节切片：a=1,你=3,好=3,b=1 共 8 字节。
        // 字节 1..7 = "你好"；全范围 = "a你好b"。
        val mirror = DisplayTextMirror()
        mirror.loadText("a你好b", 4)
        assertEquals("你好", mirror.committedSliceUtf8(1, 7))
        assertEquals("a你好b", mirror.committedSliceUtf8(0, 8))
    }

    @Test
    fun committedSliceUtf8_noComposition_emoji() {
        // 无 composition emoji 切片：a=1,😀=4,b=1 共 6 字节。
        // 😀 是 supplementary char（UTF-16 surrogate pair 占 2 char），验证 UTF-8→UTF-16
        // 映射在 surrogate 边界正确。字节 1..5 = "😀"；全范围 = "a😀b"。
        val mirror = DisplayTextMirror()
        mirror.loadText("a😀b", 3)
        assertEquals("😀", mirror.committedSliceUtf8(1, 5))
        assertEquals("a😀b", mirror.committedSliceUtf8(0, 6))
    }

    @Test
    fun committedSliceUtf8_noComposition_fullRange_equalsGetCommittedText() {
        // 无 composition 时全范围切片应等于 getCommittedText()。
        val mirror = DisplayTextMirror()
        mirror.loadText("a你好b", 4)
        assertEquals(
            mirror.getCommittedText(),
            mirror.committedSliceUtf8(0, mirror.getCommittedTextLengthUtf8()),
        )
    }

    @Test
    fun committedSliceUtf8_noComposition_emptyRange() {
        // 空区间返回 ""：start==end 直接返回；reversed 区间经 coerce 后 safeStart>=safeEnd 也返回 ""。
        val mirror = DisplayTextMirror()
        mirror.loadText("abc", 3)
        assertEquals("", mirror.committedSliceUtf8(1, 1))
        assertEquals("", mirror.committedSliceUtf8(2, 1))
    }

    @Test
    fun committedSliceUtf8_noComposition_outOfBounds_coerced() {
        // 越界参数被 coerce 到 [0, committedLen]：(-5,100)→[0,3]="abc"；(2,100)→[2,3]="c"。
        val mirror = DisplayTextMirror()
        mirror.loadText("abc", 3)
        assertEquals("abc", mirror.committedSliceUtf8(-5, 100))
        assertEquals("c", mirror.committedSliceUtf8(2, 100))
    }

    // ── committedSliceUtf8 有 composition ──
    // 以下用 loadText("abcdef") + updateComposition(3,4,"XY") 作为基础 setup：
    // 覆盖区 [3,4) 原文本 "d" 被替换为 preedit "XY"，committed text 仍是 "abcdef"。
    // 验证三段拼接逻辑（覆盖区前 / compositionOriginalText / 覆盖区后）。

    @Test
    fun committedSliceUtf8_composition_beforeOverlay() {
        // 覆盖区前段：字节 0..3 = "abc"（纯 buffer 前段，不进入覆盖区）。
        val mirror = DisplayTextMirror()
        mirror.loadText("abcdef", 6)
        mirror.updateComposition(replaceStartUtf8 = 3, replaceEndUtf8 = 4, preeditText = "XY")
        assertEquals("abc", mirror.committedSliceUtf8(0, 3))
    }

    @Test
    fun committedSliceUtf8_composition_withinOverlay_returnsOriginalText() {
        // 覆盖区内段：committed 坐标 [3,4) 对应 compositionOriginalText="d"，
        // 不是 buffer 里的 preedit "XY"。
        val mirror = DisplayTextMirror()
        mirror.loadText("abcdef", 6)
        mirror.updateComposition(replaceStartUtf8 = 3, replaceEndUtf8 = 4, preeditText = "XY")
        assertEquals("d", mirror.committedSliceUtf8(3, 4))
    }

    @Test
    fun committedSliceUtf8_composition_afterOverlay() {
        // 覆盖区后段：字节 4..6 = "ef"（映射回 buffer 后段）。
        val mirror = DisplayTextMirror()
        mirror.loadText("abcdef", 6)
        mirror.updateComposition(replaceStartUtf8 = 3, replaceEndUtf8 = 4, preeditText = "XY")
        assertEquals("ef", mirror.committedSliceUtf8(4, 6))
    }

    @Test
    fun committedSliceUtf8_composition_spanningAllThreeSegments() {
        // 横跨三段：字节 0..6 = "abc" + "d" + "ef" = "abcdef"。
        val mirror = DisplayTextMirror()
        mirror.loadText("abcdef", 6)
        mirror.updateComposition(replaceStartUtf8 = 3, replaceEndUtf8 = 4, preeditText = "XY")
        assertEquals("abcdef", mirror.committedSliceUtf8(0, 6))
    }

    @Test
    fun committedSliceUtf8_composition_spanningBeforeAndOverlay() {
        // 横跨前段+覆盖区：字节 1..4 = "bc" + "d" = "bcd"。
        val mirror = DisplayTextMirror()
        mirror.loadText("abcdef", 6)
        mirror.updateComposition(replaceStartUtf8 = 3, replaceEndUtf8 = 4, preeditText = "XY")
        assertEquals("bcd", mirror.committedSliceUtf8(1, 4))
    }

    @Test
    fun committedSliceUtf8_composition_spanningOverlayAndAfter() {
        // 横跨覆盖区+后段：字节 3..6 = "d" + "ef" = "def"。
        val mirror = DisplayTextMirror()
        mirror.loadText("abcdef", 6)
        mirror.updateComposition(replaceStartUtf8 = 3, replaceEndUtf8 = 4, preeditText = "XY")
        assertEquals("def", mirror.committedSliceUtf8(3, 6))
    }

    @Test
    fun committedSliceUtf8_composition_fullRange_equalsGetCommittedText() {
        // 有 composition 时全范围切片应等于 getCommittedText()（多字节覆盖层场景）。
        // loadText("a你好c") committed=8 字节；updateComposition(1,4,"XX") 覆盖 "你"（UTF-8 1..4）。
        // committed text = "a你好c"。
        val mirror = DisplayTextMirror()
        mirror.loadText("a你好c", 3)
        mirror.updateComposition(replaceStartUtf8 = 1, replaceEndUtf8 = 4, preeditText = "XX")
        assertEquals(
            mirror.getCommittedText(),
            mirror.committedSliceUtf8(0, mirror.getCommittedTextLengthUtf8()),
        )
    }

    @Test
    fun committedSliceUtf8_composition_multibyteOverlay() {
        // 多字节覆盖层：loadText("a你好b") committed=8 字节；
        // updateComposition(1,7,"X") 覆盖 "你好"（UTF-8 1..7）。committed text = "a你好b"。
        // 切片 [1,7) = "你好"（从 compositionOriginalText 按字节切片）；
        // 全范围 [0,8) = "a你好b"（三段拼接）。
        val mirror = DisplayTextMirror()
        mirror.loadText("a你好b", 4)
        mirror.updateComposition(replaceStartUtf8 = 1, replaceEndUtf8 = 7, preeditText = "X")
        assertEquals("你好", mirror.committedSliceUtf8(1, 7))
        assertEquals("a你好b", mirror.committedSliceUtf8(0, 8))
    }
}
