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
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInWindow
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.dp
import androidx.compose.ui.zIndex
import com.xiwei.sujian.ui.compose.workbench.model.DockZone
import com.xiwei.sujian.ui.compose.workbench.model.DragDropTarget
import com.xiwei.sujian.ui.compose.workbench.model.PanelVisibility
import com.xiwei.sujian.ui.compose.workbench.model.TabGroupHitArea
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchDragState
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelId
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelState
import com.xiwei.sujian.ui.compose.workbench.state.WorkbenchReducer

@Composable
fun FloatingPanelLayer(
    panels: List<WorkbenchPanelState>,
    onFloat: (WorkbenchPanelId) -> Unit,
    onCollapse: (WorkbenchPanelId) -> Unit,
    onHide: (WorkbenchPanelId) -> Unit,
    onMoveFloating: (WorkbenchPanelId, Float, Float) -> Unit,
    onDock: (WorkbenchPanelId, DockZone) -> Unit,
    onBringToFront: (WorkbenchPanelId) -> Unit,
    onResizeFloating: (WorkbenchPanelId, Float, Float) -> Unit,
    onMovePanelToGroup: (WorkbenchPanelId, String) -> Unit,
    onFloatPanelAt: (WorkbenchPanelId, Float, Float) -> Unit,
    tabGroupHitAreas: List<TabGroupHitArea>,
    onDragUpdate: ((WorkbenchDragState) -> Unit)? = null,
    onDragEnd: (() -> Unit)? = null,
    onDragCancel: (() -> Unit)? = null,
    maxWidthDp: Float,
    maxHeightDp: Float,
    modifier: Modifier = Modifier,
    panelContent: @Composable (WorkbenchPanelState) -> Unit,
) {
    val floatingPanels = panels.filter { it.zone == DockZone.Floating && it.visibility == PanelVisibility.Expanded }
    if (floatingPanels.isEmpty()) return

    Box(modifier = modifier.fillMaxSize()) {
        val sortedByZ = floatingPanels.sortedBy { it.floatingZIndex }
        for ((index, panel) in sortedByZ.withIndex()) {
            val density = LocalDensity.current
            var dragOffsetX by remember(panel.id) { mutableFloatStateOf(0f) }
            var dragOffsetY by remember(panel.id) { mutableFloatStateOf(0f) }
            var startPointerOffsetX by remember(panel.id) { mutableFloatStateOf(0f) }
            var startPointerOffsetY by remember(panel.id) { mutableFloatStateOf(0f) }
            var resizeOffsetW by remember(panel.id) { mutableFloatStateOf(0f) }
            var resizeOffsetH by remember(panel.id) { mutableFloatStateOf(0f) }
            var titleBarRootXDp by remember(panel.id) { mutableFloatStateOf(0f) }
            var titleBarRootYDp by remember(panel.id) { mutableFloatStateOf(0f) }

            val clampedWidth = (panel.floatingWidthDp + resizeOffsetW)
                .coerceIn(200f, maxWidthDp)
            val clampedHeight = (panel.floatingHeightDp + resizeOffsetH)
                .coerceIn(150f, maxHeightDp)

            Surface(
                modifier = Modifier
                    .offset(
                        x = (panel.floatingX + dragOffsetX).dp,
                        y = (panel.floatingY + dragOffsetY).dp
                    )
                    .size(
                        width = clampedWidth.dp,
                        height = clampedHeight.dp,
                    )
                    .zIndex(panel.floatingZIndex.toFloat()),
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
                        .onGloballyPositioned { coords ->
                            val pos = coords.positionInWindow()
                            titleBarRootXDp = pos.x / density.density
                            titleBarRootYDp = pos.y / density.density
                        }
                        .pointerInput(panel.id) {
                            detectDragGestures(
                                onDragStart = { offset ->
                                    startPointerOffsetX = offset.x / density.density
                                    startPointerOffsetY = offset.y / density.density
                                    onBringToFront(panel.id)
                                },
                                onDragEnd = {
                                    val newPanelX = panel.floatingX + dragOffsetX
                                    val newPanelY = panel.floatingY + dragOffsetY
                                    val pointerX = newPanelX + startPointerOffsetX
                                    val pointerY = newPanelY + startPointerOffsetY
                                    val dragState = WorkbenchDragState(
                                        isDragging = true,
                                        draggedPanelId = panel.id,
                                        pointerX = pointerX,
                                        pointerY = pointerY,
                                        tabGroupHitAreas = tabGroupHitAreas,
                                    )
                                    val (target, groupId) = dragState.resolveDropTarget(maxWidthDp, maxHeightDp)
                                    when (target) {
                                        DragDropTarget.DockLeft -> onDock(panel.id, DockZone.Left)
                                        DragDropTarget.DockRight -> onDock(panel.id, DockZone.Right)
                                        DragDropTarget.DockBottom -> onDock(panel.id, DockZone.Bottom)
                                        DragDropTarget.TabGroup -> {
                                            if (groupId != null) {
                                                onMovePanelToGroup(panel.id, groupId)
                                            } else {
                                                val (cx, cy) = WorkbenchReducer.clampFloatingPosition(
                                                    newPanelX, newPanelY,
                                                    panel.floatingWidthDp, panel.floatingHeightDp,
                                                    maxWidthDp, maxHeightDp,
                                                )
                                                onFloatPanelAt(panel.id, cx, cy)
                                            }
                                        }
                                        else -> {
                                            val (cx, cy) = WorkbenchReducer.clampFloatingPosition(
                                                newPanelX, newPanelY,
                                                panel.floatingWidthDp, panel.floatingHeightDp,
                                                maxWidthDp, maxHeightDp,
                                            )
                                            onFloatPanelAt(panel.id, cx, cy)
                                        }
                                    }
                                    dragOffsetX = 0f
                                    dragOffsetY = 0f
                                    onDragEnd?.invoke()
                                },
                                onDragCancel = {
                                    dragOffsetX = 0f
                                    dragOffsetY = 0f
                                    onDragCancel?.invoke()
                                },
                            ) { change, dragAmount ->
                                val dxDp = dragAmount.x / density.density
                                val dyDp = dragAmount.y / density.density
                                dragOffsetX += dxDp
                                dragOffsetY += dyDp
                                val currentPointerX = panel.floatingX + dragOffsetX + startPointerOffsetX
                                val currentPointerY = panel.floatingY + dragOffsetY + startPointerOffsetY
                                val interimState = WorkbenchDragState(
                                    isDragging = true,
                                    draggedPanelId = panel.id,
                                    pointerX = currentPointerX,
                                    pointerY = currentPointerY,
                                    tabGroupHitAreas = tabGroupHitAreas,
                                )
                                val (target, _) = interimState.resolveDropTarget(maxWidthDp, maxHeightDp)
                                onDragUpdate?.invoke(interimState.copy(dropTarget = target))
                                change.consume()
                            }
                        },
                ) {
                    panelContent(panel)
                }

                Box(
                    modifier = Modifier
                        .align(androidx.compose.ui.Alignment.BottomEnd)
                        .width(16.dp)
                        .height(16.dp)
                        .pointerInput(panel.id) {
                            detectDragGestures(
                                onDragEnd = {
                                    val newW = (panel.floatingWidthDp + resizeOffsetW)
                                        .coerceIn(200f, maxWidthDp)
                                    val newH = (panel.floatingHeightDp + resizeOffsetH)
                                        .coerceIn(150f, maxHeightDp)
                                    onResizeFloating(panel.id, newW, newH)
                                    resizeOffsetW = 0f
                                    resizeOffsetH = 0f
                                },
                                onDragCancel = {
                                    resizeOffsetW = 0f
                                    resizeOffsetH = 0f
                                },
                            ) { change, dragAmount ->
                                resizeOffsetW += dragAmount.x / density.density
                                resizeOffsetH += dragAmount.y / density.density
                                change.consume()
                            }
                        },
                )
            }
        }
    }
}
