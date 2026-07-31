package com.xiwei.sujian.ui.compose.workbench.model

data class DockGroupState(
    val id: String,
    val zone: DockZone,
    val order: Int,
    val weight: Float = 1f,
    val activePanelId: WorkbenchPanelId? = null,
    val panelIds: List<WorkbenchPanelId> = emptyList(),
)
