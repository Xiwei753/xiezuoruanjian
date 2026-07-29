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
import com.xiwei.sujian.ui.compose.workbench.model.DockGroupState
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
    onMovePanelToGroup: (panelId: WorkbenchPanelId, tabGroupId: String) -> Unit = { _, _ -> },
    onDragStart: ((WorkbenchPanelId, Float, Float) -> Unit)? = null,
    onDrag: ((WorkbenchPanelId, Float, Float) -> Unit)? = null,
    onDragEnd: ((WorkbenchPanelId) -> Unit)? = null,
    onResizeGroup: ((String, Float) -> Unit)? = null,
    modifier: Modifier = Modifier,
    panelContent: @Composable (WorkbenchPanelState) -> Unit,
) {
    val dims = LocalSujianDimensions.current
    val expandedPanels = panels.filter { it.visibility == PanelVisibility.Expanded }
    if (expandedPanels.isEmpty()) return

    val groups = expandedPanels
        .groupBy { it.tabGroupId }
        .map { (groupId, groupPanels) ->
            val sorted = groupPanels.sortedBy { it.order }
            DockGroupState(
                id = groupId,
                zone = zone,
                order = sorted.firstOrNull()?.order ?: 0,
                activePanelId = activeTabByGroup[groupId] ?: sorted.firstOrNull()?.id,
                panelIds = sorted.map { it.id },
            )
        }
        .sortedBy { it.order }

    Surface(
        color = MaterialTheme.colorScheme.surface,
        tonalElevation = 1.dp,
        modifier = modifier,
    ) {
        when (zone) {
            DockZone.Left, DockZone.Right -> {
                if (groups.size <= 1) {
                    val group = groups.first()
                    val activePanelId = group.activePanelId ?: group.panelIds.firstOrNull()
                    val activePanel = expandedPanels.find { it.id == activePanelId } ?: expandedPanels.first()
                    Column(modifier = Modifier.width(activePanel.sizeDp.dp).fillMaxHeight()) {
                        if (group.panelIds.size > 1) {
                            DockTabStrip(
                                panels = expandedPanels.filter { it.tabGroupId == group.id },
                                activeTabId = activePanelId ?: WorkbenchPanelId.ProjectNavigator,
                                onActivateTab = { panelId -> onActivateTab(group.id, panelId) },
                            )
                        }
                        WorkbenchPanelFrame(
                            panelState = activePanel,
                            onFloat = { onFloat(activePanel.id) },
                            onCollapse = { onCollapse(activePanel.id) },
                            onClose = { onHide(activePanel.id) },
                            onDragStart = onDragStart?.let { { x, y -> it(activePanel.id, x, y) } },
                            onDrag = onDrag?.let { { x, y -> it(activePanel.id, x, y) } },
                            onDragEnd = onDragEnd?.let { { _, _ -> it(activePanel.id) } },
                            modifier = Modifier.fillMaxSize(),
                        ) {
                            panelContent(activePanel)
                        }
                    }
                } else {
                    Column(modifier = Modifier.fillMaxHeight()) {
                        for ((index, group) in groups.withIndex()) {
                            val groupPanels = expandedPanels.filter { it.tabGroupId == group.id }
                            val activePanelId = group.activePanelId ?: group.panelIds.firstOrNull()
                            val activePanel = groupPanels.find { it.id == activePanelId } ?: groupPanels.firstOrNull()
                            if (activePanel != null) {
                                Column(
                                    modifier = Modifier.width(activePanel.sizeDp.dp).weight(1f)
                                ) {
                                    if (group.panelIds.size > 1) {
                                        DockTabStrip(
                                            panels = groupPanels,
                                            activeTabId = activePanelId ?: WorkbenchPanelId.ProjectNavigator,
                                            onActivateTab = { panelId -> onActivateTab(group.id, panelId) },
                                        )
                                    }
                                    WorkbenchPanelFrame(
                                        panelState = activePanel,
                                        onFloat = { onFloat(activePanel.id) },
                                        onCollapse = { onCollapse(activePanel.id) },
                                        onClose = { onHide(activePanel.id) },
                                        onDragStart = onDragStart?.let { { x, y -> it(activePanel.id, x, y) } },
                                        onDrag = onDrag?.let { { x, y -> it(activePanel.id, x, y) } },
                                        onDragEnd = onDragEnd?.let { { _, _ -> it(activePanel.id) } },
                                        modifier = Modifier.fillMaxSize(),
                                    ) {
                                        panelContent(activePanel)
                                    }
                                }
                            }
                            if (index < groups.size - 1) {
                                DockResizeHandle(
                                    zone = zone,
                                    panelId = group.panelIds.first(),
                                    currentSizeDp = activePanel?.sizeDp ?: group.sizeRatio * 300f,
                                    onResize = { _, newSize ->
                                        onResizeGroup?.invoke(group.id, newSize)
                                    },
                                    modifier = if (zone == DockZone.Left || zone == DockZone.Right) {
                                        Modifier.fillMaxHeight().width(4.dp)
                                    } else {
                                        Modifier.fillMaxWidth().height(4.dp)
                                    },
                                )
                            }
                        }
                    }
                }
            }
            DockZone.Bottom -> {
                if (groups.size <= 1) {
                    val group = groups.first()
                    val activePanelId = group.activePanelId ?: group.panelIds.firstOrNull()
                    val activePanel = expandedPanels.find { it.id == activePanelId } ?: expandedPanels.first()
                    Column(modifier = Modifier.fillMaxWidth().height(activePanel.sizeDp.dp)) {
                        if (group.panelIds.size > 1) {
                            DockTabStrip(
                                panels = expandedPanels.filter { it.tabGroupId == group.id },
                                activeTabId = activePanelId ?: WorkbenchPanelId.ProjectNavigator,
                                onActivateTab = { panelId -> onActivateTab(group.id, panelId) },
                            )
                        }
                        WorkbenchPanelFrame(
                            panelState = activePanel,
                            onFloat = { onFloat(activePanel.id) },
                            onCollapse = { onCollapse(activePanel.id) },
                            onClose = { onHide(activePanel.id) },
                            onDragStart = onDragStart?.let { { x, y -> it(activePanel.id, x, y) } },
                            onDrag = onDrag?.let { { x, y -> it(activePanel.id, x, y) } },
                            onDragEnd = onDragEnd?.let { { _, _ -> it(activePanel.id) } },
                            modifier = Modifier.fillMaxSize(),
                        ) {
                            panelContent(activePanel)
                        }
                    }
                } else {
                    Row(modifier = Modifier.fillMaxWidth()) {
                        for ((index, group) in groups.withIndex()) {
                            val groupPanels = expandedPanels.filter { it.tabGroupId == group.id }
                            val activePanelId = group.activePanelId ?: group.panelIds.firstOrNull()
                            val activePanel = groupPanels.find { it.id == activePanelId } ?: groupPanels.firstOrNull()
                            if (activePanel != null) {
                                Column(
                                    modifier = Modifier.weight(1f).height(activePanel.sizeDp.dp)
                                ) {
                                    if (group.panelIds.size > 1) {
                                        DockTabStrip(
                                            panels = groupPanels,
                                            activeTabId = activePanelId ?: WorkbenchPanelId.ProjectNavigator,
                                            onActivateTab = { panelId -> onActivateTab(group.id, panelId) },
                                        )
                                    }
                                    WorkbenchPanelFrame(
                                        panelState = activePanel,
                                        onFloat = { onFloat(activePanel.id) },
                                        onCollapse = { onCollapse(activePanel.id) },
                                        onClose = { onHide(activePanel.id) },
                                        onDragStart = onDragStart?.let { { x, y -> it(activePanel.id, x, y) } },
                                        onDrag = onDrag?.let { { x, y -> it(activePanel.id, x, y) } },
                                        onDragEnd = onDragEnd?.let { { _, _ -> it(activePanel.id) } },
                                        modifier = Modifier.fillMaxSize(),
                                    ) {
                                        panelContent(activePanel)
                                    }
                                }
                            }
                        }
                    }
                }
            }
            DockZone.Floating -> {}
        }
    }
}
