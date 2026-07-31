package com.xiwei.sujian.ui.compose.workbench.component

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
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
import com.xiwei.sujian.ui.compose.workbench.model.DockGroupState
import com.xiwei.sujian.ui.compose.workbench.model.DockZone
import com.xiwei.sujian.ui.compose.workbench.model.TabGroupHitArea
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelId
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelState

private fun computeTabGroupHitArea(
    groupId: String,
    coords: androidx.compose.ui.layout.LayoutCoordinates,
    density: Float,
): TabGroupHitArea {
    val pos = coords.positionInWindow()
    val w: Float = coords.size.width.toFloat()
    val h: Float = coords.size.height.toFloat()
    return TabGroupHitArea(
        groupId = groupId,
        left = pos.x / density,
        top = pos.y / density,
        right = (pos.x + w) / density,
        bottom = (pos.y + h) / density,
    )
}

enum class SplitHandleOrientation { Horizontal, Vertical }

fun splitHandleOrientation(zone: DockZone): SplitHandleOrientation = when (zone) {
    DockZone.Left, DockZone.Right -> SplitHandleOrientation.Horizontal
    DockZone.Bottom -> SplitHandleOrientation.Vertical
    DockZone.Floating -> SplitHandleOrientation.Horizontal
}

@Composable
private fun splitHandleModifier(zone: DockZone): Modifier {
    return when (splitHandleOrientation(zone)) {
        SplitHandleOrientation.Horizontal -> Modifier.fillMaxWidth().height(4.dp)
        SplitHandleOrientation.Vertical -> Modifier.fillMaxHeight().width(4.dp)
    }
}

@Composable
private fun titleBarHitAreaModifier(
    groupId: String,
    onRegisterTabGroupHitArea: ((TabGroupHitArea) -> Unit)?,
    hasTabStrip: Boolean,
): Modifier {
    if (hasTabStrip || onRegisterTabGroupHitArea == null) return Modifier
    val density = LocalDensity.current
    return Modifier.onGloballyPositioned { coords ->
        onRegisterTabGroupHitArea(computeTabGroupHitArea(groupId, coords, density.density))
    }
}

