package com.xiwei.sujian.editor.selfrender

import org.junit.Assert.*
import org.junit.Before
import org.junit.Test

/**
 * SujianEditorBuffer 单元测试
 *
 * 验证核心文本编辑操作：选区处理、commit、删除、剪切、粘贴等。
 * 只使用 SujianEditorBuffer 的公共 API。
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
        buffer.loadText("abc")
        buffer.setSelection(0, 3) // 选中 "abc"
        val result = buffer.commitText("你")
        assertEquals("你", buffer.text)
        assertEquals(1, result.selection.head) // 光标在 "你" 之后
    }

    // ── 2. testCommitTextEmptyWithSelection ──
    // 选区下 commitText("") → 删除选区
    @Test
    fun testCommitTextEmptyWithSelection() {
        buffer.loadText("abcdef")
        buffer.setSelection(2, 5) // 选中 "cde"
        val result = buffer.commitText("")
        assertEquals("abf", buffer.text)
        assertEquals(2, result.selection.head)
    }

    // ── 3. testBackspaceWithSelection ──
    // 有选区时 deleteSelectionAsEdit 删除选区
    @Test
    fun testBackspaceWithSelection() {
        buffer.loadText("abcdef")
        buffer.setSelection(1, 4) // 选中 "bcd"
        val result = buffer.deleteSelectionAsEdit(SujianEditCause.Delete)
        assertEquals("aef", buffer.text)
        assertEquals(1, result.selection.head)
    }

    // ── 4. testForwardDeleteWithSelection ──
    // ForwardDelete 有选区时也删除选区（与 Backspace 行为一致）
    @Test
    fun testForwardDeleteWithSelection() {
        buffer.loadText("abcdef")
        buffer.setSelection(1, 4) // 选中 "bcd"
        val result = buffer.deleteSelectionAsEdit(SujianEditCause.Delete)
        assertEquals("aef", buffer.text)
        assertEquals(1, result.selection.head)
    }

    // ── 5. testCutCopiesAndDeletesSelection ──
    // Cut: getSelectedText + deleteSelection
    @Test
    fun testCutCopiesAndDeletesSelection() {
        buffer.loadText("abcdef")
        buffer.setSelection(2, 5) // 选中 "cde"
        val cutText = buffer.getSelectedText()
        assertEquals("cde", cutText)
        buffer.deleteSelection()
        assertEquals("abf", buffer.text)
    }

    // ── 6. testPasteWithSelection ──
    // Paste 替换选区
    @Test
    fun testPasteWithSelection() {
        buffer.loadText("abcdef")
        buffer.setSelection(1, 4) // 选中 "bcd"
        val result = buffer.replaceSelectionOrInsert("XYZ", SujianEditCause.Paste)
        assertEquals("aXYZef", buffer.text)
        assertEquals(4, result.selection.head) // 光标在 "XYZ" 之后
    }

    // ── 7. testCommitTextReturnsCorrectSelection ──
    // 正确的 SujianEditResult
    @Test
    fun testCommitTextReturnsCorrectSelection() {
        buffer.loadText("hello")
        // loadText 后光标在 0，需要移到末尾
        buffer.setSelection(5, 5)
        val result = buffer.commitText("世界")
        assertEquals("hello世界", buffer.text)
        assertEquals(buffer.text.length, result.selection.head)
        assertEquals(buffer.text.length, result.selection.anchor)
    }

    // ── 8. testTypingCommitReturnsCorrectResult ──
    // 多字 commitText
    @Test
    fun testTypingCommitReturnsCorrectResult() {
        buffer.loadText("")
        val result = buffer.commitText("风和日丽")
        assertEquals("风和日丽", buffer.text)
        assertEquals(buffer.text.length, result.selection.head)
        assertEquals(SujianEditCause.TypingCommit, result.cause)
    }

    // ── 9. testDeleteSurroundingClampsToCharBoundary ──
    // 不拆开 surrogate pair
    @Test
    fun testDeleteSurroundingClampsToCharBoundary() {
        // "😀" is a surrogate pair (U+1F600): 2 UTF-16 code units, 1 code point
        buffer.loadText("A😀B")
        // loadText 后光标在 0，移到 emoji 之后 (A=1, 😀=2 units, cursor at 3)
        buffer.setSelection(3, 3)
        // Delete 1 char before cursor — should delete the entire emoji, not half
        val result = buffer.deleteSurrounding(1, 0)
        assertEquals("AB", buffer.text)
        assertEquals(1, result.selection.head)
    }

    // ── 10. testDeleteSurroundingForwardClampsToCharBoundary ──
    // Forward delete 也不拆开 surrogate pair
    @Test
    fun testDeleteSurroundingForwardClampsToCharBoundary() {
        buffer.loadText("A😀B")
        // cursor after "A" (position 1)
        buffer.setSelection(1, 1)
        // Delete 1 char after cursor — should delete the entire emoji
        val result = buffer.deleteSurrounding(0, 1)
        assertEquals("AB", buffer.text)
        assertEquals(1, result.selection.head)
    }
}
