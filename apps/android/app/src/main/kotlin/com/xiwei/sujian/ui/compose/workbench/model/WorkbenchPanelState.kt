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
    val floatingZIndex: Int = 0,
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
    val nextFloatingZIndex: Int = 1,
) {
    fun dockGroupsByZone(zone: DockZone): List<DockGroupState> {
        val panelsInZone = panels.values
            .filter { it.zone == zone && it.visibility == PanelVisibility.Expanded }
        return panelsInZone
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
    }
}
