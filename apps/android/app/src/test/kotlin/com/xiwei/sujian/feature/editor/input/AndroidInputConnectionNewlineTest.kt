package com.xiwei.sujian.feature.editor.input

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #624 评论2：换行命令收敛契约测试 — 软键盘 commitText 在非 composition 状态
 * 收到 `\n` / `\r\n` 时统一走 [InputCommandPort.insertLineBreak]：
 *
 * - `\r\n` 规范化为一个逻辑换行；
 * - 连续按 Enter 得到连续 `\n`，第二个空行不会被吞掉；
 * - 有选区时按“换行替换”语义一次替换（不先删选区再插入）；
 * - 不经过普通 sendCommitTextToKernel 提交路径。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34], shadows = [RecordingInputMethodManagerShadow::class])
class AndroidInputConnectionNewlineTest {
    private companion object {
        const val ABXY = "ABXY"
        const val KERNEL_MATCHES_MIRROR = "内核文本必须与 mirror 一致"
        const val NEWLINE = "\n"
        const val ABXY_NEWLINE = "ABXY\n"
        const val ABXY_TWO_NEWLINES = "ABXY\n\n"
        const val ABXY_NEWLINE_Y = "A\nY"
    }

    @Test
    fun commitTextNewline_noComposition_goesThroughInsertLineBreak() {
        val h = InputConnectionTestHarness(ABXY, 4)

        h.connection.commitText(NEWLINE, 1)

        assertEquals("换行必须插入到光标处", ABXY_NEWLINE, h.mirror.getCommittedText())
        assertEquals(KERNEL_MATCHES_MIRROR, ABXY_NEWLINE, h.commandPort.getKernelText())
        assertEquals("换行不得走普通 commitComposition 路径", 0, h.commandPort.commitCalls.size)
        assertFalse("换行提交不得进入 composing 状态", h.adapter.isComposing())
    }

    @Test
    fun commitTextCrlf_normalizedToSingleNewline() {
        val h = InputConnectionTestHarness(ABXY, 4)

        h.connection.commitText("\r\n", 1)

        assertEquals("CRLF 必须规范化为一个逻辑换行", ABXY_NEWLINE, h.mirror.getCommittedText())
        assertEquals(KERNEL_MATCHES_MIRROR, ABXY_NEWLINE, h.commandPort.getKernelText())
    }

    @Test
    fun consecutiveEnterPresses_produceConsecutiveNewlines() {
        val h = InputConnectionTestHarness(ABXY, 4)

        h.connection.commitText(NEWLINE, 1)
        h.connection.commitText(NEWLINE, 1)

        assertEquals("连续 Enter 必须产生连续空行", ABXY_TWO_NEWLINES, h.mirror.getCommittedText())
        assertEquals(KERNEL_MATCHES_MIRROR, ABXY_TWO_NEWLINES, h.commandPort.getKernelText())
    }

    @Test
    fun commitTextNewline_outputRoutedToHost() {
        val h = InputConnectionTestHarness(ABXY, 4)
        val outputs = mutableListOf<com.xiwei.sujian.feature.editor.pipeline.PipelineOutput>()
        h.adapter.onPipelineOutput = { outputs.add(it) }

        h.connection.commitText(NEWLINE, 1)

        // 输出必须与其他 send* 路径一致经 onPipelineOutput 回到宿主：宿主据此更新
        // 滚动上限、触发 onLocalEdit/onContentChanged、invalidate 与动画帧请求。
        // 若丢弃输出，正文进了 mirror 但屏幕不重绘、会话层与 ViewModel 内容不更新。
        assertEquals("换行输出必须路由到宿主", 1, outputs.size)
        val edited = outputs.single() as? com.xiwei.sujian.feature.editor.pipeline.PipelineOutput.Edited
        assertNotNull("输出必须是 Edited", edited)
        assertTrue("输出结果必须已应用", edited!!.result.isApplied())
        assertEquals("输出 patch 必须插入换行", NEWLINE, edited.result.displayPatches.single().insertedText)
        assertEquals(KERNEL_MATCHES_MIRROR, ABXY_NEWLINE, h.mirror.getCommittedText())
    }

    @Test
    fun commitTextNewline_withSelection_replacesSelectionWithSingleNewline() {
        val h = InputConnectionTestHarness("ABXY", 2)
        h.connection.setSelection(1, 3)

        h.connection.commitText(NEWLINE, 1)

        assertEquals("选区必须被单个换行替换", ABXY_NEWLINE_Y, h.mirror.getCommittedText())
        assertEquals(KERNEL_MATCHES_MIRROR, ABXY_NEWLINE_Y, h.commandPort.getKernelText())
        assertFalse("换行替换不得进入 composing 状态", h.adapter.isComposing())
    }
}
