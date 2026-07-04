package com.xiwei.sujian.editor.selfrender

import android.os.Bundle
import android.view.KeyEvent
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.ExtractedText
import android.view.inputmethod.ExtractedTextRequest
import android.view.inputmethod.InputConnection
import android.view.View
import com.xiwei.sujian.model.SujianEditCauseData

/**
 * SujianInputConnection — 自研写作区的 InputConnection
 *
 * 替代系统 EditText 的 InputConnection，直接与 SujianEditorBuffer 交互。
 *
 * ## IME 规则
 * - setComposingText 只更新 preedit 层，不触发吐字/吞字
 * - composing text 不进 undo/保存/正文动画
 * - commitText 才插入正文 buffer，触发 TypingCommit
 * - finishComposingText 清 preedit
 * - deleteSurroundingText 先拍删除前 glyph rect，再更新 buffer，再播放吞字动画
 * - 每次文本/选区变化后 InputMethodManager.updateSelection
 */
class SujianInputConnection(
    private val view: View,
    private val buffer: SujianEditorBuffer,
    private val imeController: SujianImeController
) : BaseInputConnection(view, true) {

    private val TAG = "SujianInputConn"

    private var batchEditCount = 0
    private var isClosed = false

    // ── 批量编辑 ──

    override fun beginBatchEdit(): Boolean {
        batchEditCount++
        return true
    }

    override fun endBatchEdit(): Boolean {
        batchEditCount--
        return batchEditCount > 0
    }

    // ── 文本提交 ──

    override fun commitText(text: CharSequence?, newCursorPosition: Int): Boolean {
        if (isClosed || text == null) return false

        val textStr = text.toString()

        // 空字符串：如果有选区，删除选区；否则无操作
        if (textStr.isEmpty()) {
            if (!buffer.selection.isCollapsed) {
                val editorView = view as? SujianEditorView
                if (editorView != null) {
                    editorView.runVisualEdit(SujianEditCauseData.Delete) {
                        val result = buffer.deleteSelectionAsEdit(SujianEditCause.Delete)
                        imeController.onEditResult(result)
                        imeController.updateSelection()
                    }
                } else {
                    val result = buffer.deleteSelectionAsEdit(SujianEditCause.Delete)
                    imeController.onEditResult(result)
                    imeController.updateSelection()
                }
            }
            return true
        }

        // 判断是 Typing 还是 TypingCommit
        val wasComposing = buffer.hasComposing
        val cause = if (wasComposing) {
            SujianEditCauseData.TypingCommit
        } else if (textStr.length == 1) {
            SujianEditCauseData.Typing
        } else {
            SujianEditCauseData.TypingCommit
        }

        // 使用 runVisualEdit 包装编辑操作
        val editorView = view as? SujianEditorView
        if (editorView != null) {
            editorView.runVisualEdit(cause) {
                val result = buffer.replaceSelectionOrInsert(textStr, cause.toLegacyCause())
                imeController.onEditResult(result)
                imeController.updateSelection()
            }
        } else {
            // fallback：旧版 WriterEditText 不修改
            val result = buffer.replaceSelectionOrInsert(textStr, cause.toLegacyCause())
            imeController.onEditResult(result)
            imeController.updateSelection()
        }

        return true
    }

    // ── Composing ──

    override fun setComposingText(text: CharSequence?, newCursorPosition: Int): Boolean {
        if (isClosed) return false

        if (text.isNullOrEmpty()) {
            finishComposingText()
            return true
        }

        // composing text 不进正文 buffer，只更新 preedit 状态
        buffer.setComposingText(text.toString(), newCursorPosition)
        imeController.onComposingChanged(text.toString())
        imeController.updateSelection()

        // 触发重绘以显示 composing 文字
        view.invalidate()

        return true
    }

    override fun setComposingRegion(start: Int, end: Int): Boolean {
        if (isClosed) return false

        buffer.setComposingRegion(start, end)
        imeController.updateSelection()

        // 触发重绘以显示 composing 下划线
        view.invalidate()

        return true
    }

    override fun finishComposingText(): Boolean {
        if (isClosed) return false

        // finishComposingText 只清除 composing 状态，不提交文本到正文，不移动光标。
        // composing 文本是 IME 临时状态，不应进入 undo/保存。
        // IME 通常在 finishComposingText 之前或之后调用 commitText 来正式提交文本。
        // 如果 IME 不调用 commitText（如用户点击取消），composing 文本应被丢弃。
        buffer.finishComposing()
        imeController.onComposingFinished()
        imeController.updateSelection()

        // 触发重绘以清除 composing 文字
        view.invalidate()

        return true
    }

    // ── 删除 ──

    override fun deleteSurroundingText(beforeLength: Int, afterLength: Int): Boolean {
        if (isClosed) return false
        if (beforeLength < 0 || afterLength < 0) return false

        // 使用 runVisualEdit 包装删除操作
        val editorView = view as? SujianEditorView
        if (editorView != null) {
            // 先记录删除前 glyph rect（用于动画）
            imeController.onBeforeDelete(beforeLength, afterLength)
            editorView.runVisualEdit(SujianEditCauseData.Delete) {
                val result = buffer.deleteSurrounding(beforeLength, afterLength)
                imeController.onEditResult(result)
                imeController.updateSelection()
            }
        } else {
            // fallback
            imeController.onBeforeDelete(beforeLength, afterLength)
            val result = buffer.deleteSurrounding(beforeLength, afterLength)
            imeController.onEditResult(result)
            imeController.updateSelection()
        }

        return true
    }

    override fun deleteSurroundingTextInCodePoints(beforeLength: Int, afterLength: Int): Boolean {
        if (isClosed) return false
        if (beforeLength < 0 || afterLength < 0) return false

        val text = buffer.text
        val cursorPos = buffer.selection.head

        // 将 code point 数量转换为 UTF-16 unit 数量
        // 向前扫描：从 cursorPos 向左数 beforeLength 个 code point
        var utf16Before = 0
        var offset = cursorPos
        var cpCount = 0
        while (cpCount < beforeLength && offset > 0) {
            offset -= 1
            // 如果当前是低代理项且前面有高代理项，它们组成一个 code point（surrogate pair）
            if (offset > 0 && Character.isSurrogatePair(text[offset - 1], text[offset])) {
                offset -= 1
            }
            cpCount++
        }
        utf16Before = cursorPos - offset

        // 向后扫描：从 cursorPos 向右数 afterLength 个 code point
        var utf16After = 0
        offset = cursorPos
        cpCount = 0
        while (cpCount < afterLength && offset < text.length) {
            val ch = text[offset]
            if (Character.isHighSurrogate(ch) && offset + 1 < text.length && Character.isLowSurrogate(text[offset + 1])) {
                offset += 2
            } else {
                offset += 1
            }
            cpCount++
        }
        utf16After = offset - cursorPos

        return deleteSurroundingText(utf16Before, utf16After)
    }

    // ── 文本查询 ──

    override fun getTextBeforeCursor(length: Int, flags: Int): CharSequence {
        if (isClosed) return ""
        return buffer.getTextBeforeCursor(length)
    }

    override fun getTextAfterCursor(length: Int, flags: Int): CharSequence {
        if (isClosed) return ""
        return buffer.getTextAfterCursor(length)
    }

    override fun getSelectedText(flags: Int): CharSequence? {
        if (isClosed) return null
        return buffer.getSelectedText()
    }

    override fun getExtractedText(request: ExtractedTextRequest?, flags: Int): ExtractedText {
        val et = ExtractedText()
        if (isClosed) return et

        et.text = buffer.text
        et.selectionStart = buffer.selection.start
        et.selectionEnd = buffer.selection.end
        et.startOffset = 0

        if (request != null) {
            val partialStart = request.hintMaxChars.coerceAtMost(buffer.text.length)
            if (partialStart > 0 && buffer.text.length > partialStart) {
                et.startOffset = buffer.text.length - partialStart
                et.text = buffer.text.substring(et.startOffset)
            }
        }

        return et
    }

    // ── 选区 ──

    override fun setSelection(start: Int, end: Int): Boolean {
        if (isClosed) return false

        buffer.setSelection(start, end)
        imeController.updateSelection()

        return true
    }

    // ── 键盘事件 ──

    override fun sendKeyEvent(event: KeyEvent): Boolean {
        if (isClosed) return false

        if (event.action == KeyEvent.ACTION_DOWN) {
            when (event.keyCode) {
                KeyEvent.KEYCODE_DEL -> {
                    // Backspace
                    if (buffer.selection.isCollapsed) {
                        deleteSurroundingText(1, 0)
                    } else {
                        imeController.onBeforeDeleteSelection()
                        val editorView = view as? SujianEditorView
                        if (editorView != null) {
                            editorView.runVisualEdit(SujianEditCauseData.Delete) {
                                val result = buffer.deleteSelectionAsEdit(SujianEditCause.Delete)
                                imeController.onEditResult(result)
                                imeController.updateSelection()
                            }
                        } else {
                            val result = buffer.deleteSelectionAsEdit(SujianEditCause.Delete)
                            imeController.onEditResult(result)
                            imeController.updateSelection()
                        }
                    }
                    return true
                }
                KeyEvent.KEYCODE_FORWARD_DEL -> {
                    // Forward delete
                    if (buffer.selection.isCollapsed) {
                        deleteSurroundingText(0, 1)
                    } else {
                        imeController.onBeforeDeleteSelection()
                        val editorView = view as? SujianEditorView
                        if (editorView != null) {
                            editorView.runVisualEdit(SujianEditCauseData.Delete) {
                                val result = buffer.deleteSelectionAsEdit(SujianEditCause.Delete)
                                imeController.onEditResult(result)
                                imeController.updateSelection()
                            }
                        } else {
                            val result = buffer.deleteSelectionAsEdit(SujianEditCause.Delete)
                            imeController.onEditResult(result)
                            imeController.updateSelection()
                        }
                    }
                    return true
                }
                KeyEvent.KEYCODE_ENTER -> {
                    val editorView = view as? SujianEditorView
                    if (editorView != null) {
                        editorView.runVisualEdit(SujianEditCauseData.Typing) {
                            val result = buffer.commitText("\n", SujianEditCause.Typing)
                            imeController.onEditResult(result)
                            imeController.updateSelection()
                        }
                    } else {
                        val result = buffer.commitText("\n", SujianEditCause.Typing)
                        imeController.onEditResult(result)
                        imeController.updateSelection()
                    }
                    return true
                }
            }
        }

        return super.sendKeyEvent(event)
    }

    // ── 上下文菜单 ──

    override fun performContextMenuAction(id: Int): Boolean {
        when (id) {
            android.R.id.selectAll -> {
                buffer.setSelection(0, buffer.text.length)
                imeController.updateSelection()
                return true
            }
            android.R.id.cut -> {
                val selected = buffer.getSelectedText()
                if (selected != null) {
                    imeController.onClipboardCopy(selected, isCut = true)
                    imeController.onBeforeDeleteSelection()
                    val editorView = view as? SujianEditorView
                    if (editorView != null) {
                        editorView.runVisualEdit(SujianEditCauseData.Delete) {
                            val result = buffer.deleteSelectionAsEdit(SujianEditCause.Delete)
                            imeController.onEditResult(result)
                            imeController.updateSelection()
                        }
                    } else {
                        val result = buffer.deleteSelectionAsEdit(SujianEditCause.Delete)
                        imeController.onEditResult(result)
                        imeController.updateSelection()
                    }
                }
                return true
            }
            android.R.id.copy -> {
                val selected = buffer.getSelectedText()
                if (selected != null) {
                    imeController.onClipboardCopy(selected, isCut = false)
                }
                return true
            }
            android.R.id.paste -> {
                val pasteText = imeController.onClipboardPaste()
                if (pasteText != null) {
                    val editorView = view as? SujianEditorView
                    if (editorView != null) {
                        editorView.runVisualEdit(SujianEditCauseData.Paste) {
                            val result = buffer.replaceSelectionOrInsert(pasteText, SujianEditCause.Paste)
                            imeController.onEditResult(result)
                            imeController.updateSelection()
                        }
                    } else {
                        val result = buffer.replaceSelectionOrInsert(pasteText, SujianEditCause.Paste)
                        imeController.onEditResult(result)
                        imeController.updateSelection()
                    }
                }
                return true
            }
        }
        return false
    }

    // ── Cursor updates ──

    override fun requestCursorUpdates(cursorUpdateMode: Int): Boolean {
        imeController.requestCursorUpdate(cursorUpdateMode)
        return true
    }

    // ── 关闭 ──

    override fun closeConnection() {
        isClosed = true
        batchEditCount = 0
        // 关闭连接时丢弃未提交的 composing 文本，不提交到正文
        buffer.finishComposing()
        super.closeConnection()
    }
}

/**
 * SujianEditCauseData → SujianEditCause 转换
 * 用于将新的 Phase 2 cause 枚举映射到 buffer 层的 legacy cause
 */
private fun SujianEditCauseData.toLegacyCause(): SujianEditCause = when (this) {
    SujianEditCauseData.Typing -> SujianEditCause.Typing
    SujianEditCauseData.Delete -> SujianEditCause.Delete
    SujianEditCauseData.ImeComposition -> SujianEditCause.ImeComposition
    SujianEditCauseData.TypingCommit -> SujianEditCause.TypingCommit
    SujianEditCauseData.Paste -> SujianEditCause.Paste
    SujianEditCauseData.Undo -> SujianEditCause.Programmatic
    SujianEditCauseData.Redo -> SujianEditCause.Programmatic
    SujianEditCauseData.Load -> SujianEditCause.Load
    SujianEditCauseData.Format -> SujianEditCause.Format
    SujianEditCauseData.Programmatic -> SujianEditCause.Programmatic
}
