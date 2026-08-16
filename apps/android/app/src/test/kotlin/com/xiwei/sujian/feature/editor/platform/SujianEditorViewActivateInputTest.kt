package com.xiwei.sujian.feature.editor.platform

import android.view.inputmethod.EditorInfo
import androidx.test.core.app.ApplicationProvider
import com.xiwei.sujian.feature.editor.interop.EditorKernelBridge
import com.xiwei.sujian.feature.editor.session.TextEditorProfile
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.writer_core.EditorEditResultDto
import uniffi.writer_core.EditorTransactionCauseDto

/**
 * #630 评论 5306659312 问题 C：activateInput 的 inputRestartPending 状态机契约 —
 *
 * - 普通重复 tap（不换 session）：inputRestartPending=false，activateInput 不 restartInput；
 * - session 换绑（rebind）且旧连接持有焦点：inputRestartPending=true，
 *   activateInput 在已聚焦时 restartInput 一次后清 pending，再次调用不 restart；
 * - 首次绑定不设 pending（没有旧连接需要 restart）；
 * - onCreateInputConnection 成功创建连接后清 pending（系统已拿到新连接）。
 *
 * 用反射读取 private inputRestartPending 字段验证状态机；restartInput 的实际调用由
 * pending 状态变化间接验证（pending 只在 activateInput 的 restart 分支被清 false）。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SujianEditorViewActivateInputTest {
    @Test
    fun firstBind_doesNotSetInputRestartPending() {
        val view = SujianEditorView(ApplicationProvider.getApplicationContext())
        view.attachSession(
            sessionBridge = RecordingBridge(),
            profile = TextEditorProfile.DocumentBody,
            text = "正文",
            revision = 1L,
            cursorUtf8 = 0,
            selStartUtf8 = 0,
            selEndUtf8 = 0,
        )
        assertFalse(
            "首次绑定不得设 inputRestartPending（没有旧连接需要 restart）",
            view.readInputRestartPending(),
        )
    }

    @Test
    fun rebind_whenFocused_setsInputRestartPending() {
        val view = SujianEditorView(ApplicationProvider.getApplicationContext())
        val bridge = RecordingBridge()
        view.attachSession(bridge, TextEditorProfile.DocumentBody, "a", 1L, 0, 0, 0)
        view.requestFocus()
        assertTrue("前置：view 应已聚焦", view.hasFocus())

        // rebind：bindSessionInternal 检测 isSessionBound==true，捕获 hadFocus=true，
        // unbindSession clearFocus，然后设 inputRestartPending=true。
        view.attachSession(bridge, TextEditorProfile.DocumentBody, "b", 2L, 0, 0, 0)

        assertTrue(
            "rebind 且旧连接持有焦点时应设 inputRestartPending",
            view.readInputRestartPending(),
        )
    }

    @Test
    fun rebind_whenNotFocused_doesNotSetInputRestartPending() {
        val view = SujianEditorView(ApplicationProvider.getApplicationContext())
        val bridge = RecordingBridge()
        view.attachSession(bridge, TextEditorProfile.DocumentBody, "a", 1L, 0, 0, 0)
        assertFalse("前置：未 requestFocus", view.hasFocus())

        view.attachSession(bridge, TextEditorProfile.DocumentBody, "b", 2L, 0, 0, 0)

        assertFalse(
            "rebind 但旧连接未持有焦点时不得设 inputRestartPending",
            view.readInputRestartPending(),
        )
    }

    @Test
    fun activateInput_whenFocusedAndPendingFalse_doesNotRestart() {
        val view = SujianEditorView(ApplicationProvider.getApplicationContext())
        view.attachSession(RecordingBridge(), TextEditorProfile.DocumentBody, "a", 1L, 0, 0, 0)
        view.requestFocus()
        view.setInputRestartPending(false)

        // 普通重复 tap：已聚焦 + pending=false → 不走 restart 分支
        view.activateInput()

        assertFalse(
            "已聚焦 + pending=false 时 activateInput 不得改 pending（未 restart）",
            view.readInputRestartPending(),
        )
    }

    @Test
    fun activateInput_whenFocusedAndPendingTrue_restartsOnceAndClearsPending() {
        val view = SujianEditorView(ApplicationProvider.getApplicationContext())
        view.attachSession(RecordingBridge(), TextEditorProfile.DocumentBody, "a", 1L, 0, 0, 0)
        view.requestFocus()
        view.setInputRestartPending(true)

        // 第一次 activateInput：已聚焦 + pending=true → restartInput 一次，清 pending
        view.activateInput()
        assertFalse(
            "已聚焦 + pending=true 时 activateInput 应清 pending（restart 了一次）",
            view.readInputRestartPending(),
        )

        // 第二次 activateInput：已聚焦 + pending=false → 不 restart
        view.activateInput()
        assertFalse(
            "pending 清零后再次 activateInput 不得 restart",
            view.readInputRestartPending(),
        )
    }

    @Test
    fun onCreateInputConnection_clearsPendingOnSuccess() {
        val view = SujianEditorView(ApplicationProvider.getApplicationContext())
        view.attachSession(RecordingBridge(), TextEditorProfile.DocumentBody, "a", 1L, 0, 0, 0)
        view.setInputRestartPending(true)

        val ic = view.onCreateInputConnection(EditorInfo())
        assertTrue("绑定后应能创建 InputConnection", ic != null)
        assertFalse(
            "onCreateInputConnection 成功创建连接后应清 pending（系统已拿新连接）",
            view.readInputRestartPending(),
        )
    }

    @Test
    fun activateInput_whenSessionUnbound_doesNothing() {
        val view = SujianEditorView(ApplicationProvider.getApplicationContext())
        view.setInputRestartPending(true)
        // 未绑定 → activateInput 直接返回，不动 pending
        view.activateInput()
        assertTrue(
            "未绑定时 activateInput 不得动 pending（直接返回）",
            view.readInputRestartPending(),
        )
    }

    // ── 反射读写 private inputRestartPending ──

    private fun SujianEditorView.readInputRestartPending(): Boolean {
        val field = SujianEditorView::class.java.getDeclaredField("inputRestartPending")
        field.isAccessible = true
        return field.getBoolean(this)
    }

    private fun SujianEditorView.setInputRestartPending(value: Boolean) {
        val field = SujianEditorView::class.java.getDeclaredField("inputRestartPending")
        field.isAccessible = true
        field.setBoolean(this, value)
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
