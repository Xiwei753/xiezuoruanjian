package com.xiwei.sujian.editor.v2.layout

import android.graphics.Bitmap

data class LineClusterSnapshot(
    val clusterId: Long,
    val documentByteStart: Int,
    val documentByteEndExclusive: Int,
    val documentUtf16Start: Int,
    val documentUtf16EndExclusive: Int,
    val sourceRectInLineImage: android.graphics.Rect,
    val visualRectInDocument: android.graphics.RectF,
    val shapingFingerprint: String,
    val shapingIdentityConfident: Boolean = true
)

data class AndroidLineSnapshot(
    val snapshotId: Long,
    val bitmap: Bitmap?,
    val lineIndex: Int,
    val sourceRect: android.graphics.Rect,
    val destinationRect: android.graphics.RectF,
    val clusters: List<LineClusterSnapshot> = emptyList(),
    val documentByteStart: Int = 0,
    val documentByteEndExclusive: Int = 0,
    val documentUtf16Start: Int = 0,
    val documentUtf16EndExclusive: Int = 0,
    val baseline: Float = 0f,
    val lineHeight: Float = 0f
)
