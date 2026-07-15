package com.xiwei.sujian.editor.selfrender

import android.graphics.RectF

/**
 * 行快照唯一标识，由 layout revision 和 visual line ordinal 组成。
 */
data class AndroidLineSnapshotId(
    val revision: Long,
    val visualLineOrdinal: Int
)

/**
 * 一次平台排版后的不可变 glyph cluster 视觉快照。
 *
 * 坐标空间：
 * - [documentByteStart]/[documentByteEnd]：跨平台 UTF-8 文档坐标。
 * - [platformTextStart]/[platformTextEnd]：Android `CharSequence` 的 UTF-16 offset。
 * - [sourceRectInLineSnapshot]：行视觉资源局部坐标。
 * - [visualRectInDocument]：文档坐标。
 * - [shapingIdentity]：用于判断 old/new cluster 是否可复用同一视觉资源（Move vs Crossfade）。
 */
data class AndroidClusterSnapshot(
    val documentByteStart: Int,
    val documentByteEnd: Int,
    val platformTextStart: Int,
    val platformTextEnd: Int,
    val sourceRectInLineSnapshot: RectF,
    val visualRectInDocument: RectF,
    val textDirection: Int,
    val shapingIdentity: String
)

/**
 * 一次平台排版后的不可变行视觉快照。
 *
 * [visualResource] 在事务结束前由 snapshot 持有，[release] 必须可重复调用且
 * 不能提前释放仍被 slice 引用的资源。
 */
data class AndroidLineSnapshot(
    val id: AndroidLineSnapshotId,
    val revision: Long,
    val paragraphId: Int,
    val visualLineOrdinal: Int,
    val documentByteStart: Int,
    val documentByteEnd: Int,
    val platformTextStart: Int,
    val platformTextEnd: Int,
    val documentRect: RectF,
    val baseline: Float,
    val lineImageLocalSize: RectF,
    val clusters: List<AndroidClusterSnapshot>,
    val visualResource: AndroidLineVisualResource?
) {
    private var released = false

    fun release() {
        check(!released) { "Double release of AndroidLineSnapshot ${id.revision}/${id.visualLineOrdinal}" }
        released = true
        visualResource?.release()
    }

    fun isReleased(): Boolean = released
}
