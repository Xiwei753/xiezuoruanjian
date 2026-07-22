package com.xiwei.sujian.ui.compose.editor

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.graphics.nativeCanvas
import com.xiwei.sujian.editor.v2.projection.TargetDisplayRuntime

@Composable
fun ReadonlyChapterPreview(
    projection: TargetDisplayRuntime,
    modifier: Modifier = Modifier
) {
    Canvas(modifier = modifier.fillMaxSize()) {
        drawIntoCanvas { canvas ->
            val nativeCanvas = canvas.nativeCanvas
            nativeCanvas.save()
            projection.drawFrame(nativeCanvas)
            nativeCanvas.restore()
        }
    }
}
