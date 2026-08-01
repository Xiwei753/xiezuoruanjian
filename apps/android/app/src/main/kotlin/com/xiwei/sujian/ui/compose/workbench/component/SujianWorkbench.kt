package com.xiwei.sujian.ui.compose.workbench.component

import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
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
import androidx.compose.material3.Text
import androidx.compose.material3.Icon
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.designsystem.theme.LocalSujianDimensions
import com.xiwei.sujian.ui.compose.workbench.model.DockZone
import com.xiwei.sujian.ui.compose.workbench.model.DragDropTarget
import com.xiwei.sujian.ui.compose.workbench.model.PanelVisibility
import com.xiwei.sujian.ui.compose.workbench.model.TabGroupHitArea
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchAction
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchDragState
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchLayoutState
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelId
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelState
import com.xiwei.sujian.ui.compose.workbench.model.filterTabGroupHitAreas
import com.xiwei.sujian.ui.compose.workbench.model.upsertTabGroupHitArea
import com.xiwei.sujian.ui.compose.workbench.state.WorkbenchReducer

private const val DUAL_SIDE_THRESHOLD_DP = 1200

@Composable
fun SujianWorkbench(
    layoutState: WorkbenchLayoutState,
    onAction: (WorkbenchAction) -> Unit,
    onWindowSizeChanged: (maxWidthDp: Float, maxHeightDp: Float) -> Unit = { _, _ -> },
    modifier: Modifier = Modifier,
    editorContent: @Composable () -> Unit,
    panelContent: @Composable (WorkbenchPanelState) -> Unit,
) {
    val dims = LocalSujianDimensions.current
    val density = LocalDensity.current

    var dragState by remember { mutableStateOf(WorkbenchDragState.Idle) }
    var tabGroupHitAreas by remember { mutableStateOf<List<TabGroupHitArea>>(emptyList()) }
    var titleBarPositions by remember { mutableStateOf<Map<WorkbenchPanelId, Pair<Float, Float>>>(emptyMap()) }

    BoxWithConstraints(modifier = modifier.fillMaxSize()) {
        val maxWidthDp = maxWidth.value
        val maxHeightDp = maxHeight.value

        LaunchedEffect(maxWidthDp, maxHeightDp) {
            onWindowSizeChanged(maxWidthDp, maxHeightDp)
        }

        val allowDualSide = maxWidthDp >= DUAL_SIDE_THRESHOLD_DP

        val presentationState = remember(layoutState, maxWidthDp, maxHeightDp) {
            WorkbenchReducer.computePresentationState(layoutState, maxWidthDp, maxHeightDp)
        }

        val leftPanels = layoutState.panels.values
            .filter { it.zone == DockZone.Left }
            .sortedBy { it.order }
        val rightPanels = layoutState.panels.values
            .filter { it.zone == DockZone.Right }
            .sortedBy { it.order }
        val bottomPanels = layoutState.panels.values
            .filter { it.zone == DockZone.Bottom }
            .sortedBy { it.order }

        val leftExpanded = leftPanels.filter { it.visibility == PanelVisibility.Expanded }
        val rightExpanded = rightPanels.filter { it.visibility == PanelVisibility.Expanded }
        val bottomExpanded = bottomPanels.filter { it.visibility == PanelVisibility.Expanded }

        val overlayPanelIds = presentationState.overlayPanelIds.toSet()

        val leftForDock = leftExpanded.filter { it.id !in overlayPanelIds }
        val rightForDock = rightExpanded.filter { it.id !in overlayPanelIds }
        val bottomForDock = bottomExpanded.filter { it.id !in overlayPanelIds }

        val leftDockGroups = layoutState.dockGroupsForHost(DockZone.Left, overlayPanelIds)
        val rightDockGroups = layoutState.dockGroupsForHost(DockZone.Right, overlayPanelIds)
        val bottomDockGroups = layoutState.dockGroupsForHost(DockZone.Bottom, overlayPanelIds)
        val liveGroupIds = (leftDockGroups + rightDockGroups + bottomDockGroups).map { it.id }.toSet()

        LaunchedEffect(liveGroupIds) {
            tabGroupHitAreas = filterTabGroupHitAreas(tabGroupHitAreas, liveGroupIds)
        }

        val leftCollapsed = leftPanels.filter { it.visibility == PanelVisibility.Collapsed || it.visibility == PanelVisibility.Hidden }
        val rightCollapsed = rightPanels.filter { it.visibility == PanelVisibility.Collapsed || it.visibility == PanelVisibility.Hidden }
        val bottomCollapsed = bottomPanels.filter { it.visibility == PanelVisibility.Collapsed || it.visibility == PanelVisibility.Hidden }

        val showRightDock = rightForDock.isNotEmpty() && (allowDualSide || leftForDock.isEmpty())

        val onTitleBarPositionChanged: (WorkbenchPanelId, Float, Float) -> Unit = { id, xWindowPx, yWindowPx ->
            titleBarPositions = titleBarPositions + (id to (xWindowPx to yWindowPx))
        }

        val onPanelDragStart: (WorkbenchPanelId, Float, Float) -> Unit = { id, pointerLocalDpX, pointerLocalDpY ->
            val titleBarPosPx = titleBarPositions[id]
            val absoluteXDp = if (titleBarPosPx != null) {
                titleBarPosPx.first / density.density + pointerLocalDpX
            } else {
                pointerLocalDpX
            }
            val absoluteYDp = if (titleBarPosPx != null) {
                titleBarPosPx.second / density.density + pointerLocalDpY
            } else {
                pointerLocalDpY
            }
            dragState = WorkbenchDragState(
                isDragging = true,
                draggedPanelId = id,
                pointerX = absoluteXDp,
                pointerY = absoluteYDp,
                dropTarget = DragDropTarget.None,
                tabGroupHitAreas = tabGroupHitAreas,
            )
        }

        val onPanelDrag: (WorkbenchPanelId, Float, Float) -> Unit = { _, dx, dy ->
            val newX = dragState.pointerX + dx
            val newY = dragState.pointerY + dy
            val (target, groupId) = dragState.copy(
                pointerX = newX, pointerY = newY,
                tabGroupHitAreas = tabGroupHitAreas,
            ).resolveDropTarget(maxWidthDp, maxHeightDp)
            dragState = dragState.copy(
                pointerX = newX,
                pointerY = newY,
                dropTarget = target,
                targetTabGroupId = groupId,
                tabGroupHitAreas = tabGroupHitAreas,
            )
        }

        val onPanelDragEnd: (WorkbenchPanelId) -> Unit = { id ->
            val target = dragState.dropTarget
            when (target) {
                DragDropTarget.DockLeft -> onAction(WorkbenchAction.DockPanelAsNewGroup(id, DockZone.Left, Int.MAX_VALUE))
                DragDropTarget.DockRight -> onAction(WorkbenchAction.DockPanelAsNewGroup(id, DockZone.Right, Int.MAX_VALUE))
                DragDropTarget.DockBottom -> onAction(WorkbenchAction.DockPanelAsNewGroup(id, DockZone.Bottom, Int.MAX_VALUE))
                DragDropTarget.TabGroup -> {
                    val groupId = dragState.targetTabGroupId
                    if (groupId != null) {
                        onAction(WorkbenchAction.MovePanelToGroup(id, groupId))
                    } else {
                        onAction(WorkbenchAction.FloatPanelAt(id, dragState.pointerX, dragState.pointerY))
                    }
                }
                else -> onAction(WorkbenchAction.FloatPanelAt(id, dragState.pointerX, dragState.pointerY))
            }
            dragState = WorkbenchDragState.Idle
        }

        val onPanelDragCancel: () -> Unit = {
            dragState = WorkbenchDragState.Idle
        }

        val onRegisterTabGroupHitArea: (TabGroupHitArea) -> Unit = { area ->
            tabGroupHitAreas = upsertTabGroupHitArea(tabGroupHitAreas, area)
        }

        Column(modifier = Modifier.fillMaxSize()) {
            Row(
                modifier = Modifier.weight(1f).fillMaxWidth()
            ) {
                if (leftCollapsed.isNotEmpty()) {
                    PanelLauncherRail(
                        zone = DockZone.Left,
                        panels = leftCollapsed,
                        onTogglePanel = { onAction(WorkbenchAction.TogglePanel(it)) },
                        modifier = Modifier.width(56.dp).fillMaxHeight(),
                    )
                }

                if (leftForDock.isNotEmpty()) {
                    DockHost(
                        zone = DockZone.Left,
                        panels = leftForDock,
                        groups = leftDockGroups,
                        activeTabByGroup = layoutState.activeTabByGroup,
                        dockGroupWeights = layoutState.dockGroupWeights,
                        dockZoneSizeDp = layoutState.dockZoneSizeDp[DockZone.Left] ?: WorkbenchReducer.SIDE_PANEL_MIN_DP,
                        onFloat = { onAction(WorkbenchAction.FloatPanel(it)) },
                        onCollapse = { onAction(WorkbenchAction.CollapsePanel(it)) },
                        onHide = { onAction(WorkbenchAction.HidePanel(it)) },
                        onActivateTab = { g, p -> onAction(WorkbenchAction.ActivateTab(g, p)) },
                        onMovePanelToGroup = { p, g -> onAction(WorkbenchAction.MovePanelToGroup(p, g)) },
                        onDragStart = onPanelDragStart,
                        onDrag = onPanelDrag,
                        onDragEnd = onPanelDragEnd,
                        onDragCancel = onPanelDragCancel,
                        onResizeSplit = { zone, beforeId, afterId, delta, available -> onAction(WorkbenchAction.ResizeDockSplit(zone, beforeId, afterId, delta, available)) },
                        onReorderDockGroup = { groupId, newOrder -> onAction(WorkbenchAction.ReorderDockGroup(groupId, newOrder)) },
                        onRegisterTabGroupHitArea = onRegisterTabGroupHitArea,
                        onTitleBarPositionChanged = onTitleBarPositionChanged,
                        modifier = Modifier.fillMaxHeight(),
                        panelContent = panelContent,
                    )
                    DockResizeHandle(
                        zone = DockZone.Left,
                        onResizeZoneDelta = { z, delta ->
                            val otherActualWidth: Float? = if (showRightDock) layoutState.actualSideWidthDp(DockZone.Right).let { if (it > 0f) it else null } else 0f
                            onAction(WorkbenchAction.ResizeDockZone(z, delta, maxWidthDp, otherActualWidth))
                        },
                        modifier = Modifier.fillMaxHeight(),
                    )
                }

                Surface(
                    modifier = Modifier.weight(1f).fillMaxHeight(),
                    color = MaterialTheme.colorScheme.background,
                ) {
                    Box(
                        modifier = Modifier.fillMaxSize(),
                        contentAlignment = Alignment.Center,
                    ) {
                        editorContent()
                    }
                }

                if (showRightDock) {
                    DockResizeHandle(
                        zone = DockZone.Right,
                        onResizeZoneDelta = { z, delta ->
                            val otherActualWidth: Float? = if (leftForDock.isNotEmpty()) layoutState.actualSideWidthDp(DockZone.Left).let { if (it > 0f) it else null } else 0f
                            onAction(WorkbenchAction.ResizeDockZone(z, delta, maxWidthDp, otherActualWidth))
                        },
                        modifier = Modifier.fillMaxHeight(),
                    )
                    DockHost(
                        zone = DockZone.Right,
                        panels = rightForDock,
                        groups = rightDockGroups,
                        activeTabByGroup = layoutState.activeTabByGroup,
                        dockGroupWeights = layoutState.dockGroupWeights,
                        dockZoneSizeDp = layoutState.dockZoneSizeDp[DockZone.Right] ?: WorkbenchReducer.SIDE_PANEL_MIN_DP,
                        onFloat = { onAction(WorkbenchAction.FloatPanel(it)) },
                        onCollapse = { onAction(WorkbenchAction.CollapsePanel(it)) },
                        onHide = { onAction(WorkbenchAction.HidePanel(it)) },
                        onActivateTab = { g, p -> onAction(WorkbenchAction.ActivateTab(g, p)) },
                        onMovePanelToGroup = { p, g -> onAction(WorkbenchAction.MovePanelToGroup(p, g)) },
                        onDragStart = onPanelDragStart,
                        onDrag = onPanelDrag,
                        onDragEnd = onPanelDragEnd,
                        onDragCancel = onPanelDragCancel,
                        onResizeSplit = { zone, beforeId, afterId, delta, available -> onAction(WorkbenchAction.ResizeDockSplit(zone, beforeId, afterId, delta, available)) },
                        onReorderDockGroup = { groupId, newOrder -> onAction(WorkbenchAction.ReorderDockGroup(groupId, newOrder)) },
                        onRegisterTabGroupHitArea = onRegisterTabGroupHitArea,
                        onTitleBarPositionChanged = onTitleBarPositionChanged,
                        modifier = Modifier.fillMaxHeight(),
                        panelContent = panelContent,
                    )
                }

                if (rightCollapsed.isNotEmpty()) {
                    PanelLauncherRail(
                        zone = DockZone.Right,
                        panels = rightCollapsed,
                        onTogglePanel = { onAction(WorkbenchAction.TogglePanel(it)) },
                        modifier = Modifier.width(56.dp).fillMaxHeight(),
                    )
                }
            }

            if (bottomForDock.isNotEmpty()) {
                DockResizeHandle(
                    zone = DockZone.Bottom,
                    onResizeZoneDelta = { z, delta -> onAction(WorkbenchAction.ResizeDockZone(z, delta, maxHeightDp)) },
                    modifier = Modifier.fillMaxWidth(),
                )
                DockHost(
                    zone = DockZone.Bottom,
                    panels = bottomForDock,
                    groups = bottomDockGroups,
                    activeTabByGroup = layoutState.activeTabByGroup,
                    dockGroupWeights = layoutState.dockGroupWeights,
                    dockZoneSizeDp = layoutState.dockZoneSizeDp[DockZone.Bottom] ?: WorkbenchReducer.BOTTOM_PANEL_MIN_DP,
                    onFloat = { onAction(WorkbenchAction.FloatPanel(it)) },
                    onCollapse = { onAction(WorkbenchAction.CollapsePanel(it)) },
                    onHide = { onAction(WorkbenchAction.HidePanel(it)) },
                    onActivateTab = { g, p -> onAction(WorkbenchAction.ActivateTab(g, p)) },
                    onMovePanelToGroup = { p, g -> onAction(WorkbenchAction.MovePanelToGroup(p, g)) },
                    onDragStart = onPanelDragStart,
                    onDrag = onPanelDrag,
                    onDragEnd = onPanelDragEnd,
                    onDragCancel = onPanelDragCancel,
                    onResizeSplit = { zone, beforeId, afterId, delta, available -> onAction(WorkbenchAction.ResizeDockSplit(zone, beforeId, afterId, delta, available)) },
                    onReorderDockGroup = { groupId, newOrder -> onAction(WorkbenchAction.ReorderDockGroup(groupId, newOrder)) },
                    onRegisterTabGroupHitArea = onRegisterTabGroupHitArea,
                    onTitleBarPositionChanged = onTitleBarPositionChanged,
                    modifier = Modifier.fillMaxWidth(),
                    panelContent = panelContent,
                )
            }

            if (bottomCollapsed.isNotEmpty()) {
                PanelLauncherRail(
                    zone = DockZone.Bottom,
                    panels = bottomCollapsed,
                    onTogglePanel = { onAction(WorkbenchAction.TogglePanel(it)) },
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        }

        Box(modifier = Modifier.fillMaxSize()) {
            FloatingPanelLayer(
                panels = layoutState.panels.values.toList(),
                onFloat = { onAction(WorkbenchAction.FloatPanel(it)) },
                onCollapse = { onAction(WorkbenchAction.CollapsePanel(it)) },
                onHide = { onAction(WorkbenchAction.HidePanel(it)) },
                onMoveFloating = { id, x, y ->
                    val panel = layoutState.panels[id]
                    val w = panel?.floatingWidthDp ?: 420f
                    val h = panel?.floatingHeightDp ?: 560f
                    val (cx, cy) = WorkbenchReducer.clampFloatingPosition(x, y, w, h, maxWidthDp, maxHeightDp)
                    onAction(WorkbenchAction.MoveFloatingPanel(id, cx, cy))
                },
                onDock = { id, zone -> onAction(WorkbenchAction.DockPanelAsNewGroup(id, zone, Int.MAX_VALUE)) },
                onBringToFront = { onAction(WorkbenchAction.BringFloatingToFront(it)) },
                onResizeFloating = { id, w, h ->
                    val (clampedW, clampedH) = WorkbenchReducer.clampFloatingSize(w, h, maxWidthDp, maxHeightDp)
                    onAction(WorkbenchAction.ResizeFloatingPanel(id, clampedW, clampedH))
                },
                onMovePanelToGroup = { id, groupId -> onAction(WorkbenchAction.MovePanelToGroup(id, groupId)) },
                onFloatPanelAt = { id, x, y -> onAction(WorkbenchAction.FloatPanelAt(id, x, y)) },
                tabGroupHitAreas = tabGroupHitAreas,
                onDragUpdate = { newState ->
                    dragState = newState
                },
                onDragEnd = {
                    dragState = WorkbenchDragState.Idle
                },
                onDragCancel = {
                    dragState = WorkbenchDragState.Idle
                },
                maxWidthDp = maxWidthDp,
                maxHeightDp = maxHeightDp,
                modifier = Modifier.fillMaxSize(),
                panelContent = panelContent,
            )

            if (presentationState.overlayPanelIds.isNotEmpty()) {
                val overlayExpanded = (leftExpanded + rightExpanded + bottomExpanded)
                    .filter { it.id in presentationState.overlayPanelIds }
                    .filter { it.zone != DockZone.Floating }
                if (overlayExpanded.isNotEmpty()) {
                    val activeOverlayId = presentationState.activeOverlayPanelId ?: overlayExpanded.first().id
                    val activePanel = overlayExpanded.find { it.id == activeOverlayId } ?: overlayExpanded.first()
                    val overlayWidth = (layoutState.dockZoneSizeDp[activePanel.zone] ?: activePanel.sizeDp).dp.coerceIn(WorkbenchReducer.SIDE_PANEL_MIN_DP.dp, WorkbenchReducer.SIDE_PANEL_MAX_DP.dp)
                    val (overlayAlignment, overlayModifier) = when (activePanel.zone) {
                        DockZone.Left -> Alignment.CenterStart to Modifier.width(overlayWidth).fillMaxHeight()
                        DockZone.Bottom -> Alignment.BottomCenter to Modifier.fillMaxWidth().height((layoutState.dockZoneSizeDp[DockZone.Bottom] ?: WorkbenchReducer.BOTTOM_PANEL_MIN_DP).dp.coerceIn(WorkbenchReducer.BOTTOM_PANEL_MIN_DP.dp, 400.dp))
                        else -> Alignment.CenterEnd to Modifier.width(overlayWidth).fillMaxHeight()
                    }
                    Surface(
                        modifier = Modifier
                            .align(overlayAlignment)
                            .then(overlayModifier),
                        tonalElevation = 4.dp,
                        shadowElevation = 8.dp,
                        shape = MaterialTheme.shapes.large,
                        color = MaterialTheme.colorScheme.surface,
                    ) {
                        Column(modifier = Modifier.fillMaxSize()) {
                            if (presentationState.overlayPanelIds.size > 1) {
                                OverlayTabStrip(
                                    panelIds = presentationState.overlayPanelIds,
                                    activeId = activeOverlayId,
                                    allExpanded = overlayExpanded,
                                    onSwitch = { id ->
                                        onAction(WorkbenchAction.ActivateOverlayPanel(id))
                                    },
                                    modifier = Modifier.fillMaxWidth(),
                                )
                            }
                            WorkbenchPanelFrame(
                                panelState = activePanel,
                                onFloat = { onAction(WorkbenchAction.FloatPanel(activePanel.id)) },
                                onCollapse = { onAction(WorkbenchAction.CollapsePanel(activePanel.id)) },
                                onClose = { onAction(WorkbenchAction.HidePanel(activePanel.id)) },
                                modifier = Modifier.weight(1f),
                            ) {
                                panelContent(activePanel)
                            }
                        }
                    }
                }
            }

            if (dragState.isDragging) {
                WorkbenchDragOverlay(
                    dragState = dragState,
                    maxWidthDp = maxWidthDp,
                    maxHeightDp = maxHeightDp,
                    modifier = Modifier.fillMaxSize(),
                )
            }
        }
    }
}

@Composable
private fun OverlayTabStrip(
    panelIds: List<WorkbenchPanelId>,
    activeId: WorkbenchPanelId,
    allExpanded: List<WorkbenchPanelState>,
    onSwitch: (WorkbenchPanelId) -> Unit,
    modifier: Modifier = Modifier,
) {
    Surface(
        color = MaterialTheme.colorScheme.surfaceVariant,
        tonalElevation = 2.dp,
        modifier = modifier,
    ) {
        Row(
            modifier = Modifier.fillMaxWidth().height(36.dp).padding(horizontal = 8.dp),
        ) {
            for (id in panelIds) {
                val isActive = id == activeId
                val icon = panelIconForId(id)
                Row(
                    modifier = Modifier
                        .clickable(
                            interactionSource = remember { MutableInteractionSource() },
                            indication = null,
                            onClick = { onSwitch(id) },
                        )
                        .padding(horizontal = 8.dp, vertical = 4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    if (icon != null) {
                        Icon(
                            imageVector = icon,
                            contentDescription = null,
                            modifier = Modifier.width(14.dp).height(14.dp),
                            tint = if (isActive) MaterialTheme.colorScheme.primary
                            else MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    Text(
                        text = panelTitleForId(id),
                        style = MaterialTheme.typography.labelSmall,
                        color = if (isActive) MaterialTheme.colorScheme.primary
                        else MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
    }
}
