package com.xiwei.sujian.feature.editor.pipeline

import android.text.TextPaint
import com.xiwei.sujian.feature.editor.interop.EditorKernelBridge
import com.xiwei.sujian.feature.editor.projection.DisplayTextMirror
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.writer_core.AnimationModeDto
import uniffi.writer_core.CoordinatedCursorDto
import uniffi.writer_core.DisplayPatchDto
import uniffi.writer_core.EditorByteRangeDto
import uniffi.writer_core.EditorEditOutcomeDto
import uniffi.writer_core.EditorEditResultDto
import uniffi.writer_core.EditorOperationKindDto
import uniffi.writer_core.EditorTransactionCauseDto
import uniffi.writer_core.EditorVisualIntentDto

/**
 * #592 一：attachSnapshot 契约测试。
 *
 * 验证窗口重建/重新绑定时只把 textEditSessionSnapshot 装入 Android mirror/layout：
 * - 不调用 kernel loadText（Rust revision 不变、Undo/Redo 不清空、composition 不重置）
 * - text/revision/cursor/selection 全部从 snapshot 恢复
 * - 镜像后续编辑命令继续使用恢复的 revision
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class AttachSnapshotTest {
    private class RecordingBridge : EditorKernelBridge {
        var loadTextCalls = 0
        var insertCalls = 0
        var insertResult: EditorEditResultDto? = null

        override fun insert(
            byteOffset: Int,
            text: String,
            cause: EditorTransactionCauseDto,
            expectedRevision: Long,
        ): EditorEditResultDto? {
            insertCalls++
            return insertResult
        }

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
        ): EditorEditResultDto? {
            loadTextCalls++
            return null
        }

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

        override fun setAnimationEnabled(enabled: Boolean) { }

        override fun setAnimationDurationMs(durationMs: Long) { }

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

        // #606: grapheme 边界 stub — 测试不验证 grapheme 语义
        override fun previousGraphemeBoundary(byteOffset: Int): Int = byteOffset

        override fun nextGraphemeBoundary(byteOffset: Int): Int = byteOffset

        // #606: 测试不覆盖 rebase 映射 — 返回 null（平台端按无映射处理）。
        override fun computeRebaseSliceMappings(
            oldSliceRoles: List<uniffi.writer_core.AnimatedSliceRoleDto>,
            oldSliceByteRanges: List<uniffi.writer_core.EditorByteRangeDto>,
            newSliceRoles: List<uniffi.writer_core.AnimatedSliceRoleDto>,
            newSliceByteRanges: List<uniffi.writer_core.EditorByteRangeDto>,
            offsetMap: uniffi.writer_core.OffsetMapDto?,
        ): List<uniffi.writer_core.RebaseSliceMappingDto>? = null
    }

    private fun newPipeline(): Pair<AndroidEditorPipeline, RecordingBridge> {
        val mirror = DisplayTextMirror()
        val pipeline = AndroidEditorPipeline.create(mirror, TextPaint())
        val bridge = RecordingBridge()
        pipeline.kernelBridge = bridge
        return pipeline to bridge
    }

    @Test
    fun attachSnapshot_doesNotCallKernelLoadText() {
        val (pipeline, bridge) = newPipeline()
        pipeline.attachSnapshot(
            text = "你好 world",
            revision = 42L,
            cursorUtf8 = 9,
            selStartUtf8 = 3,
            selEndUtf8 = 9,
        )
        assertEquals("attachSnapshot must not call textEditSessionLoadText", 0, bridge.loadTextCalls)
    }

    @Test
    fun attachSnapshot_restoresTextRevisionCursorSelection() {
        val (pipeline, _) = newPipeline()
        pipeline.attachSnapshot(
            text = "你好 world",
            revision = 42L,
            cursorUtf8 = 9,
            selStartUtf8 = 3,
            selEndUtf8 = 9,
        )
        assertEquals("你好 world", pipeline.getText())
        assertEquals(42L, pipeline.getRevision())
        assertEquals(9, pipeline.getCursorUtf8())
        assertEquals(3, pipeline.getSelectionStartUtf8())
        assertEquals(9, pipeline.getSelectionEndUtf8())
    }

    @Test
    fun loadText_stillCallsKernelForExternalReset() {
        val (pipeline, bridge) = newPipeline()
        pipeline.loadText("new content", 11)
        assertEquals("loadText（明确外部内容重置）仍走 kernel", 1, bridge.loadTextCalls)
    }

    @Test
    fun attachSnapshot_preservesKernelBridgeForSubsequentEdits() {
        val (pipeline, bridge) = newPipeline()
        pipeline.attachSnapshot(text = "abc", revision = 7L, cursorUtf8 = 3, selStartUtf8 = 3, selEndUtf8 = 3)
        assertTrue(
            "attach 后 kernelBridge 必须保留，后续编辑可继续走 Rust",
            pipeline.kernelBridge != null,
        )
        // 后续编辑命令仍使用同一 bridge（revision 不被 loadText 重置）
        pipeline.insertText(3, "d", EditorTransactionCauseDto.TYPING)
        assertTrue("后续编辑必须到达 bridge", bridge.insertCalls == 1)
    }

    /**
     * #624 评论7：普通编辑（普通正文，无 secret projection）走 mirror 增量修改 +
     * identity 投影刷新，不重建 DynamicLayout — 布局实例跨编辑保持同一个，
     * 只有 revision 前进。旧代码在每次编辑时 rebuildDisplayProjection 清 fingerprint，
     * 导致每字符都 new DynamicLayout（卡顿来源之一）。
     */
    @Test
    fun plainEdit_reusesSameDynamicLayout() {
        val (pipeline, bridge) = newPipeline()
        pipeline.attachSnapshot(text = "ab\ncd", revision = 0L, cursorUtf8 = 5, selStartUtf8 = 5, selEndUtf8 = 5)
        pipeline.updateLayout(500f)
        val layoutBefore = pipeline.getLayout()
        assertNotNull("首次布局必须已建立", layoutBefore)
        bridge.insertResult = appliedInsertDto()

        val output = pipeline.insertText(1, "X", EditorTransactionCauseDto.TYPING)

        assertTrue("普通编辑必须成功应用", output is PipelineOutput.Edited)
        assertSame("普通编辑必须复用同一个 DynamicLayout", layoutBefore, pipeline.getLayout())
        assertEquals("mirror 反映新正文", "aXb\ncd", pipeline.getText())
        assertEquals("kernel revision 前进", 1L, pipeline.getRevision())
    }

    private fun appliedInsertDto(): EditorEditResultDto =
        EditorEditResultDto(
            outcome = EditorEditOutcomeDto.APPLIED,
            transactionId = 1uL,
            baseRevision = 0uL,
            newRevision = 1uL,
            displayPatches =
                listOf(
                    DisplayPatchDto(
                        baseRevision = 0uL,
                        newRevision = 1uL,
                        replaceByteStart = 1u,
                        replaceByteEndExclusive = 1u,
                        insertedText = "X",
                        resultingSelectionStart = 2u,
                        resultingSelectionEnd = 2u,
                    ),
                ),
            oldSelectionStart = 1u,
            oldSelectionEnd = 1u,
            newSelectionStart = 2u,
            newSelectionEnd = 2u,
            visualIntent =
                EditorVisualIntentDto(
                    cause = EditorTransactionCauseDto.TYPING,
                    operationKind = EditorOperationKindDto.INSERT,
                    oldAffectedByteRanges = emptyList(),
                    newAffectedByteRanges = listOf(EditorByteRangeDto(1u, 2u)),
                    animationMode = AnimationModeDto.SYSTEM_SUPPRESSED,
                    durationMs = 0uL,
                    coordinatedCursor = CoordinatedCursorDto(1u, 2u, false),
                    offsetMap = null,
                ),
            compositionSession = null,
            contentDelta = uniffi.writer_core.EditorContentDeltaDto(0u, 0u, 0u, 0u),
        )
}
