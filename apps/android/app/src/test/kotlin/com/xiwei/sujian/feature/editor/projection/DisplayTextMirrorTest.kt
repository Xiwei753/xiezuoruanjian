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

        val patches =
            listOf(
                DisplayPatch(0, 1, 2, 2, "c", 3, 3),
                DisplayPatch(0, 1, 2, 2, "d", 3, 3),
            )

        mirror.applyPatches(patches)
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
}
