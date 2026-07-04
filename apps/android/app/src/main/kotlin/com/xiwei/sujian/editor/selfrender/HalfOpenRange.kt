package com.xiwei.sujian.editor.selfrender

/**
 * 半开区间 [start, end)，替代 Kotlin IntRange 的 [start, end] 闭区间语义。
 *
 * IntRange.last 是包含的，这导致：
 * - IntRange(1, 3) 表示 [1, 2, 3]（3 个元素），不是 [1, 3)（2 个元素）
 * - 对于 UTF-16 offset range，我们需要半开区间 [start, end)
 *
 * 所有动画范围（activeInsertRange、reflowRange、clusterRange、snapshotRange）
 * 必须使用 HalfOpenRange，禁止用 IntRange。
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
