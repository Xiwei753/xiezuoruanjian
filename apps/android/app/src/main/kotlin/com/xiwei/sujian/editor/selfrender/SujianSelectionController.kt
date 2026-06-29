package com.xiwei.sujian.editor.selfrender

import android.graphics.RectF

/**
 * SujianSelectionController — 自研写作区选区控制器
 *
 * 管理文本选区的创建、修改、查询。
 *
 * ## 第一阶段
 * - 点击定位光标
 * - 拖动选择
 * - 长按选词可后置
 * - 复制/剪切/粘贴走系统 Clipboard
 * - 暂不追求系统级选择手柄完全一致
 */
class SujianSelectionController(
    private val buffer: SujianEditorBuffer,
    private val layout: SujianEditorLayout
) {
    /**
     * 点击定位光标
     */
    fun handleTap(x: Float, y: Float) {
        val offset = layout.getOffsetForPosition(buffer.text, x, y)
        buffer.setSelection(offset, offset)
    }
    
    /**
     * 拖动选择（从 anchor 到当前手指位置）
     */
    fun handleDrag(anchorX: Float, anchorY: Float, currentX: Float, currentY: Float) {
        val anchorOffset = layout.getOffsetForPosition(buffer.text, anchorX, anchorY)
        val headOffset = layout.getOffsetForPosition(buffer.text, currentX, currentY)
        buffer.setSelection(anchorOffset, headOffset)
    }
    
    /**
     * 获取选区矩形（用于绘制选区手柄等）
     */
    fun getSelectionRects(): List<RectF> {
        if (buffer.selection.isCollapsed) return emptyList()
        
        val text = buffer.text
        val start = buffer.selection.start
        val end = buffer.selection.end
        
        val rects = mutableListOf<RectF>()
        val startLine = layout.getLineInfo(text, 0) // 简化，实际应从 layout 获取
        // 第一阶段简化：返回整个选区的矩形
        val cursorRect = layout.getCursorRect(text, start)
        val endCursorRect = layout.getCursorRect(text, end)
        rects.add(RectF(
            cursorRect.x,
            cursorRect.top,
            endCursorRect.x,
            endCursorRect.bottom
        ))
        
        return rects
    }
}
