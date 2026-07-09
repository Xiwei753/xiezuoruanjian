package com.xiwei.sujian.editor.selfrender

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.graphics.Matrix
import android.view.View
import android.view.inputmethod.CursorAnchorInfo
import android.view.inputmethod.InputMethodManager
import android.os.Build
import com.xiwei.sujian.diagnostics.DiagnosticsLogger

/**
 * SujianImeController — 自研写作区 IME 控制器
 *
 * 管理 InputMethodManager 交互、updateSelection、CursorAnchorInfo、剪贴板。
 *
 * ## 职责
 * - updateSelection：每次文本/选区变化后通知 IMM
 * - CursorAnchorInfo：让候选框跟随光标
 * - 剪贴板：复制/剪切/粘贴
 * - 删除前记录 glyph rect（委托给 AnimationController）
 *
 * ## 注意
 * - 动画事件由 SujianAnimationController 通过 buffer.onTextChanged 回调统一处理
 * - 本控制器不再重复处理动画事件
 */
class SujianImeController(
    private val view: View,
    private val buffer: SujianEditorBuffer,
    private val layout: SujianEditorLayout,
    private val renderer: SujianEditorRenderer,
    private val animationController: SujianAnimationController
) {
    private val TAG = "SujianImeCtrl"
    
    private val imm: InputMethodManager by lazy {
        view.context.getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager
    }
    
    private val clipboardManager: ClipboardManager by lazy {
        view.context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
    }
    
    // ── 删除前快照 ──
    // 已委托给 SujianAnimationController.recordDeleteSnapshot()
    // 本控制器不再维护 preDeleteGlyphRects/preDeleteCursorRect
    
    /**
     * 通知 IMM 选区变化
     *
     * 同时更新 CursorAnchorInfo，确保候选框跟随光标。
     * 每次 selection/cursor/layout/scroll 改变后都应调用此方法。
     */
    fun updateSelection() {
        val selStart = buffer.selection.start
        val selEnd = buffer.selection.end
        val composingStart = if (buffer.hasComposing) buffer.composingStart else -1
        val composingEnd = if (buffer.hasComposing) buffer.composingEnd else -1
        
        imm.updateSelection(
            view,
            selStart,
            selEnd,
            composingStart,
            composingEnd
        )
        
        // 同步更新 CursorAnchorInfo，让候选框跟随光标
        notifyCursorAnchorInfoChanged()
    }
    
    /**
     * 通知 IMM CursorAnchorInfo 已变化（scroll/layout 变化时调用）
     */
    fun notifyCursorAnchorInfoChanged() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.LOLLIPOP) {
            try {
                val info = buildCursorAnchorInfo()
                if (info != null) {
                    imm.updateCursorAnchorInfo(view, info)
                }
            } catch (e: Throwable) {
                DiagnosticsLogger.e(TAG, "notifyCursorAnchorInfoChanged failed: ${e.message}")
            }
        }
    }
    
    /**
     * 请求光标更新（CursorAnchorInfo）
     */
    fun requestCursorUpdate(cursorUpdateMode: Int) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.LOLLIPOP) {
            try {
                val info = buildCursorAnchorInfo()
                if (info != null) {
                    imm.updateCursorAnchorInfo(view, info)
                }
            } catch (e: Throwable) {
                DiagnosticsLogger.e(TAG, "requestCursorUpdate failed: ${e.message}")
            }
        }
    }
    
    /**
     * 编辑结果处理
     * 动画事件已由 SujianAnimationController 通过 buffer.onTextChanged 回调统一处理，
     * 此处不再重复处理动画。
     */
    fun onEditResult(result: SujianEditResult) {
        // 动画事件由 SujianAnimationController.handleVisualEdit() 处理
        // 此处仅做 IME 相关的善后工作
    }
    
    /**
     * 删除前记录 glyph rect（委托给 AnimationController）
     * 返回 animationId，用于精确匹配删除动画
     *
     * 同时设置 SujianEditorView.preDeleteOldCursorRect，供 runVisualEdit(Delete) 复用
     */
    fun onBeforeDelete(beforeLength: Int, afterLength: Int): ULong {
        val text = buffer.text
        if (text.isEmpty()) return 0u

        val cursorPos = buffer.selection.head
        var deleteStart = (cursorPos - beforeLength).coerceAtLeast(0)
        var deleteEnd = (cursorPos + afterLength).coerceAtMost(text.length)

        // 钳位到 code point 边界，避免拆开 surrogate pair
        deleteStart = SujianEditorBuffer.clampToCharBoundary(text, deleteStart)
        if (deleteEnd < text.length && Character.isLowSurrogate(text[deleteEnd])
            && deleteEnd > 0 && Character.isHighSurrogate(text[deleteEnd - 1])) {
            deleteEnd += 1
        }

        if (deleteStart < deleteEnd) {
            val deletedGlyphRects = layout.getGlyphRects(text, deleteStart, deleteEnd)
            val oldCursorRect = layout.getCursorRect(text, cursorPos)
            val deletedText = text.substring(deleteStart, deleteEnd)

            // 设置 preDeleteOldCursorRect 供 runVisualEdit 复用
            val editorView = view as? SujianEditorView
            if (editorView != null) {
                editorView.preDeleteOldCursorRect = com.xiwei.sujian.model.SujianCursorRectData(
                    oldCursorRect.x.toDouble(),
                    oldCursorRect.top.toDouble(),
                    oldCursorRect.bottom.toDouble(),
                    oldCursorRect.baselineY.toDouble()
                )
            }

            return animationController.recordDeleteSnapshot(deletedText, deletedGlyphRects, oldCursorRect)
        }
        return 0u
    }

    /**
     * 选区删除前记录 glyph rect（委托给 AnimationController）
     * 用于有选区时的删除操作
     *
     * 同时设置 SujianEditorView.preDeleteOldCursorRect，供 runVisualEdit 复用
     */
    fun onBeforeDeleteSelection(): ULong {
        val text = buffer.text
        if (text.isEmpty() || buffer.selection.isCollapsed) return 0u

        val selStart = buffer.selection.start
        val selEnd = buffer.selection.end
        if (selStart < selEnd) {
            val deletedGlyphRects = layout.getGlyphRects(text, selStart, selEnd)
            val oldCursorRect = layout.getCursorRect(text, buffer.selection.head)
            val deletedText = text.substring(selStart, selEnd)

            // 设置 preDeleteOldCursorRect 供 runVisualEdit 复用
            val editorView = view as? SujianEditorView
            if (editorView != null) {
                editorView.preDeleteOldCursorRect = com.xiwei.sujian.model.SujianCursorRectData(
                    oldCursorRect.x.toDouble(),
                    oldCursorRect.top.toDouble(),
                    oldCursorRect.bottom.toDouble(),
                    oldCursorRect.baselineY.toDouble()
                )
            }

            return animationController.recordDeleteSnapshot(deletedText, deletedGlyphRects, oldCursorRect)
        }
        return 0u
    }
    
    /**
     * Composing 变化通知
     * composing 开始时清除静态层跳过范围，避免动画卡住
     */
    fun onComposingChanged(composingText: String) {
        if (composingText.isNotEmpty()) {
            renderer.clearActiveInsertRanges()
        }
        DiagnosticsLogger.d(TAG, "Composing changed: len=${composingText.length}")
    }
    
    /**
     * Composing 结束通知
     */
    fun onComposingFinished() {
        DiagnosticsLogger.d(TAG, "Composing finished")
    }
    
    /**
     * 剪贴板复制/剪切
     */
    fun onClipboardCopy(text: String, isCut: Boolean) {
        val clip = ClipData.newPlainText("text", text)
        clipboardManager.setPrimaryClip(clip)
    }
    
    /**
     * 剪贴板粘贴
     */
    fun onClipboardPaste(): String? {
        val clip = clipboardManager.primaryClip ?: return null
        if (clip.itemCount == 0) return null
        return clip.getItemAt(0)?.text?.toString()
    }
    
    /**
     * 显示软键盘
     */
    fun showSoftInput() {
        if (view.isFocused) {
            imm.showSoftInput(view, 0)
        }
    }
    
    /**
     * 隐藏软键盘
     */
    fun hideSoftInput() {
        imm.hideSoftInputFromWindow(view.windowToken, 0)
    }
    
    // ── 内部方法 ──
    
    private fun buildCursorAnchorInfo(): CursorAnchorInfo? {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.LOLLIPOP) return null

        val builder = CursorAnchorInfo.Builder()
        val text = buffer.text

        val cursorRect: SujianCursorRect
        if (buffer.hasComposing && renderer.hasComposingCursor) {
            cursorRect = SujianCursorRect(
                renderer.composingCursorX,
                renderer.composingCursorTop,
                renderer.composingCursorBottom,
                renderer.composingCursorBottom
            )
        } else {
            cursorRect = layout.getCursorRect(text, buffer.selection.head)
        }

        val flags = computeCursorVisibilityFlags(cursorRect)

        val editorView = view as? SujianEditorView
        val scrollX = editorView?.touchController?.scrollX?.toFloat() ?: 0f
        val scrollY = editorView?.touchController?.scrollY?.toFloat() ?: 0f
        val padLeft = editorView?.paddingLeft?.toFloat() ?: 0f
        val padTop = editorView?.paddingTop?.toFloat() ?: 0f

        builder.setSelectionRange(buffer.selection.start, buffer.selection.end)
        builder.setInsertionMarkerLocation(
            cursorRect.x + padLeft - scrollX,
            cursorRect.top + padTop - scrollY,
            cursorRect.baselineY + padTop - scrollY,
            cursorRect.bottom + padTop - scrollY,
            flags
        )

        val matrix = Matrix()
        try {
            view.transformMatrixToGlobal(matrix)
        } catch (_: Throwable) {
            matrix.reset()
        }
        builder.setMatrix(matrix)

        return builder.build()
    }
    
    /**
     * 计算光标可见性 flags
     *
     * 根据 Android 官方文档：
     * - FLAG_HAS_VISIBLE_REGION：光标在可视区域内
     * - FLAG_HAS_INVISIBLE_REGION：光标被裁剪/不可见
     * - 两者可以组合（部分可见）
     */
    private fun computeCursorVisibilityFlags(cursorRect: SujianCursorRect): Int {
        val editorView = view as? SujianEditorView
        if (editorView == null) {
            return CursorAnchorInfo.FLAG_HAS_INVISIBLE_REGION
        }
        
        val scrollY = editorView.touchController.scrollY
        val viewportTop = scrollY
        val viewportBottom = scrollY + (editorView.height - editorView.paddingTop - editorView.paddingBottom)
        val viewportLeft = editorView.touchController.scrollX
        val viewportRight = viewportLeft + (editorView.width - editorView.paddingLeft - editorView.paddingRight)
        
        val cursorTop = cursorRect.top
        val cursorBottom = cursorRect.bottom
        val cursorLeft = cursorRect.x
        val cursorRight = cursorRect.x + 2f  // 光标宽度约 2dp
        
        val verticallyVisible = cursorBottom > viewportTop && cursorTop < viewportBottom
        val horizontallyVisible = cursorRight > viewportLeft && cursorLeft < viewportRight
        
        var flags = 0
        if (verticallyVisible && horizontallyVisible) {
            flags = flags or CursorAnchorInfo.FLAG_HAS_VISIBLE_REGION
        }
        if (!verticallyVisible || !horizontallyVisible) {
            flags = flags or CursorAnchorInfo.FLAG_HAS_INVISIBLE_REGION
        }
        
        // 如果完全不可见，至少设置 INVISIBLE
        if (flags == 0) {
            flags = CursorAnchorInfo.FLAG_HAS_INVISIBLE_REGION
        }
        
        return flags
    }
}
