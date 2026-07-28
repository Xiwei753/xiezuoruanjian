package com.xiwei.sujian.ui.compose.workbench.component

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
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
            Surface(
                modifier = Modifier
                    .offset(x = panel.floatingX.dp, y = panel.floatingY.dp)
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
                ) {
                    panelContent(panel)
                }
            }
        }
    }
}
