package com.xiwei.sujian.feature.editor.platform

import com.xiwei.sujian.feature.editor.interop.EditorKernelBridge
import com.xiwei.sujian.feature.editor.session.EditorAppliedEvent
import com.xiwei.sujian.feature.editor.session.TextEditorProfile
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import uniffi.writer_core.AnimationModeDto
import uniffi.writer_core.CoordinatedCursorDto
import uniffi.writer_core.EditorEditOutcomeDto
import uniffi.writer_core.EditorEditResultDto
import uniffi.writer_core.EditorOperationKindDto
import uniffi.writer_core.EditorSessionSnapshotDto
import uniffi.writer_core.EditorTransactionCauseDto
import uniffi.writer_core.EditorVisualIntentDto

/**
 * #624 评论11 第1项：`reloadFromKernel()` 不是一次正文编辑 — 只是 Android mirror
 * 与 Rust session 重新对齐。事件必须 contentChanged=false、空 contentDelta、
 * cause=LOAD；不得伪造整章插入（否则整章再次计入 wordCount、记一笔 programmatic
 * 统计、置 Unsaved、触发 autosave）。
 *
 * 触发链：编辑结果空 displayPatches + revision 前进 → pipeline 判定 NeedReload
 * → view.reloadFromKernel() → kernelBridge.sessionSnapshot() 整章重装。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SujianEditorViewReloadEventTest {
    /** 记录 undo 调用并返回「空 displayPatches + revision 前进」的假 bridge。 */
    @Suppress("TooManyFunctions")
    private class NeedReloadBridge(
        private val snapshot: EditorSessionSnapshotDto,
    ) : EditorKernelBridge {
        var undoCalls = 0

        private fun revisionAdvancingEmptyPatchResult(): EditorEditResultDto =
            EditorEditResultDto(
                outcome = EditorEditOutcomeDto.APPLIED,
                transactionId = 2UL,
                baseRevision = 1UL,
                newRevision = 2UL,
                displayPatches = emptyList(),
                oldSelectionStart = 0u,
                oldSelectionEnd = 0u,
                newSelectionStart = 0u,
                newSelectionEnd = 0u,
                visualIntent =
                    EditorVisualIntentDto(
                        cause = EditorTransactionCauseDto.UNDO,
                        operationKind = EditorOperationKindDto.REPLACE,
                        oldAffectedByteRanges = emptyList(),
                        newAffectedByteRanges = emptyList(),
                        animationMode = AnimationModeDto.SYSTEM_SUPPRESSED,
                        durationMs = 0uL,
                        coordinatedCursor =
                            CoordinatedCursorDto(
                                oldByteOffset = 0u,
                                newByteOffset = 0u,
                                shouldAnimate = false,
                            ),
                        offsetMap = null,
                    ),
                compositionSession = null,
                contentDelta = uniffi.writer_core.EditorContentDeltaDto(0u, 0u, 0u, 0u),
                composition = null,
            )

        override fun undo(expectedRevision: Long): EditorEditResultDto? {
            undoCalls++
            return revisionAdvancingEmptyPatchResult()
        }

        override fun sessionSnapshot(): EditorSessionSnapshotDto = snapshot

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

        override fun setAnimationEnabled(enabled: Boolean) {}

        override fun setAnimationDurationMs(durationMs: Long) {}

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

    /**
     * reload 事件必须不携带伪造 delta：
     * - contentChanged=false（不是一次正文编辑）；
     * - cause=LOAD（非人工输入）；
     * - contentDelta 全零（不能把整章再次加进 wordCount/统计）；
     * - revision 为 snapshot 的 revision；mirror 与 snapshot 正文一致。
     */
    @Test
    fun reloadFromKernel_emitsLoadEventWithoutFakeDelta() {
        val context = RuntimeEnvironment.getApplication()
        val view = SujianEditorView(context)
        val bridge =
            NeedReloadBridge(
                EditorSessionSnapshotDto(
                    text = "reloaded body",
                    revision = 2UL,
                    cursor = 6u,
                    selectionAnchor = 6u,
                    generation = 0UL,
                    chapterId = "a",
                    composition = null,
                ),
            )
        val attached =
            view.attachSession(
                sessionBridge = bridge,
                profile = TextEditorProfile.DocumentBody,
                text = "hello",
                revision = 1L,
                cursorUtf8 = 5,
                selStartUtf8 = 5,
                selEndUtf8 = 5,
            )
        assertTrue("attachSession must succeed", attached)

        val events = mutableListOf<EditorAppliedEvent>()
        view.onLocalEdit = { events.add(it) }

        // 空 displayPatches + revision 前进 → pipeline 判定 NeedReload → reloadFromKernel。
        view.performUndo()

        assertEquals("undo 必须到达 kernel bridge", 1, bridge.undoCalls)
        assertEquals("reload 后必须发一次会话层事件", 1, events.size)
        val event = events[0]
        assertEquals("reload 是 mirror 重新对齐 — revision 取真实 snapshot", 2L, event.revision)
        assertEquals(
            "#624 评论11 第1项：reload 不是正文编辑 → contentChanged=false",
            false,
            event.contentChanged,
        )
        assertEquals(
            "#624 评论11 第1项：reload 必须带 cause=LOAD（非人工输入）",
            EditorTransactionCauseDto.LOAD,
            event.cause,
        )
        assertEquals("reload 不得伪造插入 delta", 0, event.contentDelta.insertedChars)
        assertEquals(
            "reload 不得伪造非空白插入 delta",
            0,
            event.contentDelta.insertedNonWhitespaceChars,
        )
        assertEquals("reload 不得伪造删除 delta", 0, event.contentDelta.deletedChars)
        assertEquals(
            "mirror 必须与 Rust snapshot 正文对齐",
            "reloaded body",
            view.getText(),
        )
    }
}
