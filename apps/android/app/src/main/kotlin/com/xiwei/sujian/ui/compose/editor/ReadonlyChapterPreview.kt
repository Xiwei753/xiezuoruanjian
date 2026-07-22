package com.xiwei.sujian.ui.compose.editor

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.graphics.nativeCanvas
import com.xiwei.sujian.editor.v2.projection.TargetReadonlyProjection
import com.xiwei.sujian.editor.v2.render.AndroidTextRenderer

@Composable
fun ReadonlyChapterPreview(
    projection: TargetReadonlyProjection,
    modifier: Modifier = Modifier
) {
    val layoutEngine = projection.getLayoutEngine()
    val layout = layoutEngine.getLayout()
    val renderer = remember { AndroidTextRenderer() }

    Canvas(modifier = modifier.fillMaxSize()) {
        if (layout != null) {
            drawIntoCanvas { canvas ->
                val nativeCanvas = canvas.nativeCanvas
                nativeCanvas.save()
                renderer.drawBackground(nativeCanvas)
                val highlightsUtf16 = projection.getSearchHighlightsUtf16()
                if (highlightsUtf16.isNotEmpty()) {
                    renderer.drawSearchHighlights(nativeCanvas, layout, highlightsUtf16)
                }
                val selStart = projection.getSelectionStartUtf16()
                val selEnd = projection.getSelectionEndUtf16()
                if (selStart >= 0 && selEnd >= 0 && selStart != selEnd) {
                    renderer.drawSelectionHighlight(nativeCanvas, layout, selStart, selEnd)
                }
                renderer.drawStaticText(nativeCanvas, layout)
                nativeCanvas.restore()
            }
        }
    }
}
