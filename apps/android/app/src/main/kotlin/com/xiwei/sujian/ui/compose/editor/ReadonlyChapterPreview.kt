package com.xiwei.sujian.ui.compose.editor

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.graphics.nativeCanvas
import com.xiwei.sujian.editor.v2.projection.TargetDisplayRuntime
import kotlinx.coroutines.delay

@Composable
fun ReadonlyChapterPreview(
    projection: TargetDisplayRuntime,
    modifier: Modifier = Modifier
) {
    var frameVersion by remember { mutableLongStateOf(0L) }

    LaunchedEffect(projection) {
        while (true) {
            if (projection.hasActiveAnimation()) {
                frameVersion++
                delay(16)
            } else {
                delay(100)
            }
        }
    }

    Canvas(modifier = modifier.fillMaxSize()) {
        drawIntoCanvas { canvas ->
            val nativeCanvas = canvas.nativeCanvas
            nativeCanvas.save()
            projection.drawFrame(nativeCanvas)
            nativeCanvas.restore()
        }
    }
}
