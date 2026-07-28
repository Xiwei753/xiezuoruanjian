package com.xiwei.sujian.ui.compose.workbench.component

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
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
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.designsystem.theme.LocalSujianDimensions
import com.xiwei.sujian.ui.compose.workbench.model.DockZone
import com.xiwei.sujian.ui.compose.workbench.model.PanelVisibility
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchAction
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchLayoutState
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelId
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelState

private const val SIDE_PANEL_MIN_DP = 280f
private const val SIDE_PANEL_MAX_DP = 520f
private const val OVERLAY_THRESHOLD_DP = 840
private const val DUAL_SIDE_THRESHOLD_DP = 1200

@Composable
fun SujianWorkbench(
    layoutState: WorkbenchLayoutState,
    onAction: (WorkbenchAction) -> Unit,
    modifier: Modifier = Modifier,
    editorContent: @Composable () -> Unit,
    panelContent: @Composable (WorkbenchPanelState) -> Unit,
) {
    val dims = LocalSujianDimensions.current

    BoxWithConstraints(modifier = modifier.fillMaxSize()) {
        val maxWidthDp = maxWidth.value

        val isOverlayMode = maxWidthDp < OVERLAY_THRESHOLD_DP
        val allowDualSide = maxWidthDp >= DUAL_SIDE_THRESHOLD_DP

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

        val leftCollapsed = leftPanels.filter { it.visibility == PanelVisibility.Collapsed || it.visibility == PanelVisibility.Hidden }
        val rightCollapsed = rightPanels.filter { it.visibility == PanelVisibility.Collapsed || it.visibility == PanelVisibility.Hidden }
        val bottomCollapsed = bottomPanels.filter { it.visibility == PanelVisibility.Collapsed || it.visibility == PanelVisibility.Hidden }

        Box(modifier = Modifier.fillMaxSize()) {
            Row(modifier = Modifier.fillMaxSize()) {
                if (leftCollapsed.isNotEmpty()) {
                    PanelLauncherRail(
                        zone = DockZone.Left,
                        panels = leftCollapsed,
                        onTogglePanel = { onAction(WorkbenchAction.TogglePanel(it)) },
                        modifier = Modifier.width(56.dp).fillMaxHeight(),
                    )
                }

                if (leftExpanded.isNotEmpty() && !isOverlayMode) {
                    DockHost(
                        zone = DockZone.Left,
                        panels = leftExpanded,
                        activeTabByGroup = layoutState.activeTabByGroup,
                        onFloat = { onAction(WorkbenchAction.FloatPanel(it)) },
                        onCollapse = { onAction(WorkbenchAction.CollapsePanel(it)) },
                        onHide = { onAction(WorkbenchAction.HidePanel(it)) },
                        onActivateTab = { g, p -> onAction(WorkbenchAction.ActivateTab(g, p)) },
                        modifier = Modifier.fillMaxHeight(),
                        panelContent = panelContent,
                    )
                    DockResizeHandle(
                        zone = DockZone.Left,
                        panelId = leftExpanded.first().id,
                        currentSizeDp = leftExpanded.first().sizeDp,
                        onResize = { id, size -> onAction(WorkbenchAction.ResizePanel(id, size, maxWidthDp)) },
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

                if (rightExpanded.isNotEmpty() && !isOverlayMode && (allowDualSide || leftExpanded.isEmpty())) {
                    DockResizeHandle(
                        zone = DockZone.Right,
                        panelId = rightExpanded.first().id,
                        currentSizeDp = rightExpanded.first().sizeDp,
                        onResize = { id, size -> onAction(WorkbenchAction.ResizePanel(id, size, maxWidthDp)) },
                        modifier = Modifier.fillMaxHeight(),
                    )
                    DockHost(
                        zone = DockZone.Right,
                        panels = rightExpanded,
                        activeTabByGroup = layoutState.activeTabByGroup,
                        onFloat = { onAction(WorkbenchAction.FloatPanel(it)) },
                        onCollapse = { onAction(WorkbenchAction.CollapsePanel(it)) },
                        onHide = { onAction(WorkbenchAction.HidePanel(it)) },
                        onActivateTab = { g, p -> onAction(WorkbenchAction.ActivateTab(g, p)) },
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

            if (bottomExpanded.isNotEmpty() && !isOverlayMode) {
                DockHost(
                    zone = DockZone.Bottom,
                    panels = bottomExpanded,
                    activeTabByGroup = layoutState.activeTabByGroup,
                    onFloat = { onAction(WorkbenchAction.FloatPanel(it)) },
                    onCollapse = { onAction(WorkbenchAction.CollapsePanel(it)) },
                    onHide = { onAction(WorkbenchAction.HidePanel(it)) },
                    onActivateTab = { g, p -> onAction(WorkbenchAction.ActivateTab(g, p)) },
                    modifier = Modifier.align(Alignment.BottomCenter).fillMaxWidth(),
                    panelContent = panelContent,
                )
            }

            if (bottomCollapsed.isNotEmpty()) {
                PanelLauncherRail(
                    zone = DockZone.Bottom,
                    panels = bottomCollapsed,
                    onTogglePanel = { onAction(WorkbenchAction.TogglePanel(it)) },
                    modifier = Modifier.align(Alignment.BottomCenter).fillMaxWidth(),
                )
            }

            FloatingPanelLayer(
                panels = layoutState.panels.values.toList(),
                onFloat = { onAction(WorkbenchAction.FloatPanel(it)) },
                onCollapse = { onAction(WorkbenchAction.CollapsePanel(it)) },
                onHide = { onAction(WorkbenchAction.HidePanel(it)) },
                onMoveFloating = { id, x, y -> onAction(WorkbenchAction.MoveFloatingPanel(id, x, y)) },
                modifier = Modifier.fillMaxSize(),
                panelContent = panelContent,
            )

            if (isOverlayMode) {
                val allExpanded = (leftExpanded + rightExpanded + bottomExpanded)
                    .filter { it.zone != DockZone.Floating }
                if (allExpanded.isNotEmpty()) {
                    val panel = allExpanded.first()
                    Surface(
                        modifier = Modifier
                            .align(Alignment.CenterEnd)
                            .width(panel.sizeDp.dp.coerceIn(SIDE_PANEL_MIN_DP.dp, SIDE_PANEL_MAX_DP.dp))
                            .fillMaxHeight(),
                        tonalElevation = 4.dp,
                        shadowElevation = 8.dp,
                        shape = MaterialTheme.shapes.large,
                        color = MaterialTheme.colorScheme.surface,
                    ) {
                        WorkbenchPanelFrame(
                            panelState = panel,
                            onFloat = { onAction(WorkbenchAction.FloatPanel(panel.id)) },
                            onCollapse = { onAction(WorkbenchAction.CollapsePanel(panel.id)) },
                            onClose = { onAction(WorkbenchAction.HidePanel(panel.id)) },
                            modifier = Modifier.fillMaxSize(),
                        ) {
                            panelContent(panel)
                        }
                    }
                }
            }
        }
    }
}
