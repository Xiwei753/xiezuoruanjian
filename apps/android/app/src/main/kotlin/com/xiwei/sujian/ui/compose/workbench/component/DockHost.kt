package com.xiwei.sujian.ui.compose.workbench.component

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.designsystem.theme.LocalSujianDimensions
import com.xiwei.sujian.ui.compose.workbench.model.DockZone
import com.xiwei.sujian.ui.compose.workbench.model.PanelVisibility
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelId
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelState

@Composable
fun DockHost(
    zone: DockZone,
    panels: List<WorkbenchPanelState>,
    activeTabByGroup: Map<String, WorkbenchPanelId>,
    onFloat: (WorkbenchPanelId) -> Unit,
    onCollapse: (WorkbenchPanelId) -> Unit,
    onHide: (WorkbenchPanelId) -> Unit,
    onActivateTab: (groupId: String, panelId: WorkbenchPanelId) -> Unit,
    modifier: Modifier = Modifier,
    panelContent: @Composable (WorkbenchPanelState) -> Unit,
) {
    val dims = LocalSujianDimensions.current
    val expandedPanels = panels.filter { it.visibility == PanelVisibility.Expanded }
    if (expandedPanels.isEmpty()) return

    val firstPanel = expandedPanels.first()
    val activePanelId = activeTabByGroup[firstPanel.tabGroupId] ?: firstPanel.id
    val activePanel = expandedPanels.find { it.id == activePanelId } ?: firstPanel

    Surface(
        color = MaterialTheme.colorScheme.surface,
        tonalElevation = 1.dp,
        modifier = modifier,
    ) {
        when (zone) {
            DockZone.Left, DockZone.Right -> {
                Column(modifier = Modifier.width(activePanel.sizeDp.dp).fillMaxHeight()) {
                    if (expandedPanels.size > 1) {
                        DockTabStrip(
                            panels = expandedPanels,
                            activeTabId = activePanelId,
                            onActivateTab = { panelId ->
                                onActivateTab(firstPanel.tabGroupId, panelId)
                            },
                        )
                    }
                    WorkbenchPanelFrame(
                        panelState = activePanel,
                        onFloat = { onFloat(activePanel.id) },
                        onCollapse = { onCollapse(activePanel.id) },
                        onClose = { onHide(activePanel.id) },
                        modifier = Modifier.fillMaxSize(),
                    ) {
                        panelContent(activePanel)
                    }
                }
            }
            DockZone.Bottom -> {
                Column(modifier = Modifier.fillMaxWidth().height(activePanel.sizeDp.dp)) {
                    if (expandedPanels.size > 1) {
                        DockTabStrip(
                            panels = expandedPanels,
                            activeTabId = activePanelId,
                            onActivateTab = { panelId ->
                                onActivateTab(firstPanel.tabGroupId, panelId)
                            },
                        )
                    }
                    WorkbenchPanelFrame(
                        panelState = activePanel,
                        onFloat = { onFloat(activePanel.id) },
                        onCollapse = { onCollapse(activePanel.id) },
                        onClose = { onHide(activePanel.id) },
                        modifier = Modifier.fillMaxSize(),
                    ) {
                        panelContent(activePanel)
                    }
                }
            }
            DockZone.Floating -> {}
        }
    }
}
