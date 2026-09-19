package com.xiwei.sujian.feature.editor.layout

import androidx.compose.ui.text.TextRange

/**
 * Issue #717 评论 5743443030 修复1：OutputTransformation 与 onTextLayout 的同版本绑定。
 *
 * OutputTransformation 每次执行时记录 {rawText, projection, displayText}；
 * onTextLayout 拿到 result.layoutInput.text.text（即 displayText）后，
 * 从 holder 里按 displayText 内容精确匹配对应那次 transformation 的绑定，
 * 不再靠"长度猜版本"或"把 display 文本反解成 raw"。
 *
 * 同长度不同内容（如 "abcdef" vs "abcxef"）不会误匹配，因为比较的是 displayText 全文内容。
 * 环形缓冲区容量有限，极端快输入下旧绑定被淘汰时由调用方决定 fallback 策略。
 *
 * Issue #717 评论 5743745219：记录的是完整同版本输入快照
 * {rawText, projection, displayText, rawSelection, compositionActive}。
 * rawSelection 是 OutputTransformation 执行前的原始 selection（TextFieldBuffer.originalSelection，
 * androidx.compose.ui.text.TextRange），compositionActive 是当时 TextFieldState 是否处于 composing 状态。
 * 这样 onTextLayout 拿到匹配的 Match 时，selection / compositionActive 与 rawText / projection / displayText
 * 严格来自同一次 buffer 变化，不会出现"layout A 配 state B 的 selection"。
 */
class EditorSoftBreakLayoutBinding {
    /**
     * 精确匹配到的绑定结果。
     *
     * [rawSelection] / [compositionActive] 与 [rawText] / [projection] 严格同版本，
     * 来自同一次 OutputTransformation 执行时的 TextFieldBuffer 快照。
     */
    data class Match(
        val rawText: String,
        val projection: EditorSoftBreakProjection,
        val rawSelection: TextRange,
        val compositionActive: Boolean,
    )

    private data class Binding(
        val rawText: String,
        val projection: EditorSoftBreakProjection,
        val displayText: String,
        val rawSelection: TextRange,
        val compositionActive: Boolean,
    )

    private val bindings = ArrayDeque<Binding>()
    private var versionCounter = 0L

    /**
     * 记录一次 OutputTransformation 的绑定。返回版本号。
     *
     * [rawSelection] 这次 buffer 变化前的原始 selection（TextFieldBuffer.originalSelection）。
     * [compositionActive] OutputTransformation 执行时 TextFieldState 是否处于 composing 状态。
     */
    fun record(
        rawText: String,
        projection: EditorSoftBreakProjection,
        displayText: String,
        rawSelection: TextRange,
        compositionActive: Boolean,
    ): Long {
        versionCounter += 1
        bindings.addLast(
            Binding(
                rawText = rawText,
                projection = projection,
                displayText = displayText,
                rawSelection = rawSelection,
                compositionActive = compositionActive,
            ),
        )
        while (bindings.size > CAPACITY) bindings.removeFirst()
        return versionCounter
    }

    /** 按 displayText 内容精确匹配最近一次绑定。从最新往最旧找。 */
    fun findForDisplayText(displayText: String): Match? {
        for (i in bindings.size - 1 downTo 0) {
            val b = bindings[i]
            if (b.displayText == displayText) {
                return Match(b.rawText, b.projection, b.rawSelection, b.compositionActive)
            }
        }
        return null
    }

    companion object {
        private const val CAPACITY = 16
    }
}
