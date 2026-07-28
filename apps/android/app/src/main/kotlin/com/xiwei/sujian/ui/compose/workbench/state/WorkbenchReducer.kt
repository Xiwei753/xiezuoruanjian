package com.xiwei.sujian.ui.compose.workbench.state

import com.xiwei.sujian.ui.compose.workbench.model.DockZone
import com.xiwei.sujian.ui.compose.workbench.model.PanelVisibility
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchAction
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchLayoutState
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelId
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelState
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPreset
import kotlin.math.max
import kotlin.math.min

object WorkbenchReducer {

    private const val SIDE_PANEL_MIN_DP = 280f
    private const val SIDE_PANEL_MAX_DP = 520f
    private const val BOTTOM_PANEL_MIN_DP = 220f
    private const val EDITOR_MIN_DP = 480f

    fun reduce(state: WorkbenchLayoutState, action: WorkbenchAction): WorkbenchLayoutState {
        return when (action) {
            is WorkbenchAction.TogglePanel -> togglePanel(state, action.panelId)
            is WorkbenchAction.ExpandPanel -> expandPanel(state, action.panelId)
            is WorkbenchAction.CollapsePanel -> collapsePanel(state, action.panelId)
            is WorkbenchAction.HidePanel -> hidePanel(state, action.panelId)
            is WorkbenchAction.MovePanel -> movePanel(state, action.panelId, action.zone)
            is WorkbenchAction.ResizePanel -> resizePanel(state, action.panelId, action.sizeDp)
            is WorkbenchAction.ActivateTab -> activateTab(state, action.tabGroupId, action.panelId)
            is WorkbenchAction.FloatPanel -> floatPanel(state, action.panelId)
            is WorkbenchAction.DockPanel -> dockPanel(state, action.panelId, action.zone)
            is WorkbenchAction.MoveFloatingPanel -> moveFloatingPanel(state, action.panelId, action.x, action.y)
            is WorkbenchAction.ApplyPreset -> applyPreset(state, action.preset)
            is WorkbenchAction.RestoreLayout -> state
        }
    }

    private fun togglePanel(state: WorkbenchLayoutState, panelId: WorkbenchPanelId): WorkbenchLayoutState {
        val panel = state.panels[panelId] ?: return state
        val newVisibility = when (panel.visibility) {
            PanelVisibility.Hidden -> PanelVisibility.Expanded
            PanelVisibility.Collapsed -> PanelVisibility.Expanded
            PanelVisibility.Expanded -> PanelVisibility.Collapsed
        }
        return updatePanel(state, panel.copy(visibility = newVisibility), markCustom = true)
    }

    private fun expandPanel(state: WorkbenchLayoutState, panelId: WorkbenchPanelId): WorkbenchLayoutState {
        val panel = state.panels[panelId] ?: return state
        if (panel.visibility == PanelVisibility.Expanded) return state
        return updatePanel(state, panel.copy(visibility = PanelVisibility.Expanded), markCustom = true)
    }

    private fun collapsePanel(state: WorkbenchLayoutState, panelId: WorkbenchPanelId): WorkbenchLayoutState {
        val panel = state.panels[panelId] ?: return state
        if (panel.visibility == PanelVisibility.Collapsed) return state
        return updatePanel(state, panel.copy(visibility = PanelVisibility.Collapsed), markCustom = true)
    }

    private fun hidePanel(state: WorkbenchLayoutState, panelId: WorkbenchPanelId): WorkbenchLayoutState {
        val panel = state.panels[panelId] ?: return state
        if (panel.visibility == PanelVisibility.Hidden) return state
        return updatePanel(state, panel.copy(visibility = PanelVisibility.Hidden), markCustom = true)
    }

    private fun movePanel(state: WorkbenchLayoutState, panelId: WorkbenchPanelId, zone: DockZone): WorkbenchLayoutState {
        val panel = state.panels[panelId] ?: return state
        return updatePanel(state, panel.copy(zone = zone), markCustom = true)
    }

