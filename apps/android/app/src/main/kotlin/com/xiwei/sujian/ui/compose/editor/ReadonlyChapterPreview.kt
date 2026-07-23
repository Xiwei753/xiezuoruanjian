package com.xiwei.sujian.ui.compose.editor

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.graphics.nativeCanvas
import com.xiwei.sujian.editor.v2.projection.TargetDisplayRuntime

@Composable
fun ReadonlyChapterPreview(
    projection: TargetDisplayRuntime,
    modifier: Modifier = Modifier
) {
    DisposableEffect(projection) {
        projection.startFrameClock()
        onDispose {
            projection.stopFrameClock()
        }
    }

    Box(
        modifier = modifier
            .fillMaxSize()
            .drawBehind {
                @Suppress("UNUSED_VARIABLE")
                val gen = projection.frameGeneration
                val nativeCanvas = drawContext.canvas.nativeCanvas
                nativeCanvas.save()
                projection.drawFrame(nativeCanvas)
                nativeCanvas.restore()
            }
    )
}
