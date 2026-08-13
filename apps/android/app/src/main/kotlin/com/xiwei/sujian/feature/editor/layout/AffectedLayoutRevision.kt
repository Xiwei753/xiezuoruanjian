package com.xiwei.sujian.feature.editor.layout

/**
 * #624 评论3：普通编辑路径的“受影响区域”不可变视觉快照。
 *
 * 与全量 [AndroidLayoutRevision] 的关系：普通按键不再复制整章 `lineRanges`；
 * 编辑前通过当前 DynamicLayout 的 `getLineForOffset()` 找到编辑所在段落，
 * 只保存该段落（以及删除/合段时的相邻段落）的 old line geometry；编辑后只读取
 * 新的受影响段落 line geometry。后续完全没改内容、只是整体 Y 偏移的正文不逐段
 * 枚举，由 [stableSuffixAnchor] 保存锚点 + deltaY，动画层据此生成一个 block shift。
 *
 * [lineRangeAt] 以**绝对行号**查询（与 Layout 行号、Bitmap snapshot 键一致），
 * 越界返回 null — 与全量 revision 的 `lineRanges.getOrNull(i)` 语义等价。
 *
 * [firstAffectedLineIndex] 到 `firstAffectedLineIndex + affectedLines.size` 之外的行
 * 不在本快照内；查询方不得假设快照覆盖整章。
 */
data class AffectedLayoutRevision(
    /** Rust EditorKernel 的编辑 revision（capture 时刻 mirror 的状态）。 */
    val editorRevision: Long,
    /** 本引擎的局部布局推进计数（每次 capture 递增）。 */
    val layoutRevision: Long,
    /** capture 时刻的布局配置 fingerprint — 与 [AndroidLayoutEngine] 的
     *  `computeConfigFingerprint()` 一致；渲染路径据此判断快照是否过期。 */
    val layoutConfigFingerprint: String,
    /** 受影响视觉行的起始绝对行号。 */
    val firstAffectedLineIndex: Int,
    /** 受影响视觉行（绝对行号 = firstAffectedLineIndex + index）。 */
    val affectedLines: List<AndroidLayoutRevision.LineRange>,
    /** 当前 layout 的总行数（O(1) 读取，用于 block shift 终点等）。 */
    override val lineCount: Int,
    // ── cursor / selection / composition 几何（与全量 revision 同源） ──
    override val cursorUtf8: Int,
    override val cursorUtf16: Int,
    override val cursorX: Float,
    override val cursorY: Float,
    override val cursorHeight: Float,
    override val selectionAnchorUtf8: Int,
    override val selectionHeadUtf8: Int,
    override val selectionAnchorUtf16: Int,
    override val selectionHeadUtf16: Int,
    override val compositionStartUtf16: Int,
    override val compositionEndUtf16: Int,
    /**
     * 稳定后缀锚点：受影响区域之后第一个内容未变化的段落起点（本侧坐标）。
     * 只做整体 Y 偏移的后缀正文不再逐段枚举，动画层用一个 block shift 覆盖。
     * old 侧捕获时 [StableSuffixAnchor.deltaY] 恒为 0；new 侧捕获时由引擎
     * 用 old 锚点与新锚点的 top 差计算真实 deltaY（两侧锚点不对应时置 null）。
     */
    val stableSuffixAnchor: StableSuffixAnchor?,
) : LayoutRevisionSource {
    /** 本快照覆盖的总行数（受影响的视觉行数）。 */
    val affectedLineCount: Int get() = affectedLines.size

    /** 绝对行号 → LineRange；行号在本快照范围外返回 null。 */
    override fun lineRangeAt(lineIndex: Int): AndroidLayoutRevision.LineRange? {
        if (lineIndex !in firstAffectedLineIndex until firstAffectedLineIndex + affectedLines.size) {
            return null
        }
        return affectedLines[lineIndex - firstAffectedLineIndex]
    }

    /** 本快照提供的全部（绝对行号, LineRange）对 — 供按行迭代的 planner 使用。 */
    override fun lineEntries(): List<Pair<Int, AndroidLayoutRevision.LineRange>> {
        return affectedLines.mapIndexed { i, range -> Pair(firstAffectedLineIndex + i, range) }
    }

    /** 受影响视觉行的绝对行号集合。 */
    fun affectedLineIndexSet(): Set<Int> {
        if (affectedLines.isEmpty()) return emptySet()
        return (firstAffectedLineIndex until firstAffectedLineIndex + affectedLines.size).toSet()
    }

    data class StableSuffixAnchor(
        /** 稳定后缀起点（本侧 UTF-8 字节偏移）。 */
        val startUtf8: Int,
        /** 稳定后缀起点（本侧 UTF-16 偏移）。 */
        val startUtf16: Int,
        /** 后缀首行在本侧 layout 中的绝对行号。 */
        val lineIndex: Int,
        /** 后缀首行 top。 */
        val top: Float,
        /** 后缀首行 bottom。 */
        val bottom: Float,
        /** 后缀首行 left。 */
        val left: Float,
        /** 后缀首行 right。 */
        val right: Float,
        /** 本侧正文总 UTF-8 字节长。 */
        val textLengthUtf8: Int,
        /**
         * new 侧：newTop - oldTop（后缀整体 Y 偏移）；old 侧恒 0。
         * 两侧锚点内容不对应时引擎不会生成 new 侧锚点。
         */
        val deltaY: Float = 0f,
    )
}
