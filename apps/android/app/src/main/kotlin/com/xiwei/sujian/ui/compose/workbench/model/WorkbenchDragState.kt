package com.xiwei.sujian.ui.compose.workbench.model

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
) {
    companion object {
        val Idle = WorkbenchDragState()
    }
}
