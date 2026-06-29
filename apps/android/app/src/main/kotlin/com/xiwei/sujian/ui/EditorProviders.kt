package com.xiwei.sujian.ui

import com.xiwei.sujian.model.EditorAnimationEventData
import com.xiwei.sujian.model.EditorVisualTransactionData

/**
 * Legacy: 动画事件提供者 SAM 接口。
 *
 * 仅用于旧版 WriterEditText fallback，自研写作区（SujianEditorView）不再使用此链路。
 * 自研写作区统一走 VisualTransactionProvider。
 */
fun interface AnimationEventProvider {
    fun provide(
        oldText: String,
        newText: String,
        oldCursorIndex: UInt,
        newCursorIndex: UInt,
        cause: String,
        maxAnimatedChars: UInt,
        animationDurationMs: ULong
    ): List<EditorAnimationEventData>
}

/**
 * 视觉事务提供者 SAM 接口（唯一主路径）。
 *
 * 自研写作区（SujianEditorView）通过此接口调用 Core 的 editor_visual_transaction API。
 */
fun interface VisualTransactionProvider {
    fun provide(
        oldText: String,
        newText: String,
        oldCursorIndex: UInt,
        newCursorIndex: UInt,
        cause: String,
        maxAnimatedChars: UInt,
        animationDurationMs: ULong
    ): EditorVisualTransactionData?
}
