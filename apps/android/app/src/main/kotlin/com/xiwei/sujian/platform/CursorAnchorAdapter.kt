package com.xiwei.sujian.platform

/**
 * Android CursorAnchorAdapter 接口
 *
 * IMM.updateSelection / updateCursorAnchorInfo 收敛到此。
 * SujianEditorView 只调用统一接口，不直接知道 IMM 细节。
 */
interface CursorAnchorAdapter {
    /** 通知系统输入法光标/选区已更新 */
    fun notifyCursorAnchorUpdate(
        cursorIndex: Int,
        anchorIndex: Int,
        selectionStart: Int,
        selectionEnd: Int,
        textBeforeCursor: String,
        textAfterCursor: String,
    )

    /** 请求系统更新候选框位置 */
    fun requestCandidateWindowUpdate(cursorRect: NormalizedCursorRect)

    /** 查询系统输入法是否可见 */
    fun isInputMethodVisible(): Boolean = false
}

/** 归一化光标矩形 */
data class NormalizedCursorRect(
    val x: Double,
    val top: Double,
    val bottom: Double,
    val baselineY: Double,
)
