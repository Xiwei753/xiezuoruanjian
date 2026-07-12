package com.xiwei.sujian.editor.selfrender

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import com.xiwei.sujian.model.SujianEditCauseData

/**
 * SujianClipboardController — 自研写作区剪贴板控制器
 *
 * 管理复制/剪切/粘贴操作，走系统 ClipboardManager。
 */
class SujianClipboardController(
    private val context: Context,
    private val buffer: SujianEditorBuffer,
    private val animationController: SujianAnimationController? = null,
    private val layout: SujianEditorLayout? = null,
    private val editorView: SujianEditorView? = null
) {
    private val clipboardManager: ClipboardManager by lazy {
        context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
    }
    
    /**
     * 复制选中文本到剪贴板
     */
    fun copy(): Boolean {
        val selected = buffer.getSelectedText() ?: return false
        val clip = ClipData.newPlainText("text", selected)
        clipboardManager.setPrimaryClip(clip)
        return true
    }
    
    /**
     * 剪切选中文本到剪贴板
     */
    fun cut(): Boolean {
        val selected = buffer.getSelectedText() ?: return false
        val clip = ClipData.newPlainText("text", selected)
        clipboardManager.setPrimaryClip(clip)
        if (layout != null && !buffer.selection.isCollapsed) {
            val selStart = buffer.selection.start
            val selEnd = buffer.selection.end
            if (selStart < selEnd) {
                val oldCursorRect = layout.getCursorRect(buffer.text, buffer.selection.head)
                if (editorView != null) {
                    editorView.preDeleteOldCursorRect = com.xiwei.sujian.model.SujianCursorRectData(
                        oldCursorRect.x.toDouble(),
                        oldCursorRect.top.toDouble(),
                        oldCursorRect.bottom.toDouble(),
                        oldCursorRect.baselineY.toDouble()
                    )
                }
            }
        }
        if (editorView != null) {
            editorView.runVisualEdit(SujianEditCauseData.Delete) {
                buffer.commitText("", SujianEditCause.Delete)
            }
        } else {
            buffer.commitText("", SujianEditCause.Delete)
        }
        return true
    }
    
    /**
     * 从剪贴板粘贴文本
     */
    fun paste(): Boolean {
        val clip = clipboardManager.primaryClip ?: return false
        if (clip.itemCount == 0) return false
        val pasteText = clip.getItemAt(0)?.text?.toString() ?: return false
        // 使用 runVisualEdit 包装粘贴操作
        if (editorView != null) {
            editorView.runVisualEdit(SujianEditCauseData.Paste) {
                buffer.commitText(pasteText, SujianEditCause.Paste)
            }
        } else {
            buffer.commitText(pasteText, SujianEditCause.Paste)
        }
        return true
    }
    
    /**
     * 获取剪贴板文本
     */
    fun getClipboardText(): String? {
        val clip = clipboardManager.primaryClip ?: return null
        if (clip.itemCount == 0) return null
        return clip.getItemAt(0)?.text?.toString()
    }
}
