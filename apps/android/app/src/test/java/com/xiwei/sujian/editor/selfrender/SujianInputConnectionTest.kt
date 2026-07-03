package com.xiwei.sujian.editor.selfrender

import org.junit.Assert.*
import org.junit.Before
import org.junit.Test

/**
 * SujianEditorBuffer composing 行为单元测试
 *
 * 验证 IME composing 相关行为：
 * - finishComposing 只清 composing，不提交正文
 * - commitText 才进入正文
 * - setComposingText 不修改正文 buffer
 * - composing 期间 getTextBefore/AfterCursor 返回正确内容
 * - closeConnection 语义：丢弃未提交 composing
 *
 * 注意：SujianInputConnection 依赖 View/SujianImeController，
 * 无法在纯单元测试中构造，因此通过直接测试 buffer 来验证核心语义。
 * IC 层的集成测试需要 Android instrumentation。
 */
class SujianInputConnectionTest {

    private lateinit var buffer: SujianEditorBuffer

    @Before
    fun setUp() {
        buffer = SujianEditorBuffer()
    }

    // ── 1. testFinishComposingDoesNotCommitText ──
    // finishComposing 只清 composing 状态，不提交 composing 文本到正文
    @Test
    fun testFinishComposingDoesNotCommitText() {
        buffer.loadText("abc")
        buffer.setSelection(3, 3)
        // 设置 composing（不修改正文）
        buffer.setComposingText("拼", 1)
        // finishComposing 只清 composing，不 commit
        buffer.finishComposing()
        // 正文不变
        assertEquals("abc", buffer.text)
        // composing 已清除
        assertFalse(buffer.hasComposing)
    }

    // ── 2. testCommitTextAfterComposing ──
    // composing 后 commitText 正确进入正文
    @Test
    fun testCommitTextAfterComposing() {
        buffer.loadText("")
        buffer.setComposingText("拼", 1)
        // commitText 才进入正文
        val result = buffer.commitText("拼")
        assertEquals("拼", buffer.text)
        assertFalse(buffer.hasComposing)
        assertEquals(1, result.selection.head)
    }

    // ── 3. testCloseConnectionDiscardsComposing ──
    // 关闭连接时丢弃未提交 composing（通过 finishComposing 模拟）
    @Test
    fun testCloseConnectionDiscardsComposing() {
        buffer.loadText("abc")
        buffer.setSelection(3, 3)
        buffer.setComposingText("拼", 1)
        // 模拟 closeConnection：调用 finishComposing 丢弃 composing
        buffer.finishComposing()
        // composing 文本不应出现在 buffer 中
        assertEquals("abc", buffer.text)
        assertFalse(buffer.hasComposing)
    }

    // ── 4. testSetComposingTextDoesNotModifyBuffer ──
    // setComposingText 不修改正文 buffer
    @Test
    fun testSetComposingTextDoesNotModifyBuffer() {
        buffer.loadText("abc")
        buffer.setSelection(3, 3)
        val textBefore = buffer.text
        buffer.setComposingText("拼", 1)
        // composing 不应修改 buffer 的正式文本
        assertEquals(textBefore, buffer.text)
    }

    // ── 5. testGetTextBeforeCursorDuringComposing ──
    // composing 期间 getTextBeforeCursor 返回正文中的内容
    @Test
    fun testGetTextBeforeCursorDuringComposing() {
        buffer.loadText("你好")
        buffer.setSelection(2, 2) // cursor after "你好"
        buffer.setComposingText("世界", 2)
        // getTextBeforeCursor 应返回正文中的内容
        val before = buffer.getTextBeforeCursor(10)
        assertTrue(before.contains("你好"))
    }

    // ── 6. testGetTextAfterCursorDuringComposing ──
    // composing 期间 getTextAfterCursor 返回正文中的内容
    @Test
    fun testGetTextAfterCursorDuringComposing() {
        buffer.loadText("你好世界")
        buffer.setSelection(2, 2) // cursor after "你好"
        buffer.setComposingText("的", 1)
        // getTextAfterCursor 应返回正文中的内容
        val after = buffer.getTextAfterCursor(10)
        assertTrue(after.contains("世界"))
    }

    // ── 7. testEmptyComposingClearsState ──
    // 空 composing 文本清除 composing 状态
    @Test
    fun testEmptyComposingClearsState() {
        buffer.loadText("abc")
        buffer.setSelection(3, 3)
        buffer.setComposingText("拼", 1)
        assertTrue(buffer.hasComposing)
        // 空 composing 等同于清除
        buffer.setComposingText("", 1)
        assertFalse(buffer.hasComposing)
    }

    // ── 8. testCommitTextAfterComposingWithSelection ──
    // composing 后 commitText 替换选区
    @Test
    fun testCommitTextAfterComposingWithSelection() {
        buffer.loadText("abcdef")
        buffer.setSelection(2, 5) // 选中 "cde"
        buffer.setComposingText("拼", 1)
        val result = buffer.commitText("你")
        assertEquals("ab你f", buffer.text)
        assertFalse(buffer.hasComposing)
    }

    // ── 9. testConsecutiveComposingDoesNotModifyBuffer ──
    // 连续 composing 不修改正文
    @Test
    fun testConsecutiveComposingDoesNotModifyBuffer() {
        buffer.loadText("abc")
        buffer.setSelection(3, 3)
        buffer.setComposingText("n", 1)
        assertEquals("abc", buffer.text)
        buffer.setComposingText("ni", 1)
        assertEquals("abc", buffer.text)
        buffer.setComposingText("你", 1)
        assertEquals("abc", buffer.text)
        // 只有 commitText 才进入正文
        buffer.commitText("你")
        assertEquals("abc你", buffer.text)
    }
}
