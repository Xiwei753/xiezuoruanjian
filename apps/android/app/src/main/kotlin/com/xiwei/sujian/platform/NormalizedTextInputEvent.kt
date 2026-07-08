package com.xiwei.sujian.platform

/**
 * 归一化输入事件 — 平台适配层输出，编辑器消费
 *
 * 所有索引均为 UTF-8 byte offset，平台适配层负责一次性转换。
 * composing text 只显示 preedit，不进 undo、不保存、不触发正文动画；
 * commitText 才进正文事务。
 */
sealed class NormalizedTextInputEvent {
    /** 普通文本插入 */
    data class PlainText(val text: String) : NormalizedTextInputEvent()

    /** 快捷键 */
    data class Shortcut(val key: NormalizedKey, val modifiers: NormalizedModifiers) : NormalizedTextInputEvent()

    /** Preedit 文本变化（IME 组合输入） */
    data class PreeditChanged(
        val text: String,
        val cursor: Int,
        val attributes: List<NormalizedPreeditAttribute>
    ) : NormalizedTextInputEvent()

    /** IME commit 上屏 */
    data class ImeCommit(val text: String) : NormalizedTextInputEvent()

    /** IME commit 带替换语义 */
    data class ImeReplacementCommit(
        val text: String,
        val replaceStart: Int,
        val replaceLength: Int
    ) : NormalizedTextInputEvent()

    /** IME 取消 */
    object ImeCancel : NormalizedTextInputEvent()
}

/** 归一化按键 */
enum class NormalizedKey {
    Backspace, Tab, Enter, Insert, Delete,
    Left, Up, Right, Down, Home, End, Escape,
    PageUp, PageDown, Char, Unknown
}

/** 归一化修饰键 */
data class NormalizedModifiers(
    val ctrl: Boolean = false,
    val shift: Boolean = false,
    val alt: Boolean = false,
    val meta: Boolean = false
)

/** 归一化 preedit 属性 */
sealed class NormalizedPreeditAttribute {
    object Underline : NormalizedPreeditAttribute()
    data class TextColor(val color: String) : NormalizedPreeditAttribute()
    data class BackgroundColor(val color: String) : NormalizedPreeditAttribute()
    object FontUnderline : NormalizedPreeditAttribute()
    object Cursor : NormalizedPreeditAttribute()
}
