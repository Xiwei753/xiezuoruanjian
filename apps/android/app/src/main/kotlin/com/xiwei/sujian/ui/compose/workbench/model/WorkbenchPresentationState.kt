package com.xiwei.sujian.ui.compose.workbench.model

data class WorkbenchPresentationState(
    val overlayPanelIds: List<WorkbenchPanelId> = emptyList(),
    val activeOverlayPanelId: WorkbenchPanelId? = null,
    val isOverlayMode: Boolean = false,
)