    private fun resizePanel(state: WorkbenchLayoutState, panelId: WorkbenchPanelId, sizeDp: Float): WorkbenchLayoutState {
        val panel = state.panels[panelId] ?: return state
        val clamped = when (panel.zone) {
            DockZone.Left, DockZone.Right -> sizeDp.coerceIn(SIDE_PANEL_MIN_DP, SIDE_PANEL_MAX_DP)
            DockZone.Bottom -> max(sizeDp, BOTTOM_PANEL_MIN_DP)
            DockZone.Floating -> sizeDp
        }
        return updatePanel(state, panel.copy(sizeDp = clamped), markCustom = true)
    }

    private fun activateTab(state: WorkbenchLayoutState, tabGroupId: String, panelId: WorkbenchPanelId): WorkbenchLayoutState {
        val panel = state.panels[panelId] ?: return state
        if (panel.tabGroupId != tabGroupId) return state
        return state.copy(
            activeTabByGroup = state.activeTabByGroup + (tabGroupId to panelId),
            preset = WorkbenchPreset.Custom
        )
    }

    private fun floatPanel(state: WorkbenchLayoutState, panelId: WorkbenchPanelId): WorkbenchLayoutState {
        val panel = state.panels[panelId] ?: return state
        return updatePanel(state, panel.copy(zone = DockZone.Floating, visibility = PanelVisibility.Expanded), markCustom = true)
    }

    private fun dockPanel(state: WorkbenchLayoutState, panelId: WorkbenchPanelId, zone: DockZone): WorkbenchLayoutState {
        val panel = state.panels[panelId] ?: return state
        return updatePanel(state, panel.copy(zone = zone, visibility = PanelVisibility.Expanded), markCustom = true)
    }

    private fun moveFloatingPanel(state: WorkbenchLayoutState, panelId: WorkbenchPanelId, x: Float, y: Float): WorkbenchLayoutState {
        val panel = state.panels[panelId] ?: return state
        if (panel.zone != DockZone.Floating) return state
        return updatePanel(state, panel.copy(floatingX = x, floatingY = y), markCustom = true)
    }

    private fun applyPreset(state: WorkbenchLayoutState, preset: WorkbenchPreset): WorkbenchLayoutState {
        return when (preset) {
            WorkbenchPreset.FocusWriting -> focusWritingPreset(state)
            WorkbenchPreset.ChapterWriting -> chapterWritingPreset(state)
            WorkbenchPreset.AiWriting -> aiWritingPreset(state)
            WorkbenchPreset.ResearchWriting -> researchWritingPreset(state)
            WorkbenchPreset.Custom -> state
        }
    }

    private fun updatePanel(state: WorkbenchLayoutState, panel: WorkbenchPanelState, markCustom: Boolean): WorkbenchLayoutState {
        return state.copy(
            panels = state.panels + (panel.id to panel),
            preset = if (markCustom) WorkbenchPreset.Custom else state.preset
        )
    }

    private fun focusWritingPreset(state: WorkbenchLayoutState): WorkbenchLayoutState {
        return state.copy(
            panels = state.panels.mapValues { (_, panel) ->
                panel.copy(visibility = PanelVisibility.Collapsed)
            },
            preset = WorkbenchPreset.FocusWriting
        )
    }

    private fun chapterWritingPreset(state: WorkbenchLayoutState): WorkbenchLayoutState {
        return state.copy(
            panels = state.panels.mapValues { (id, panel) ->
                when (id) {
                    WorkbenchPanelId.ChapterNavigator -> panel.copy(
                        zone = DockZone.Left,
                        visibility = PanelVisibility.Expanded,
                        sizeDp = 320f
                    )
                    else -> panel.copy(visibility = PanelVisibility.Collapsed)
                }
            },
            activeTabByGroup = state.activeTabByGroup,
            preset = WorkbenchPreset.ChapterWriting
        )
    }

