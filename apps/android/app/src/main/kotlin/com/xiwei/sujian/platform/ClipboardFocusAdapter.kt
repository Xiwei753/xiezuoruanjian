package com.xiwei.sujian.platform

/**
 * Android ClipboardAndFocusAdapter 接口
 *
 * 剪贴板、焦点、软键盘显示隐藏、输入法激活统一从这里走。
 * SujianEditorView 不直接调用 ClipboardManager / InputMethodManager。
 */
interface ClipboardAndFocusAdapter {
    /** 执行剪贴板操作 */
    fun executeClipboard(request: ClipboardRequest): ClipboardResult

    /** 执行焦点请求 */
    fun executeFocus(request: FocusRequest)

    /** 获取当前焦点状态 */
    fun focusState(): FocusState

    /** 显示上下文菜单 */
    fun showContextMenu(request: ContextMenuRequest)

    /** 隐藏上下文菜单 */
    fun hideContextMenu()
}

/** 剪贴板操作请求 */
sealed class ClipboardRequest {
    data class Copy(val text: String) : ClipboardRequest()
    object Paste : ClipboardRequest()
    data class Cut(val text: String) : ClipboardRequest()
    object HasText : ClipboardRequest()
}

/** 剪贴板操作结果 */
sealed class ClipboardResult {
    object Copied : ClipboardResult()
    data class Pasted(val text: String) : ClipboardResult()
    object Cut : ClipboardResult()
    data class HasText(val hasText: Boolean) : ClipboardResult()
    object Unavailable : ClipboardResult()
    data class Error(val message: String) : ClipboardResult()
}

/** 焦点请求 */
enum class FocusRequest {
    RequestFocus, ReleaseFocus, RequestSoftInput, HideSoftInput
}

/** 焦点状态 */
data class FocusState(
    val hasFocus: Boolean = false,
    val softInputVisible: Boolean = false
)

/** 上下文菜单请求 */
data class ContextMenuRequest(
    val screenX: Double,
    val screenY: Double,
    val hasSelection: Boolean,
    val canPaste: Boolean,
    val canUndo: Boolean,
    val canRedo: Boolean
)
