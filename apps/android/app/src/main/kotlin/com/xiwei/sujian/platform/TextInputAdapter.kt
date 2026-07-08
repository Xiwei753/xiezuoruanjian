package com.xiwei.sujian.platform

/**
 * Android TextInputAdapter 接口
 *
 * SujianInputConnection 将 Android 输入事件转换为 NormalizedTextInputEvent，
 * SujianEditorView 只消费归一化事件，不直接知道 IMM 细节。
 *
 * composing text 只显示 preedit，不进 undo、不保存、不触发正文动画；
 * commitText 才进正文事务。
 */
interface TextInputAdapter {
    /** 将 Android 输入事件转换为归一化事件 */
    fun normalizeInputEvent(raw: Any): NormalizedTextInputEvent?

    /** 当前是否正在 IME composing */
    fun isImeComposing(): Boolean

    /** 是否可以接受纯文本按键 */
    fun canAcceptPlainTextKey(): Boolean = !isImeComposing()

    /** UTF-16 offset → UTF-8 byte offset 转换 */
    fun utf16ToUtf8Offset(text: String, utf16Offset: Int): Int

    /** UTF-8 byte offset → UTF-16 offset 转换 */
    fun utf8ToUtf16Offset(text: String, utf8Offset: Int): Int
}
