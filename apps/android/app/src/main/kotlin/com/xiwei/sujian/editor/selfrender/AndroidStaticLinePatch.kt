package com.xiwei.sujian.editor.selfrender

import android.graphics.RectF

data class AndroidStaticLinePatch(
    val newSnapshotId: AndroidLineSnapshotId?,
    val destinationDocumentRect: RectF,
    val visibleSourceRects: List<RectF>
)
