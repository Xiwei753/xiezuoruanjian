package com.xiwei.sujian.feature.editor.platform

import android.view.inputmethod.EditorInfo
import androidx.test.core.app.ApplicationProvider
import com.xiwei.sujian.feature.editor.interop.EditorKernelBridge
import com.xiwei.sujian.feature.editor.session.TextEditorProfile
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.writer_core.EditorEditResultDto
import uniffi.writer_core.EditorTransactionCauseDto

/**
 * #624 评论5：InputConnection 与真实编辑 session 的绑定门控契约 —
 *
 * - 会话未绑定时 onCreateInputConnection 直接返回 null（不再出现
 *   created=true sessionBound=false 的脱节连接）；
 * - attachSession 绑定并取得焦点后系统重新查询，拿到的连接属于当前真实 session。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SujianEditorViewSessionGateTest {
    @Test
    fun onCreateInputConnection_returnsNullWhenSessionUnbound() {
        val view = SujianEditorView(ApplicationProvider.getApplicationContext())

        assertFalse(view.isSessionBound)
        assertNull("会话未绑定不得创建 InputConnection", view.onCreateInputConnection(EditorInfo()))
        assertFalse("未绑定时不提供输入", view.onCheckIsTextEditor())
    }

    @Test
    fun onCreateInputConnection_returnsConnectionAfterSessionBound() {
        val view = SujianEditorView(ApplicationProvider.getApplicationContext())
        val ok =
            view.attachSession(
                sessionBridge = RecordingBridge(),
                profile = TextEditorProfile.DocumentBody,
                text = "正文",
                revision = 3L,
                cursorUtf8 = 6,
                selStartUtf8 = 6,
                selEndUtf8 = 6,
            )

        assertTrue("attachSession 必须成功", ok)
        assertTrue(view.isSessionBound)
        assertNotNull("绑定后必须能创建 InputConnection", view.onCreateInputConnection(EditorInfo()))
        assertTrue("绑定后必须提供输入", view.onCheckIsTextEditor())
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
}
