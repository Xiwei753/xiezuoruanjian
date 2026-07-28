package com.xiwei.sujian.ui.compose.workbench.model

sealed class WorkbenchAction {
    data class TogglePanel(val panelId: WorkbenchPanelId) : WorkbenchAction()
    data class ExpandPanel(val panelId: WorkbenchPanelId) : WorkbenchAction()
    data class CollapsePanel(val panelId: WorkbenchPanelId) : WorkbenchAction()
    data class HidePanel(val panelId: WorkbenchPanelId) : WorkbenchAction()
    data class MovePanel(val panelId: WorkbenchPanelId, val zone: DockZone) : WorkbenchAction()
    data class ResizePanel(val panelId: WorkbenchPanelId, val sizeDp: Float) : WorkbenchAction()
    data class ActivateTab(val tabGroupId: String, val panelId: WorkbenchPanelId) : WorkbenchAction()
    data class FloatPanel(val panelId: WorkbenchPanelId) : WorkbenchAction()
    data class DockPanel(val panelId: WorkbenchPanelId, val zone: DockZone) : WorkbenchAction()
    data class MoveFloatingPanel(val panelId: WorkbenchPanelId, val x: Float, val y: Float) : WorkbenchAction()
    data class ApplyPreset(val preset: WorkbenchPreset) : WorkbenchAction()
    data object RestoreLayout : WorkbenchAction()
}
