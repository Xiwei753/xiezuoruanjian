package com.xiwei.sujian.feature.editor.visual.planner

import com.xiwei.sujian.feature.editor.layout.AffectedLayoutRevision
import com.xiwei.sujian.feature.editor.projection.VisualIntent
import com.xiwei.sujian.feature.editor.visual.PreparedVisualTransaction

/**
 * #624 评论3：受影响行规划 — 只消费 [AffectedLayoutRevision]（编辑前后各自抓取的
 * 受影响段落），不再遍历整章 `lineRanges`、不再建整章段落表、不再做
 * old paragraphs × new paragraphs 嵌套匹配。
 *
 * - 编辑前：布局引擎已通过 `getLineForOffset()` 找到编辑所在段落，只保存该段落
 *   及删除/合段时相邻段落的 old line geometry（[AffectedLayoutRevision.affectedLines]）；
 * - 编辑后：只读取新的受影响段落 line geometry；
 * - 完全没改内容、只是整体 Y 偏移的后缀正文：由 [AffectedLayoutRevision.stableSuffixAnchor]
 *   保存锚点 + deltaY，这里生成一个覆盖后缀的 block shift，不逐段枚举；
 * - `OffsetMap` 只负责 affected slice 的 old/new 文本身份（[buildOffsetMapper] 等），
 *   不再用于确认“后面都没变”而扫描整章。
 */
class AffectedLayoutPlanner {
    private companion object {
        const val BLOCK_SHIFT_DELTA_Y_EPSILON = 0.5f
    }

    data class AffectedLinesResult(
        val lineIndices: Set<Int>,
        val oldLineIndices: Set<Int>,
        val newLineIndices: Set<Int>,
        val blockShifts: List<PreparedVisualTransaction.BlockShift>,
    )

    /**
     * 编辑前（[newRevision] == null）只返回 old 侧受影响行；编辑后返回
     * old/new 两侧受影响行 + 稳定后缀 block shift。
     */
    fun computeAffectedLineIndicesFromBothRevisions(
        visualIntent: VisualIntent,
        oldRevision: AffectedLayoutRevision?,
        newRevision: AffectedLayoutRevision?,
    ): AffectedLinesResult {
        if (oldRevision == null && newRevision == null) {
            return AffectedLinesResult(emptySet(), emptySet(), emptySet(), emptyList())
        }
        val oldLines = oldRevision?.affectedLineIndexSet() ?: emptySet()
        if (newRevision == null) {
            return AffectedLinesResult(
                lineIndices = emptySet(),
                oldLineIndices = oldLines,
                newLineIndices = emptySet(),
                blockShifts = emptyList(),
            )
        }
        val newLines = newRevision.affectedLineIndexSet()
        return AffectedLinesResult(
            lineIndices = emptySet(),
            oldLineIndices = oldLines,
            newLineIndices = newLines,
            blockShifts = buildSuffixBlockShift(oldRevision, newRevision),
        )
    }

    /**
     * 稳定后缀 block shift：内容未变化、只整体 Y 偏移的后缀正文用一个 block shift
     * 覆盖（[AffectedLayoutRevision.StableSuffixAnchor] 已由布局引擎按
     * `getLineForOffset()` 在编辑前后分别定位）。deltaY 为 0 或锚点不可信时
     * 不生成 shift — 后缀保持静态新布局位置。
     *
     * #639 评论 5419182722：BlockShift 只负责真正没改内容的后缀，不参与当前正在
     * 自动折行/手动拆段的那一段 — 当前段的 visual lines 已由 old/new snapshots
     * 负责（MoveCrossfadePlanner.appendRetainedTransition）。不新增按 lineIndex
     * 最近邻猜 block 的逻辑，保持 StableSuffixAnchor 语义。
     */
    internal fun buildSuffixBlockShift(
        oldRevision: AffectedLayoutRevision?,
        newRevision: AffectedLayoutRevision?,
    ): List<PreparedVisualTransaction.BlockShift> {
        val oldAnchor = oldRevision?.stableSuffixAnchor ?: return emptyList()
        val newAnchor = newRevision?.stableSuffixAnchor ?: return emptyList()
        if (kotlin.math.abs(newAnchor.deltaY) <= BLOCK_SHIFT_DELTA_Y_EPSILON) return emptyList()
        if (newAnchor.lineIndex < 0 || newAnchor.lineIndex >= newRevision.lineCount) return emptyList()
        return listOf(
            PreparedVisualTransaction.BlockShift(
                startLineIndex = newAnchor.lineIndex,
                endLineIndexExclusive = newRevision.lineCount,
                top = newAnchor.top,
                bottom = newAnchor.bottom,
                left = newAnchor.left,
                right = newAnchor.right,
                deltaY = newAnchor.deltaY,
                startUtf8 = newAnchor.startUtf8,
                endUtf8Exclusive = newAnchor.textLengthUtf8,
            ),
        )
    }

    /**
     * #606: Build an old→new offset mapper.
     *
     * If [VisualIntent.offsetMap] is non-null (Core provided an explicit offset map),
     * consume it directly — single source of truth for offset translation semantics.
     * Each [OffsetMapEntry] maps old offsets [oldByteOffset, oldByteOffset+length) to
     * new offsets [newByteOffset, newByteOffset+length) linearly. Offsets not covered
     * by any entry are in the changed region and return null.
     *
     * If [VisualIntent.offsetMap] is null (cursor-only operations, or Core did not
     * provide a map), fall back to identity mapping.
     */
    internal fun buildOffsetMapper(visualIntent: VisualIntent): (Int) -> Int? {
        val offsetMap = visualIntent.offsetMap
        if (offsetMap == null || offsetMap.entries.isEmpty()) {
            return { offset -> offset }
        }
        return { offset: Int ->
            val entry =
                offsetMap.entries.firstOrNull { e ->
                    offset >= e.oldByteOffset && offset < e.oldByteOffset + e.length
                }
            entry?.let { it.newByteOffset + (offset - it.oldByteOffset) }
        }
    }

    /**
     * #606: Build a new→old (reverse) offset mapper.
     *
     * If [VisualIntent.offsetMap] is non-null, consume it directly — traverse entries
     * to find the one containing the new byte offset and map back to old coordinates.
     * If [VisualIntent.offsetMap] is null, fall back to identity mapping.
     */
    internal fun buildReverseOffsetMapper(visualIntent: VisualIntent): (Int) -> Int? {
        val offsetMap = visualIntent.offsetMap
        if (offsetMap == null || offsetMap.entries.isEmpty()) {
            return { newOffset -> newOffset }
        }
        return { newOffset: Int ->
            val entry =
                offsetMap.entries.firstOrNull { e ->
                    newOffset >= e.newByteOffset && newOffset < e.newByteOffset + e.length
                }
            entry?.let { it.oldByteOffset + (newOffset - it.newByteOffset) }
        }
    }

    /**
     * #606: Build a standalone new→old (reverse) offset mapper.
     *
     * Same semantics as [buildReverseOffsetMapper] but does not require layout revisions.
     */
    internal fun buildStandaloneReverseOffsetMapper(visualIntent: VisualIntent): (Int) -> Int? {
        val offsetMap = visualIntent.offsetMap
        if (offsetMap == null || offsetMap.entries.isEmpty()) {
            return { newOffset -> newOffset }
        }
        return { newOffset: Int ->
            val entry =
                offsetMap.entries.firstOrNull { e ->
                    newOffset >= e.newByteOffset && newOffset < e.newByteOffset + e.length
                }
            entry?.let { it.oldByteOffset + (newOffset - it.newByteOffset) }
        }
    }
}
