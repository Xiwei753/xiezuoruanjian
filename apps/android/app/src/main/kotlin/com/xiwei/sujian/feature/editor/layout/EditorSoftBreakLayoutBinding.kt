package com.xiwei.sujian.feature.editor.layout

/**
 * Issue #717 评论 5743443030 修复1：OutputTransformation 与 onTextLayout 的同版本绑定。
 *
 * OutputTransformation 每次执行时记录 {rawText, projection, displayText}；
 * onTextLayout 拿到 result.layoutInput.text.text（即 displayText）后，
 * 从 holder 里按 displayText 内容精确匹配对应那次 transformation 的绑定，
 * 不再靠"长度猜版本"或"把 display 文本反解成 raw"。
 *
 * 同长度不同内容（如 "abcdef" vs "abcxef"）不会误匹配，因为比较的是 displayText 全文内容。
 * 环形缓冲区容量有限，极端快输入下旧绑定被淘汰时 fallback 到 live state。
 */
class EditorSoftBreakLayoutBinding {
    /** 精确匹配到的绑定结果。 */
    data class Match(
        val rawText: String,
        val projection: EditorSoftBreakProjection,
    )

    private data class Binding(
        val rawText: String,
        val projection: EditorSoftBreakProjection,
        val displayText: String,
    )

    private val bindings = ArrayDeque<Binding>()
    private var versionCounter = 0L

    /** 记录一次 OutputTransformation 的绑定。返回版本号。 */
    fun record(
        rawText: String,
        projection: EditorSoftBreakProjection,
        displayText: String,
    ): Long {
        versionCounter += 1
        bindings.addLast(Binding(rawText, projection, displayText))
        while (bindings.size > CAPACITY) bindings.removeFirst()
        return versionCounter
    }

    /** 按 displayText 内容精确匹配最近一次绑定。从最新往最旧找。 */
    fun findForDisplayText(displayText: String): Match? {
        for (i in bindings.size - 1 downTo 0) {
            val b = bindings[i]
            if (b.displayText == displayText) {
                return Match(b.rawText, b.projection)
            }
        }
        return null
    }

    companion object {
        private const val CAPACITY = 16
    }
}
