package com.xiwei.sujian.editor.v2.layout

import android.graphics.Bitmap

class AndroidLineSnapshot(
    val snapshotId: Long,
    val bitmap: Bitmap?,
    val lineIndex: Int,
    val sourceRect: android.graphics.Rect,
    val destinationRect: android.graphics.RectF
)
