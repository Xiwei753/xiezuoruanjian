package com.xiwei.sujian.editor.v2.pipeline

import android.text.TextPaint
import com.xiwei.sujian.editor.v2.host.EditorKernelBridge
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.writer_core.EditorEditResultDto
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

        override fun insert(
            byteOffset: Int,
            text: String,
            cause: EditorTransactionCauseDto,
            expectedRevision: Long,
        ): EditorEditResultDto? {
            insertCalls++
            return null
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

        override fun compositionUpdateVisualIntent(
            compositionReplaceStart: UInt,
            compositionReplaceEndExclusive: UInt,
            oldPreeditText: String,
            newPreeditText: String,
        ): EditorVisualIntentDto? = null

        override fun setAnimationEnabled(enabled: Boolean) { }

        override fun setAnimationDurationMs(durationMs: Long) { }

        override fun replaceAll(
            search: String,
            replacement: String,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun insertLineBreak(
            byteOffset: Int,
            autoIndentPrefix: String,
            cause: EditorTransactionCauseDto,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun sessionSnapshot(): uniffi.writer_core.EditorSessionSnapshotDto? = null
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
}
