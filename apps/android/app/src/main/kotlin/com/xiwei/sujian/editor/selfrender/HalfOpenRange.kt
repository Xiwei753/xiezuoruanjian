package com.xiwei.sujian.editor.selfrender

/**
 * 半开区间 [start, end)，替代 Kotlin IntRange 的 [start, end] 闭区间语义。
 *
 * IntRange.last 是包含的，这导致：
 * - IntRange(1, 3) 表示 [1, 2, 3]（3 个元素），不是 [1, 3)（2 个元素）
 * - 对于 offset range，我们需要半开区间 [start, end)
 *
 * 用于行号范围等无单位坐标。UTF-16 和 UTF-8 byte 偏移必须使用
 * [Utf16Range] 和 [ByteRange]，禁止与 HalfOpenRange 互换。
 */
data class HalfOpenRange(val start: Int, val end: Int) : Comparable<HalfOpenRange> {
    init {
        require(start <= end) { "HalfOpenRange start=$start must be <= end=$end" }
    }

    val length: Int get() = end - start
    val isEmpty: Boolean get() = start == end

    fun contains(offset: Int): Boolean = offset in start until end

    fun overlaps(other: HalfOpenRange): Boolean = !(end <= other.start || start >= other.end)

    override fun compareTo(other: HalfOpenRange): Int {
        val cmp = start.compareTo(other.start)
        return if (cmp != 0) cmp else end.compareTo(other.end)
    }

    companion object {
        val EMPTY = HalfOpenRange(0, 0)
    }
}

/**
 * UTF-16 code unit 半开区间 [start, endExclusive)。
 *
 * 用于 Android CharSequence / Layout offset、compositionReplaceRange、
 * preeditRangeInVirtualText、decorationRanges 等。
 *
 * 禁止与 [ByteRange] 或 [HalfOpenRange] 互换：编译器必须捕获混用。
 */
data class Utf16Range(val start: Int, val endExclusive: Int) : Comparable<Utf16Range> {
    init {
        require(start <= endExclusive) { "Utf16Range start=$start must be <= endExclusive=$endExclusive" }
    }

    val length: Int get() = endExclusive - start
    val isEmpty: Boolean get() = start == endExclusive

    fun contains(offset: Int): Boolean = offset in start until endExclusive

    fun overlaps(other: Utf16Range): Boolean = !(endExclusive <= other.start || start >= other.endExclusive)

    fun toHalfOpenRange(): HalfOpenRange = HalfOpenRange(start, endExclusive)

    override fun compareTo(other: Utf16Range): Int {
        val cmp = start.compareTo(other.start)
        return if (cmp != 0) cmp else endExclusive.compareTo(other.endExclusive)
    }

    companion object {
        val EMPTY = Utf16Range(0, 0)
    }
}

/**
 * UTF-8 byte offset 半开区间 [start, endExclusive)。
 *
 * 用于 EditOffsetMap 映射结果、documentByteStart/End 范围、
 * OffsetMap unchangedSegments 等。
 *
 * 禁止与 [Utf16Range] 或 [HalfOpenRange] 互换：编译器必须捕获混用。
 */
data class ByteRange(val start: Int, val endExclusive: Int) : Comparable<ByteRange> {
    init {
        require(start <= endExclusive) { "ByteRange start=$start must be <= endExclusive=$endExclusive" }
    }

    val length: Int get() = endExclusive - start
    val isEmpty: Boolean get() = start == endExclusive

    fun contains(offset: Int): Boolean = offset in start until endExclusive

    fun overlaps(other: ByteRange): Boolean = !(endExclusive <= other.start || start >= other.endExclusive)

    fun toHalfOpenRange(): HalfOpenRange = HalfOpenRange(start, endExclusive)

    override fun compareTo(other: ByteRange): Int {
        val cmp = start.compareTo(other.start)
        return if (cmp != 0) cmp else endExclusive.compareTo(other.endExclusive)
    }

    companion object {
        val EMPTY = ByteRange(0, 0)
    }
}
