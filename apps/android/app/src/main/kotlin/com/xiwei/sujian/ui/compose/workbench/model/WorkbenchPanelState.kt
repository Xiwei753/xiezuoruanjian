package com.xiwei.sujian.ui.compose.workbench.model

enum class DockZone { Left, Right, Bottom, Floating }

enum class PanelVisibility { Hidden, Collapsed, Expanded }

data class WorkbenchPanelState(
    val id: WorkbenchPanelId,
    val zone: DockZone,
    val visibility: PanelVisibility,
    val sizeDp: Float,
    val tabGroupId: String,
    val order: Int,
    val floatingX: Float = 0f,
    val floatingY: Float = 0f,
    val floatingWidthDp: Float = 420f,
    val floatingHeightDp: Float = 560f,
)

enum class WorkbenchPreset {
    FocusWriting,
    ChapterWriting,
    AiWriting,
    ResearchWriting,
    Custom,
}

data class WorkbenchLayoutState(
    val panels: Map<WorkbenchPanelId, WorkbenchPanelState>,
    val activeTabByGroup: Map<String, WorkbenchPanelId>,
    val preset: WorkbenchPreset,
)
