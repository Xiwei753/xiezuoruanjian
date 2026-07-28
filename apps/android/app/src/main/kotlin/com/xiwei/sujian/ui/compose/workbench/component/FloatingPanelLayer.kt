package com.xiwei.sujian.ui.compose.workbench.component

import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.dp
import androidx.compose.ui.zIndex
import com.xiwei.sujian.ui.compose.workbench.model.DockZone
import com.xiwei.sujian.ui.compose.workbench.model.PanelVisibility
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelId
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelState

@Composable
fun FloatingPanelLayer(
    panels: List<WorkbenchPanelState>,
    onFloat: (WorkbenchPanelId) -> Unit,
    onCollapse: (WorkbenchPanelId) -> Unit,
    onHide: (WorkbenchPanelId) -> Unit,
    onMoveFloating: (WorkbenchPanelId, Float, Float) -> Unit,
    modifier: Modifier = Modifier,
    panelContent: @Composable (WorkbenchPanelState) -> Unit,
) {
    val floatingPanels = panels.filter { it.zone == DockZone.Floating && it.visibility == PanelVisibility.Expanded }
    if (floatingPanels.isEmpty()) return

    Box(modifier = modifier.fillMaxSize()) {
        for ((index, panel) in floatingPanels.withIndex()) {
            val density = LocalDensity.current
            var dragOffsetX by remember(panel.id) { mutableFloatStateOf(0f) }
            var dragOffsetY by remember(panel.id) { mutableFloatStateOf(0f) }

            Surface(
                modifier = Modifier
                    .offset(x = (panel.floatingX + dragOffsetX).dp, y = (panel.floatingY + dragOffsetY).dp)
                    .size(width = panel.floatingWidthDp.dp, height = panel.floatingHeightDp.dp)
                    .zIndex(index.toFloat()),
                tonalElevation = 4.dp,
                shadowElevation = 8.dp,
                shape = androidx.compose.material3.MaterialTheme.shapes.large,
                color = androidx.compose.material3.MaterialTheme.colorScheme.surface,
            ) {
                WorkbenchPanelFrame(
                    panelState = panel,
                    onFloat = { onFloat(panel.id) },
                    onCollapse = { onCollapse(panel.id) },
                    onClose = { onHide(panel.id) },
                    modifier = Modifier.fillMaxSize(),
                    titleBarModifier = Modifier
                        .pointerInput(panel.id) {
                            detectDragGestures(
                                onDragEnd = {
                                    val newX = panel.floatingX + dragOffsetX
                                    val newY = panel.floatingY + dragOffsetY
                                    onMoveFloating(panel.id, newX, newY)
                                    dragOffsetX = 0f
                                    dragOffsetY = 0f
                                },
                                onDragCancel = {
                                    dragOffsetX = 0f
                                    dragOffsetY = 0f
                                },
                            ) { change, dragAmount ->
                                dragOffsetX += dragAmount.x / density.density
                                dragOffsetY += dragAmount.y / density.density
                                change.consume()
                            }
                        },
                ) {
                    panelContent(panel)
                }
            }
        }
    }
}
