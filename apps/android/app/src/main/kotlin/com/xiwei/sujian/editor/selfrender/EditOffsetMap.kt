package com.xiwei.sujian.editor.selfrender

/**
 * 编辑前后 UTF-8 byte range 映射。
 *
 * 从 old/new 文本差异构建 [unchangedSegments]、[insertedRanges]、[deletedRanges]，
 * 支持将新文本的 byte range 映射回旧文本的 byte range。
 *
 * 用途：动画事务中，新快照的 cluster 需要找到旧快照中对应的 cluster，
 * 但插入/删除后 byte offset 会偏移，不能直接按相同 byte range 匹配。
 * OffsetMap 提供精确的 new→old 映射，消除对 visualLineOrdinal 或
 * 平均字符宽度的依赖。
 */
class EditOffsetMap private constructor(
    private val unchangedSegments: List<ByteSegment>,
    private val insertedRanges: List<ByteSegment>,
    private val deletedRanges: List<ByteSegment>
) {
    data class ByteSegment(
        val oldStart: Int,
        val oldEnd: Int,
        val newStart: Int,
        val newEnd: Int
    )

    /**
     * 将新文本的 byte range 映射到旧文本的 byte range。
     *
     * 如果新 range 完全落在某个 unchanged segment 内，返回旧文本中对应的 range。
     * 如果新 range 与 inserted range 重叠，返回 null（该部分是新增内容，无旧对应）。
     * 如果新 range 跨越多个 segment，返回 null（无法精确映射）。
     */
    fun mapNewRangeToOld(newByteStart: Int, newByteEnd: Int): OldRange? {
        if (newByteStart >= newByteEnd) return null

        for (seg in unchangedSegments) {
            if (newByteStart >= seg.newStart && newByteEnd <= seg.newEnd) {
                val offsetInNew = newByteStart - seg.newStart
                val length = newByteEnd - newByteStart
                return OldRange(seg.oldStart + offsetInNew, seg.oldStart + offsetInNew + length)
            }
        }
        return null
    }

    /**
     * 判断新文本的 byte offset 是否落在 inserted range 内。
     */
    fun isNewRangeInserted(newByteStart: Int, newByteEnd: Int): Boolean {
        for (ins in insertedRanges) {
            if (newByteStart >= ins.newStart && newByteEnd <= ins.newEnd) {
                return true
            }
        }
        return false
    }

    /**
     * 将旧文本的 byte range 映射到新文本的 byte range。
     *
     * 如果旧 range 完全落在某个 unchanged segment 内，返回新文本中对应的 range。
     * 如果旧 range 与 deleted range 重叠，返回 null（该部分已被删除，无新对应）。
     */
    fun mapOldRangeToNew(oldByteStart: Int, oldByteEnd: Int): NewRange? {
        if (oldByteStart >= oldByteEnd) return null

        for (seg in unchangedSegments) {
            if (oldByteStart >= seg.oldStart && oldByteEnd <= seg.oldEnd) {
                val offsetInOld = oldByteStart - seg.oldStart
                val length = oldByteEnd - oldByteStart
                return NewRange(seg.newStart + offsetInOld, seg.newStart + offsetInOld + length)
            }
        }
        return null
    }

    /**
     * 判断旧文本的 byte offset 是否落在 deleted range 内。
     */
    fun isOldRangeDeleted(oldByteStart: Int, oldByteEnd: Int): Boolean {
        for (del in deletedRanges) {
            if (oldByteStart >= del.oldStart && oldByteEnd <= del.oldEnd) {
                return true
            }
        }
        return false
    }

    data class OldRange(val start: Int, val end: Int)
    data class NewRange(val start: Int, val end: Int)

    companion object {
        /**
         * 从 old/new 文本和编辑范围构建 OffsetMap。
         *
         * @param oldText 编辑前文本
         * @param newText 编辑后文本
         * @param insertedRangeStart 插入范围起始（UTF-8 byte offset，基于 newText）
         * @param insertedRangeEnd 插入范围结束（UTF-8 byte offset，基于 newText）
         * @param isDelete 是否为删除操作
         * @param deletedRangeStart 删除范围起始（UTF-8 byte offset，基于 oldText），仅删除时有效
         * @param deletedRangeEnd 删除范围结束（UTF-8 byte offset，基于 oldText），仅删除时有效
         */
        fun fromEdit(
            oldText: String,
            newText: String,
            insertedRangeStart: Int,
            insertedRangeEnd: Int,
            isDelete: Boolean,
            deletedRangeStart: Int = 0,
            deletedRangeEnd: Int = 0
        ): EditOffsetMap {
            val unchangedSegments = mutableListOf<ByteSegment>()
            val insertedRanges = mutableListOf<ByteSegment>()
            val deletedRanges = mutableListOf<ByteSegment>()

            if (isDelete) {
                val oldLen = SujianEditorBuffer.utf16ToUtf8(oldText, oldText.length)
                val newLen = SujianEditorBuffer.utf16ToUtf8(newText, newText.length)

                if (deletedRangeStart > 0) {
                    unchangedSegments.add(ByteSegment(0, deletedRangeStart, 0, deletedRangeStart))
                }
                deletedRanges.add(ByteSegment(deletedRangeStart, deletedRangeEnd, deletedRangeStart, deletedRangeStart))
                if (deletedRangeEnd < oldLen) {
                    val newSuffixStart = deletedRangeStart
                    unchangedSegments.add(ByteSegment(deletedRangeEnd, oldLen, newSuffixStart, newLen))
                }
            } else {
                val oldLen = SujianEditorBuffer.utf16ToUtf8(oldText, oldText.length)
                val newLen = SujianEditorBuffer.utf16ToUtf8(newText, newText.length)

                if (insertedRangeStart > 0) {
                    val prefixOldEnd = minOf(insertedRangeStart, oldLen)
                    unchangedSegments.add(ByteSegment(0, prefixOldEnd, 0, insertedRangeStart))
                }
                insertedRanges.add(ByteSegment(insertedRangeStart, insertedRangeStart, insertedRangeStart, insertedRangeEnd))
                if (insertedRangeEnd < newLen) {
                    val suffixNewStart = insertedRangeEnd
                    val suffixOldStart = insertedRangeStart
                    val suffixOldEnd = oldLen
                    if (suffixOldEnd > suffixOldStart) {
                        unchangedSegments.add(ByteSegment(suffixOldStart, suffixOldEnd, suffixNewStart, newLen))
                    }
                }
            }

            return EditOffsetMap(unchangedSegments, insertedRanges, deletedRanges)
        }
    }
}