    private fun aiWritingPreset(state: WorkbenchLayoutState): WorkbenchLayoutState {
        return state.copy(
            panels = state.panels.mapValues { (id, panel) ->
                when (id) {
                    WorkbenchPanelId.AiAssistant -> panel.copy(
                        zone = DockZone.Right,
                        visibility = PanelVisibility.Expanded,
                        sizeDp = 400f
                    )
                    else -> panel.copy(visibility = PanelVisibility.Collapsed)
                }
            },
            activeTabByGroup = state.activeTabByGroup,
            preset = WorkbenchPreset.AiWriting
        )
    }

    private fun researchWritingPreset(state: WorkbenchLayoutState): WorkbenchLayoutState {
        val searchTabGroup = "research-right"
        return state.copy(
            panels = state.panels.mapValues { (id, panel) ->
                when (id) {
                    WorkbenchPanelId.ChapterNavigator -> panel.copy(
                        zone = DockZone.Left,
                        visibility = PanelVisibility.Expanded,
                        sizeDp = 320f
                    )
                    WorkbenchPanelId.Search -> panel.copy(
                        zone = DockZone.Right,
                        visibility = PanelVisibility.Expanded,
                        sizeDp = 380f,
                        tabGroupId = searchTabGroup
                    )
                    WorkbenchPanelId.Statistics -> panel.copy(
                        zone = DockZone.Right,
                        visibility = PanelVisibility.Collapsed,
                        tabGroupId = searchTabGroup
                    )
                    else -> panel.copy(visibility = PanelVisibility.Collapsed)
                }
            },
            activeTabByGroup = state.activeTabByGroup + (searchTabGroup to WorkbenchPanelId.Search),
            preset = WorkbenchPreset.ResearchWriting
        )
    }

    fun computeDefaultLayout(): WorkbenchLayoutState {
        val panels = WorkbenchPanelId.entries.associateWith { id ->
            when (id) {
                WorkbenchPanelId.ProjectNavigator -> WorkbenchPanelState(
                    id = id, zone = DockZone.Left, visibility = PanelVisibility.Collapsed,
                    sizeDp = 320f, tabGroupId = "left-nav", order = 0
                )
                WorkbenchPanelId.ChapterNavigator -> WorkbenchPanelState(
                    id = id, zone = DockZone.Left, visibility = PanelVisibility.Collapsed,
                    sizeDp = 320f, tabGroupId = "left-nav", order = 1
                )
                WorkbenchPanelId.AiAssistant -> WorkbenchPanelState(
                    id = id, zone = DockZone.Right, visibility = PanelVisibility.Collapsed,
                    sizeDp = 400f, tabGroupId = "right-tools", order = 0
                )
                WorkbenchPanelId.Search -> WorkbenchPanelState(
                    id = id, zone = DockZone.Right, visibility = PanelVisibility.Collapsed,
                    sizeDp = 380f, tabGroupId = "right-tools", order = 1
                )
                WorkbenchPanelId.Statistics -> WorkbenchPanelState(
                    id = id, zone = DockZone.Right, visibility = PanelVisibility.Collapsed,
                    sizeDp = 380f, tabGroupId = "right-tools", order = 2
                )
                WorkbenchPanelId.StarMap -> WorkbenchPanelState(
                    id = id, zone = DockZone.Right, visibility = PanelVisibility.Collapsed,
                    sizeDp = 420f, tabGroupId = "right-tools", order = 3
                )
                WorkbenchPanelId.DocumentOutline -> WorkbenchPanelState(
                    id = id, zone = DockZone.Right, visibility = PanelVisibility.Collapsed,
                    sizeDp = 300f, tabGroupId = "right-outline", order = 0
                )
                WorkbenchPanelId.CharacterInfo -> WorkbenchPanelState(
                    id = id, zone = DockZone.Right, visibility = PanelVisibility.Collapsed,
                    sizeDp = 300f, tabGroupId = "right-outline", order = 1
                )
            }
        }
        return WorkbenchLayoutState(
            panels = panels,
            activeTabByGroup = emptyMap(),
            preset = WorkbenchPreset.FocusWriting
        )
    }
}