@Composable
fun DockHost(
    zone: DockZone,
    panels: List<WorkbenchPanelState>,
    groups: List<DockGroupState>,
    activeTabByGroup: Map<String, WorkbenchPanelId>,
    dockGroupWeights: Map<String, Float> = emptyMap(),
    dockZoneSizeDp: Float = 0f,
    onFloat: (WorkbenchPanelId) -> Unit,
    onCollapse: (WorkbenchPanelId) -> Unit,
    onHide: (WorkbenchPanelId) -> Unit,
    onActivateTab: (groupId: String, panelId: WorkbenchPanelId) -> Unit,
    onMovePanelToGroup: (panelId: WorkbenchPanelId, tabGroupId: String) -> Unit = { _, _ -> },
    onDragStart: ((WorkbenchPanelId, Float, Float) -> Unit)? = null,
    onDrag: ((WorkbenchPanelId, Float, Float) -> Unit)? = null,
    onDragEnd: ((WorkbenchPanelId) -> Unit)? = null,
    onDragCancel: (() -> Unit)? = null,
    onResizeSplit: ((zone: DockZone, beforeGroupId: String, afterGroupId: String, deltaDp: Float, availableMainAxisDp: Float) -> Unit)? = null,
    onReorderDockGroup: ((groupId: String, newOrder: Int) -> Unit)? = null,
    onRegisterTabGroupHitArea: ((TabGroupHitArea) -> Unit)? = null,
    onTitleBarPositionChanged: ((panelId: WorkbenchPanelId, xWindowPx: Float, yWindowPx: Float) -> Unit)? = null,
    modifier: Modifier = Modifier,
    panelContent: @Composable (WorkbenchPanelState) -> Unit,
) {
    if (groups.isEmpty()) return

    val density = LocalDensity.current

    Surface(
        color = MaterialTheme.colorScheme.surface,
        tonalElevation = 1.dp,
        modifier = modifier,
    ) {
        when (zone) {
            DockZone.Left, DockZone.Right -> {
                val zoneWidth = dockZoneSizeDp.coerceAtLeast(0f)
                if (groups.size <= 1) {
                    val group = groups.first()
                    val activePanelId = group.activePanelId ?: group.panelIds.firstOrNull()
                    val activePanel = panels.find { it.id == activePanelId } ?: panels.filter { it.tabGroupId == group.id }.firstOrNull()
                    if (activePanel != null) {
                        Column(
                            modifier = Modifier
                                .width(zoneWidth.dp)
                                .fillMaxHeight(),
                        ) {
                            if (group.panelIds.size > 1) {
                                DockTabStrip(
                                    panels = panels.filter { it.tabGroupId == group.id },
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
                                titleBarModifier = titleBarHitAreaModifier(group.id, onRegisterTabGroupHitArea, group.panelIds.size > 1),
                                onTitleBarPositionChanged = onTitleBarPositionChanged?.let { callback ->
                                    { x, y -> callback(activePanel.id, x, y) }
                            },
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
                } else {
                    BoxWithConstraints(
                        modifier = Modifier
                            .width(zoneWidth.dp)
                            .fillMaxHeight()
                    ) {
                        val availableMainAxisDp = maxHeight.value - (groups.size - 1) * 4f
                        Column(
                            modifier = Modifier.fillMaxSize()
                        ) {
                            for ((index, group) in groups.withIndex()) {
                                val groupPanels = panels.filter { it.tabGroupId == group.id }
                                val activePanelId = group.activePanelId ?: group.panelIds.firstOrNull()
                                val activePanel = groupPanels.find { it.id == activePanelId } ?: groupPanels.firstOrNull()
                                if (activePanel != null) {
                                    Column(
                                        modifier = Modifier
                                            .weight(group.weight.coerceAtLeast(0.1f))
                                            .fillMaxWidth(),
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
                                        DockGroupMoveBar(
                                            groupIndex = index,
                                            totalGroups = groups.size,
                                            groupId = group.id,
                                            onReorderDockGroup = onReorderDockGroup,
                                        )
                                        WorkbenchPanelFrame(
                                            panelState = activePanel,
                                            onFloat = { onFloat(activePanel.id) },
                                            onCollapse = { onCollapse(activePanel.id) },
                                            onClose = { onHide(activePanel.id) },
                                            titleBarModifier = titleBarHitAreaModifier(group.id, onRegisterTabGroupHitArea, group.panelIds.size > 1),
                                            onTitleBarPositionChanged = onTitleBarPositionChanged?.let { callback ->
                                                { x, y -> callback(activePanel.id, x, y) }
                                            },
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
                                    val beforeGroupId = group.id
                                    val afterGroupId = groups[index + 1].id
                                    DockSplitResizeHandle(
                                        zone = zone,
                                        beforeGroupId = beforeGroupId,
                                        afterGroupId = afterGroupId,
                                        availableMainAxisDp = availableMainAxisDp.coerceAtLeast(0f),
                                        onResizeSplit = onResizeSplit,
                                        modifier = splitHandleModifier(zone),
                                    )
                                }
                            }
                        }
                    }
                }
            }
            DockZone.Bottom -> {
                val zoneHeight = dockZoneSizeDp.coerceAtLeast(0f)
                if (groups.size <= 1) {
                    val group = groups.first()
                    val activePanelId = group.activePanelId ?: group.panelIds.firstOrNull()
                    val activePanel = panels.find { it.id == activePanelId } ?: panels.filter { it.tabGroupId == group.id }.firstOrNull()
                    if (activePanel != null) {
                        Column(
                            modifier = Modifier
                                .fillMaxWidth()
                                .height(zoneHeight.dp),
                        ) {
                            if (group.panelIds.size > 1) {
                                DockTabStrip(
                                    panels = panels.filter { it.tabGroupId == group.id },
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
                                titleBarModifier = titleBarHitAreaModifier(group.id, onRegisterTabGroupHitArea, group.panelIds.size > 1),
                                onTitleBarPositionChanged = onTitleBarPositionChanged?.let { callback ->
                                    { x, y -> callback(activePanel.id, x, y) }
                                },
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
                } else {
                    BoxWithConstraints(
                        modifier = Modifier
                            .fillMaxWidth()
                            .height(zoneHeight.dp)
                    ) {
                        val availableMainAxisDp = maxWidth.value - (groups.size - 1) * 4f
                        Row(
                            modifier = Modifier.fillMaxSize()
                        ) {
                            for ((index, group) in groups.withIndex()) {
                                val groupPanels = panels.filter { it.tabGroupId == group.id }
                                val activePanelId = group.activePanelId ?: group.panelIds.firstOrNull()
                                val activePanel = groupPanels.find { it.id == activePanelId } ?: groupPanels.firstOrNull()
                                if (activePanel != null) {
                                    Column(
                                        modifier = Modifier
                                            .weight(group.weight.coerceAtLeast(0.1f))
                                            .fillMaxHeight(),
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
                                        DockGroupMoveBar(
                                            groupIndex = index,
                                            totalGroups = groups.size,
                                            groupId = group.id,
                                            onReorderDockGroup = onReorderDockGroup,
                                        )
                                        WorkbenchPanelFrame(
                                            panelState = activePanel,
                                            onFloat = { onFloat(activePanel.id) },
                                            onCollapse = { onCollapse(activePanel.id) },
                                            onClose = { onHide(activePanel.id) },
                                            titleBarModifier = titleBarHitAreaModifier(group.id, onRegisterTabGroupHitArea, group.panelIds.size > 1),
                                            onTitleBarPositionChanged = onTitleBarPositionChanged?.let { callback ->
                                                { x, y -> callback(activePanel.id, x, y) }
                                            },
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
                                    val beforeGroupId = group.id
                                    val afterGroupId = groups[index + 1].id
                                    DockSplitResizeHandle(
                                        zone = zone,
                                        beforeGroupId = beforeGroupId,
                                        afterGroupId = afterGroupId,
                                        availableMainAxisDp = availableMainAxisDp.coerceAtLeast(0f),
                                        onResizeSplit = onResizeSplit,
                                        modifier = splitHandleModifier(zone),
                                    )
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

@Composable
fun DockSplitResizeHandle(
    zone: DockZone,
    beforeGroupId: String,
    afterGroupId: String,
    availableMainAxisDp: Float,
    onResizeSplit: ((zone: DockZone, beforeGroupId: String, afterGroupId: String, deltaDp: Float, availableMainAxisDp: Float) -> Unit)? = null,
    modifier: Modifier = Modifier,
) {
    val handleColor = MaterialTheme.colorScheme.outlineVariant
    val density = LocalDensity.current

    Box(
        modifier = modifier
            .background(handleColor)
            .pointerInput(zone, beforeGroupId, afterGroupId) {
                detectDragGestures(
                    onDragEnd = {},
                    onDragCancel = {},
                ) { change, dragAmount ->
                    val deltaDp = when (zone) {
                        DockZone.Left, DockZone.Right -> dragAmount.y / density.density
                        DockZone.Bottom -> dragAmount.x / density.density
                        DockZone.Floating -> 0f
                    }
                    if (deltaDp != 0f && onResizeSplit != null) {
                        onResizeSplit(zone, beforeGroupId, afterGroupId, deltaDp, availableMainAxisDp)
                    }
                    change.consume()
                }
            }
    )
}

@Composable
private fun DockGroupMoveBar(
    groupIndex: Int,
    totalGroups: Int,
    groupId: String,
    onReorderDockGroup: ((groupId: String, newOrder: Int) -> Unit)?,
    modifier: Modifier = Modifier,
) {
    if (onReorderDockGroup == null || totalGroups <= 1) return
    Row(modifier = modifier.padding(horizontal = 2.dp)) {
        if (groupIndex > 0) {
            androidx.compose.material3.Text(
                text = "▲",
                style = MaterialTheme.typography.labelSmall,
                modifier = Modifier
                    .clickable { onReorderDockGroup(groupId, groupIndex - 1) }
                    .padding(horizontal = 4.dp, vertical = 2.dp),
            )
        }
        if (groupIndex < totalGroups - 1) {
            androidx.compose.material3.Text(
                text = "▼",
                style = MaterialTheme.typography.labelSmall,
                modifier = Modifier
                    .clickable { onReorderDockGroup(groupId, groupIndex + 1) }
                    .padding(horizontal = 4.dp, vertical = 2.dp),
            )
        }
    }
}
