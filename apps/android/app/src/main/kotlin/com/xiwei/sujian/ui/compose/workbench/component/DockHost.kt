package com.xiwei.sujian.ui.compose.workbench.component

import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectDragGestures
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
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInWindow
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.designsystem.theme.LocalSujianDimensions
import com.xiwei.sujian.ui.compose.workbench.model.DockGroupState
import com.xiwei.sujian.ui.compose.workbench.model.DockZone
import com.xiwei.sujian.ui.compose.workbench.model.PanelVisibility
import com.xiwei.sujian.ui.compose.workbench.model.TabGroupHitArea
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelId
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelState

private fun computeTabGroupHitArea(
    groupId: String,
    coords: androidx.compose.ui.layout.LayoutCoordinates,
    density: Float,
): TabGroupHitArea {
    val pos = coords.positionInWindow()
    val px: Float = pos.x
    val py: Float = pos.y
    val w: Float = coords.size.width.toFloat()
    val h: Float = coords.size.height.toFloat()
    val leftDp = px / density
    val topDp = py / density
    val rightDp = (px + w) / density
    val bottomDp = (py + h) / density
    return TabGroupHitArea(
        groupId = groupId,
        left = leftDp,
        top = topDp,
        right = rightDp,
        bottom = bottomDp,
    )
}

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
    onDragCancel: (() -> Unit)? = null,
    onResizeGroup: ((groupId: String, newSizeDp: Float) -> Unit)? = null,
    onRegisterTabGroupHitArea: ((TabGroupHitArea) -> Unit)? = null,
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

    val density = LocalDensity.current

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
                                modifier = Modifier.onGloballyPositioned { coords ->
                                    onRegisterTabGroupHitArea?.invoke(
                                        computeTabGroupHitArea(group.id, coords, density.density)
                                    )
                                },
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
                            onDragCancel = onDragCancel,
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
                                             modifier = Modifier.onGloballyPositioned { coords ->
                                                 onRegisterTabGroupHitArea?.invoke(
                                                     computeTabGroupHitArea(group.id, coords, density.density)
                                                 )
                                             },
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
                                         onDragCancel = onDragCancel,
                                         modifier = Modifier.fillMaxSize(),
                                     ) {
                                         panelContent(activePanel)
                                     }
                                 }
                             }
                             if (index < groups.size - 1) {
                                 DockGroupResizeHandle(
                                     zone = zone,
                                     groupId = group.id,
                                     onResize = { gId, delta ->
                                         onResizeGroup?.invoke(gId, delta)
                                     },
                                     modifier = Modifier.fillMaxWidth().height(4.dp),
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
                                modifier = Modifier.onGloballyPositioned { coords ->
                                    onRegisterTabGroupHitArea?.invoke(
                                        computeTabGroupHitArea(group.id, coords, density.density)
                                    )
                                },
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
                            onDragCancel = onDragCancel,
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
                                            modifier = Modifier.onGloballyPositioned { coords ->
                                                onRegisterTabGroupHitArea?.invoke(
                                                    computeTabGroupHitArea(group.id, coords, density.density)
                                                )
                                            },
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
                                        onDragCancel = onDragCancel,
                                        modifier = Modifier.fillMaxSize(),
                                    ) {
                                        panelContent(activePanel)
                                    }
                                }
                            }
                            if (index < groups.size - 1) {
                                DockGroupResizeHandle(
                                    zone = zone,
                                    groupId = group.id,
                                    onResize = { gId, delta ->
                                        onResizeGroup?.invoke(gId, delta)
                                    },
                                    modifier = Modifier.fillMaxHeight().width(4.dp),
                                )
                            }
                        }
                    }
                }
            }
            DockZone.Floating -> {}
        }
    }
}

@Composable
fun DockGroupResizeHandle(
    zone: DockZone,
    groupId: String,
    onResize: (groupId: String, newSizeDp: Float) -> Unit,
    modifier: Modifier = Modifier,
) {
    val handleColor = MaterialTheme.colorScheme.outlineVariant
    val density = LocalDensity.current
    var accumulatedDp by remember { mutableFloatStateOf(0f) }

    Box(
        modifier = modifier
            .background(handleColor)
            .pointerInput(zone, groupId) {
                detectDragGestures(
                    onDragEnd = { accumulatedDp = 0f },
                    onDragCancel = { accumulatedDp = 0f },
                ) { change, dragAmount ->
                    val deltaDp = when (zone) {
                        DockZone.Left, DockZone.Right -> dragAmount.y / density.density
                        DockZone.Bottom -> dragAmount.x / density.density
                        DockZone.Floating -> 0f
                    }
                    accumulatedDp += deltaDp
                    if (accumulatedDp != 0f) {
                        onResize(groupId, accumulatedDp)
                    }
                    change.consume()
                }
            }
    )
}
