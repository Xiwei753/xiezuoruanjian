package com.xiwei.sujian.ui.compose.workbench.model

data class TabGroupHitArea(
    val groupId: String,
    val left: Float,
    val top: Float,
    val right: Float,
    val bottom: Float,
) {
    fun contains(x: Float, y: Float): Boolean = x in left..right && y in top..bottom
}

enum class DragDropTarget {
    None,
    DockLeft,
    DockRight,
    DockBottom,
    TabGroup,
    FloatArea,
}

data class WorkbenchDragState(
    val isDragging: Boolean = false,
    val draggedPanelId: WorkbenchPanelId? = null,
    val pointerX: Float = 0f,
    val pointerY: Float = 0f,
    val dropTarget: DragDropTarget = DragDropTarget.None,
    val targetTabGroupId: String? = null,
    val tabGroupHitAreas: List<TabGroupHitArea> = emptyList(),
) {
    companion object {
        val Idle = WorkbenchDragState()
    }

    fun resolveDropTarget(maxWidthDp: Float, maxHeightDp: Float): Pair<DragDropTarget, String?> {
        for (area in tabGroupHitAreas) {
            if (area.contains(pointerX, pointerY)) {
                return DragDropTarget.TabGroup to area.groupId
            }
        }
        val dockMargin = 72f
        if (pointerX < dockMargin) return DragDropTarget.DockLeft to null
        if (pointerX > maxWidthDp - dockMargin) return DragDropTarget.DockRight to null
        if (pointerY > maxHeightDp - dockMargin) return DragDropTarget.DockBottom to null
        return DragDropTarget.None to null
    }
}
