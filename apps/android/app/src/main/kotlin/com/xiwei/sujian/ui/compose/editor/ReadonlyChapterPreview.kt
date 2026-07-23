package com.xiwei.sujian.ui.compose.editor

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.graphics.nativeCanvas
import com.xiwei.sujian.editor.v2.projection.TargetDisplayRuntime

@Composable
fun ReadonlyChapterPreview(
    projection: TargetDisplayRuntime,
    modifier: Modifier = Modifier
) {
    val frameGeneration = projection.frameGeneration

    DisposableEffect(projection) {
        projection.startFrameClock()
        onDispose {
            projection.stopFrameClock()
        }
    }

    Canvas(modifier = modifier.fillMaxSize()) {
        @Suppress("UNUSED_VARIABLE")
        val gen = frameGeneration
        drawIntoCanvas { canvas ->
            val nativeCanvas = canvas.nativeCanvas
            nativeCanvas.save()
            projection.drawFrame(nativeCanvas)
            nativeCanvas.restore()
        }
    }
}
