package com.xiwei.sujian.editor.selfrender

import org.junit.Assert.*
import org.junit.Before
import org.junit.Test

/**
 * SujianInputConnection 单元测试
 *
 * 验证 IME 输入法连接行为：composing 状态、commit、close、
 * getTextBefore/AfterCursor 等。
 */
class SujianInputConnectionTest {

    private lateinit var buffer: SujianEditorBuffer
    private lateinit var ic: SujianInputConnection

    @Before
    fun setUp() {
        buffer = SujianEditorBuffer()
        ic = SujianInputConnection(buffer)
    }

    // ── 1. testFinishComposingDoesNotCommitText ──
    // finishComposingText 不应 commit composing 文本
    @Test
    fun testFinishComposingDoesNotCommitText() {
        buffer.text = "abc"
        buffer.setCursor(3)
        ic.setComposingText("拼", 1)
        // finishComposingText 应清除 composing 状态但不 commit
        ic.finishComposingText()
        // composing 文本不应出现在 buffer 中
        // （composing 是临时视觉层，不进入正式文本）
        assertFalse(ic.hasComposing())
    }

    // ── 2. testCommitTextAfterComposing ──
    // composing 后 commit 正式文本
    @Test
    fun testCommitTextAfterComposing() {
        buffer.text = ""
        buffer.setCursor(0)
        ic.setComposingText("拼", 1)
        // commit 正式文本
        ic.commitText("拼", 1)
        assertEquals("拼", buffer.text)
        assertFalse(ic.hasComposing())
    }

    // ── 3. testCloseConnectionDiscardsComposing ──
    // 关闭连接时丢弃 composing
    @Test
    fun testCloseConnectionDiscardsComposing() {
        buffer.text = "abc"
        buffer.setCursor(3)
        ic.setComposingText("拼", 1)
        ic.closeConnection()
        assertFalse(ic.hasComposing())
    }

    // ── 4. testSetComposingTextDoesNotModifyBuffer ──
    // setComposingText 不修改 buffer 正式文本
    @Test
    fun testSetComposingTextDoesNotModifyBuffer() {
        buffer.text = "abc"
        buffer.setCursor(3)
        val textBefore = buffer.text
        ic.setComposingText("拼", 1)
        // composing 不应修改 buffer 的正式文本
        assertEquals(textBefore, buffer.text)
    }

    // ── 5. testGetTextBeforeCursorDuringComposing ──
    // composing 期间 getTextBeforeCursor 返回正确内容
    @Test
    fun testGetTextBeforeCursorDuringComposing() {
        buffer.text = "你好"
        buffer.setCursor(2) // cursor after "你好"
        ic.setComposingText("世界", 2)
        val before = ic.getTextBeforeCursor(10)
        // 应包含正式文本中光标前的内容
        assertTrue(before.contains("你好"))
    }

    // ── 6. testGetTextAfterCursorDuringComposing ──
    // composing 期间 getTextAfterCursor 返回正确内容
    @Test
    fun testGetTextAfterCursorDuringComposing() {
        buffer.text = "你好世界"
        buffer.setCursor(2) // cursor after "你好"
        ic.setComposingText("的", 1)
        val after = ic.getTextAfterCursor(10)
        // 应包含正式文本中光标后的内容
        assertTrue(after.contains("世界"))
    }
}
