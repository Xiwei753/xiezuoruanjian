package com.xiwei.sujian.ui.compose.workbench.component

import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.width
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.ui.compose.workbench.model.DockZone
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelId

@Composable
fun DockResizeHandle(
    zone: DockZone,
    panelId: WorkbenchPanelId,
    currentSizeDp: Float,
    onResize: (panelId: WorkbenchPanelId, newSizeDp: Float) -> Unit,
    modifier: Modifier = Modifier,
) {
    val handleColor = MaterialTheme.colorScheme.outlineVariant
    val density = androidx.compose.ui.platform.LocalDensity.current
    var accumulatedDp by remember { mutableFloatStateOf(0f) }

    val handleModifier = when (zone) {
        DockZone.Left -> modifier.width(4.dp).fillMaxHeight()
        DockZone.Right -> modifier.width(4.dp).fillMaxHeight()
        DockZone.Bottom -> modifier.height(4.dp).fillMaxWidth()
        DockZone.Floating -> modifier.width(4.dp).height(4.dp)
    }

    Box(
        modifier = handleModifier
            .background(handleColor)
            .pointerInput(zone, panelId) {
                detectDragGestures(
                    onDragEnd = { accumulatedDp = 0f },
                    onDragCancel = { accumulatedDp = 0f },
                ) { change, dragAmount ->
                    val deltaDp = when (zone) {
                        DockZone.Left -> dragAmount.x / density.density
                        DockZone.Right -> -dragAmount.x / density.density
                        DockZone.Bottom -> -dragAmount.y / density.density
                        DockZone.Floating -> dragAmount.x / density.density
                    }
                    accumulatedDp += deltaDp
                    val newSize = (currentSizeDp + accumulatedDp).coerceAtLeast(0f)
                    onResize(panelId, newSize)
                    change.consume()
                }
            }
    )
}
