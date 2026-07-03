package com.xiwei.sujian.editor.selfrender

import org.junit.Assert.*
import org.junit.Before
import org.junit.Test

/**
 * SujianEditorBuffer 单元测试
 *
 * 验证核心文本编辑操作：选区处理、commit、删除、剪切、粘贴等。
 */
class SujianEditorBufferTest {

    private lateinit var buffer: SujianEditorBuffer

    @Before
    fun setUp() {
        buffer = SujianEditorBuffer()
    }

    // ── 1. testCommitTextWithSelection ──
    // 选中 "abc" 输入 "你" → 文本变为 "你"
    @Test
    fun testCommitTextWithSelection() {
        buffer.text = "abc"
        buffer.setSelection(0, 3) // 选中 "abc"
        val result = buffer.commitText("你")
        assertEquals("你", buffer.text)
        assertEquals(1, result.selectionHead) // 光标在 "你" 之后
    }

    // ── 2. testCommitTextEmptyWithSelection ──
    // 选区下 commitText("") → 删除选区
    @Test
    fun testCommitTextEmptyWithSelection() {
        buffer.text = "abcdef"
        buffer.setSelection(2, 5) // 选中 "cde"
        val result = buffer.commitText("")
        assertEquals("abf", buffer.text)
        assertEquals(2, result.selectionHead)
    }

    // ── 3. testBackspaceWithSelection ──
    // 有选区时删除选区
    @Test
    fun testBackspaceWithSelection() {
        buffer.text = "abcdef"
        buffer.setSelection(1, 4) // 选中 "bcd"
        buffer.deleteBackward()
        assertEquals("aef", buffer.text)
    }

    // ── 4. testForwardDeleteWithSelection ──
    // ForwardDelete 有选区时删除选区
    @Test
    fun testForwardDeleteWithSelection() {
        buffer.text = "abcdef"
        buffer.setSelection(1, 4) // 选中 "bcd"
        buffer.deleteForward()
        assertEquals("aef", buffer.text)
    }

    // ── 5. testCutCopiesAndDeletesSelection ──
    // Cut 复制后删除选区
    @Test
    fun testCutCopiesAndDeletesSelection() {
        buffer.text = "abcdef"
        buffer.setSelection(2, 5) // 选中 "cde"
        val cutText = buffer.cut()
        assertEquals("cde", cutText)
        assertEquals("abf", buffer.text)
    }

    // ── 6. testPasteWithSelection ──
    // Paste 替换选区
    @Test
    fun testPasteWithSelection() {
        buffer.text = "abcdef"
        buffer.setSelection(1, 4) // 选中 "bcd"
        buffer.paste("XYZ")
        assertEquals("aXYZef", buffer.text)
    }

    // ── 7. testCommitTextReturnsCorrectSelection ──
    // 正确的 SujianEditResult
    @Test
    fun testCommitTextReturnsCorrectSelection() {
        buffer.text = "hello"
        buffer.setCursor(5)
        val result = buffer.commitText("世界")
        assertEquals("hello世界", buffer.text)
        assertEquals(buffer.text.length, result.selectionHead)
        assertEquals(buffer.text.length, result.selectionAnchor)
    }

    // ── 8. testTypingCommitReturnsCorrectResult ──
    // 多字 commitText
    @Test
    fun testTypingCommitReturnsCorrectResult() {
        buffer.text = ""
        buffer.setCursor(0)
        val result = buffer.commitText("风和日丽")
        assertEquals("风和日丽", buffer.text)
        assertEquals(buffer.text.length, result.selectionHead)
    }

    // ── 9. testDeleteSurroundingClampsToCharBoundary ──
    // 不拆开 surrogate pair
    @Test
    fun testDeleteSurroundingClampsToCharBoundary() {
        // "😀" is a surrogate pair (U+1F600): 2 UTF-16 code units, 1 code point
        buffer.text = "A😀B"
        buffer.setCursor(3) // cursor after the surrogate pair (A=1, 😀=2 units, cursor at 3)
        // Delete 1 char before cursor — should delete the entire emoji, not half
        buffer.deleteBackward()
        assertEquals("AB", buffer.text)
    }
}
