package com.xiwei.sujian.ui.compose.workbench.state

import com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta
import com.xiwei.sujian.ui.compose.workbench.model.DockZone
import com.xiwei.sujian.ui.compose.workbench.model.PanelVisibility
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchAction
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchLayoutState
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelId
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelState
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPreset
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPresentationState
import kotlin.math.max
import kotlin.math.min

object WorkbenchReducer {

    private const val SIDE_PANEL_MIN_DP = 280f
    private const val SIDE_PANEL_MAX_DP = 520f
    private const val BOTTOM_PANEL_MIN_DP = 220f
    private const val BOTTOM_PANEL_MAX_RATIO = 0.55f
    private const val EDITOR_MIN_DP = 480f
    private const val FLOATING_MIN_WIDTH_DP = 200f
    private const val FLOATING_MIN_HEIGHT_DP = 150f
    private const val FLOATING_TITLE_BAR_DP = 40f
    private const val GROUP_MIN_DP = 80f

    fun reduce(state: WorkbenchLayoutState, action: WorkbenchAction): WorkbenchLayoutState {
        return when (action) {
            is WorkbenchAction.TogglePanel -> togglePanel(state, action.panelId)
            is WorkbenchAction.ExpandPanel -> expandPanel(state, action.panelId)
            is WorkbenchAction.CollapsePanel -> collapsePanel(state, action.panelId)
            is WorkbenchAction.HidePanel -> hidePanel(state, action.panelId)
            is WorkbenchAction.MovePanel -> movePanel(state, action.panelId, action.zone)
            is WorkbenchAction.ResizePanel -> resizePanel(state, action.panelId, action.sizeDp, action.availableWidthDp)
            is WorkbenchAction.ActivateTab -> activateTab(state, action.tabGroupId, action.panelId)
            is WorkbenchAction.FloatPanel -> floatPanel(state, action.panelId)
            is WorkbenchAction.DockPanel -> dockPanel(state, action.panelId, action.zone)
            is WorkbenchAction.MoveFloatingPanel -> moveFloatingPanel(state, action.panelId, action.x, action.y)
            is WorkbenchAction.ApplyPreset -> applyPreset(state, action.preset)
            is WorkbenchAction.RestoreLayout -> computeDefaultLayout()
            is WorkbenchAction.MovePanelToGroup -> movePanelToGroup(state, action.panelId, action.tabGroupId)
            is WorkbenchAction.CreateDockGroup -> createDockGroup(state, action.groupId, action.zone, action.order)
            is WorkbenchAction.ReorderPanel -> reorderPanel(state, action.panelId, action.newOrder)
            is WorkbenchAction.ReorderDockGroup -> reorderDockGroup(state, action.groupId, action.newOrder)
            is WorkbenchAction.BringFloatingToFront -> bringFloatingToFront(state, action.panelId)
            is WorkbenchAction.ResizeFloatingPanel -> resizeFloatingPanel(state, action.panelId, action.widthDp, action.heightDp)
            is WorkbenchAction.FloatPanelAt -> floatPanelAt(state, action.panelId, action.x, action.y)
            is WorkbenchAction.ActivateOverlayPanel -> activateOverlayPanel(state, action.panelId)
            is WorkbenchAction.ResizePanelDelta -> resizePanelDelta(state, action.panelId, action.deltaDp, action.availableWidthDp)
            is WorkbenchAction.ResizeDockSplit -> resizeDockSplit(state, action.zone, action.beforeGroupId, action.afterGroupId, action.deltaDp, action.availableMainAxisDp)
            is WorkbenchAction.ResizeDockZone -> resizeDockZone(state, action.zone, action.deltaDp, action.availableMainAxisDp)
            is WorkbenchAction.ClampFloatingPanels -> clampFloatingPanels(state, action.maxWidthDp, action.maxHeightDp)
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
        val updatedState = updatePanel(state, panel.copy(visibility = PanelVisibility.Expanded), markCustom = true)
        val syncedDockGroupWeights = if (panel.tabGroupId !in updatedState.dockGroupWeights) {
            updatedState.dockGroupWeights + (panel.tabGroupId to 1f)
        } else {
            updatedState.dockGroupWeights
        }
        val syncedDockGroupMeta = if (panel.tabGroupId !in updatedState.dockGroupMeta) {
            updatedState.dockGroupMeta + (panel.tabGroupId to DockGroupMeta(panel.tabGroupId, panel.zone, 0))
        } else {
            updatedState.dockGroupMeta
        }
        val syncedDockZoneSizeDp = if (panel.zone != DockZone.Floating && updatedState.dockZoneSizeDp[panel.zone] == null) {
            val defaultSize = when (panel.zone) {
                DockZone.Left, DockZone.Right -> SIDE_PANEL_MIN_DP
                DockZone.Bottom -> BOTTOM_PANEL_MIN_DP
                else -> 0f
            }
            updatedState.dockZoneSizeDp + (panel.zone to defaultSize)
        } else {
            updatedState.dockZoneSizeDp
        }
        return updatedState.copy(
            dockGroupWeights = syncedDockGroupWeights,
            dockGroupMeta = syncedDockGroupMeta,
            dockZoneSizeDp = syncedDockZoneSizeDp,
        )
    }

    private fun collapsePanel(state: WorkbenchLayoutState, panelId: WorkbenchPanelId): WorkbenchLayoutState {
        val panel = state.panels[panelId] ?: return state
        if (panel.visibility == PanelVisibility.Collapsed) return state
        return updatePanel(state, panel.copy(visibility = PanelVisibility.Collapsed), markCustom = true)
    }

    private fun hidePanel(state: WorkbenchLayoutState, panelId: WorkbenchPanelId): WorkbenchLayoutState {
        val panel = state.panels[panelId] ?: return state
        if (panel.visibility == PanelVisibility.Hidden) return state
        val newState = updatePanel(state, panel.copy(visibility = PanelVisibility.Hidden), markCustom = true)
        return if (newState.activeOverlayPanelId == panelId) {
            val remaining = newState.panels.values.filter {
                it.visibility == PanelVisibility.Expanded && it.zone != DockZone.Floating
            }
            newState.copy(activeOverlayPanelId = remaining.firstOrNull()?.id)
        } else newState
    }

    private fun movePanel(state: WorkbenchLayoutState, panelId: WorkbenchPanelId, zone: DockZone): WorkbenchLayoutState {
        val panel = state.panels[panelId] ?: return state
        return updatePanel(state, panel.copy(zone = zone), markCustom = true)
    }

    private fun resizePanel(state: WorkbenchLayoutState, panelId: WorkbenchPanelId, sizeDp: Float, availableWidthDp: Float): WorkbenchLayoutState {
        val panel = state.panels[panelId] ?: return state
        val clamped = when (panel.zone) {
            DockZone.Left -> {
                val otherSideWidth = state.actualSideWidthDp(DockZone.Right)
                val maxForEditor = availableWidthDp - EDITOR_MIN_DP - otherSideWidth
                if (maxForEditor < SIDE_PANEL_MIN_DP) return state
                sizeDp.coerceIn(SIDE_PANEL_MIN_DP, min(SIDE_PANEL_MAX_DP, maxForEditor))
            }
            DockZone.Right -> {
                val otherSideWidth = state.actualSideWidthDp(DockZone.Left)
                val maxForEditor = availableWidthDp - EDITOR_MIN_DP - otherSideWidth
                if (maxForEditor < SIDE_PANEL_MIN_DP) return state
                sizeDp.coerceIn(SIDE_PANEL_MIN_DP, min(SIDE_PANEL_MAX_DP, maxForEditor))
            }
            DockZone.Bottom -> {
                val maxBottomDp = availableWidthDp * BOTTOM_PANEL_MAX_RATIO
                sizeDp.coerceIn(BOTTOM_PANEL_MIN_DP, maxBottomDp)
            }
            DockZone.Floating -> sizeDp
        }
        val updatedDockZoneSizeDp = if (panel.zone != DockZone.Floating) {
            state.dockZoneSizeDp + (panel.zone to clamped)
        } else {
            state.dockZoneSizeDp
        }
        return state.copy(
            panels = state.panels + (panel.id to panel.copy(sizeDp = clamped)),
            dockZoneSizeDp = updatedDockZoneSizeDp,
            preset = WorkbenchPreset.Custom
        )
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
        val newZ = state.nextFloatingZIndex
        return state.copy(
            panels = state.panels + (panel.id to panel.copy(
                zone = DockZone.Floating,
                visibility = PanelVisibility.Expanded,
                floatingZIndex = newZ
            )),
            preset = WorkbenchPreset.Custom,
            nextFloatingZIndex = newZ + 1
        )
    }

    private fun floatPanelAt(state: WorkbenchLayoutState, panelId: WorkbenchPanelId, x: Float, y: Float): WorkbenchLayoutState {
        val panel = state.panels[panelId] ?: return state
        val newZ = state.nextFloatingZIndex
        return state.copy(
            panels = state.panels + (panel.id to panel.copy(
                zone = DockZone.Floating,
                visibility = PanelVisibility.Expanded,
                floatingX = x,
                floatingY = y,
                floatingZIndex = newZ
            )),
            preset = WorkbenchPreset.Custom,
            nextFloatingZIndex = newZ + 1
        )
    }

    private fun dockPanel(state: WorkbenchLayoutState, panelId: WorkbenchPanelId, zone: DockZone): WorkbenchLayoutState {
        val panel = state.panels[panelId] ?: return state
        if (panel.zone == zone) return updatePanel(state, panel.copy(visibility = PanelVisibility.Expanded), markCustom = true)
        val existingGroupId = state.panels.values
            .filter { it.zone == zone && it.visibility == PanelVisibility.Expanded }
            .firstOrNull()?.tabGroupId
        val newTabGroupId = existingGroupId ?: "${zone.name.lowercase()}-panel-${panel.id.name}"
        val updatedDockGroupWeights = if (newTabGroupId !in state.dockGroupWeights) {
            state.dockGroupWeights + (newTabGroupId to 1f)
        } else state.dockGroupWeights
        val updatedDockGroupMeta = if (newTabGroupId !in state.dockGroupMeta) {
            state.dockGroupMeta + (newTabGroupId to DockGroupMeta(newTabGroupId, zone, 0))
        } else state.dockGroupMeta
        val updatedDockZoneSizeDp = if (zone != DockZone.Floating && state.dockZoneSizeDp[zone] == null) {
            val defaultSize = when (zone) {
                DockZone.Left, DockZone.Right -> SIDE_PANEL_MIN_DP
                DockZone.Bottom -> BOTTOM_PANEL_MIN_DP
                else -> 0f
            }
            state.dockZoneSizeDp + (zone to defaultSize)
        } else state.dockZoneSizeDp
        return state.copy(
            panels = state.panels + (panel.id to panel.copy(
                zone = zone,
                visibility = PanelVisibility.Expanded,
                tabGroupId = newTabGroupId,
            )),
            dockGroupWeights = updatedDockGroupWeights,
            dockGroupMeta = updatedDockGroupMeta,
            dockZoneSizeDp = updatedDockZoneSizeDp,
            preset = WorkbenchPreset.Custom,
        )
    }

    private fun moveFloatingPanel(state: WorkbenchLayoutState, panelId: WorkbenchPanelId, x: Float, y: Float): WorkbenchLayoutState {
        val panel = state.panels[panelId] ?: return state
        if (panel.zone != DockZone.Floating) return state
        val (cx, cy) = clampFloatingPosition(x, y, panel.floatingWidthDp, panel.floatingHeightDp, Float.MAX_VALUE, Float.MAX_VALUE)
        return updatePanel(state, panel.copy(floatingX = cx, floatingY = cy), markCustom = true)
    }

    private fun movePanelToGroup(state: WorkbenchLayoutState, panelId: WorkbenchPanelId, tabGroupId: String): WorkbenchLayoutState {
        val panel = state.panels[panelId] ?: return state
        val targetGroupZone = state.panels.values
            .filter { it.tabGroupId == tabGroupId && it.visibility == PanelVisibility.Expanded }
            .firstOrNull()?.zone ?: state.dockGroupMeta[tabGroupId]?.zone ?: panel.zone
        val updatedPanel = panel.copy(tabGroupId = tabGroupId, zone = targetGroupZone)
        val newActiveTab = state.activeTabByGroup + (tabGroupId to panelId)
        val newGroupWeights = if (tabGroupId !in state.dockGroupWeights) {
            state.dockGroupWeights + (tabGroupId to 1f)
        } else {
            state.dockGroupWeights
        }
        val newGroupMeta = if (tabGroupId !in state.dockGroupMeta) {
            state.dockGroupMeta + (tabGroupId to DockGroupMeta(tabGroupId, targetGroupZone, 0))
        } else {
            state.dockGroupMeta
        }
        return state.copy(
            panels = state.panels + (panelId to updatedPanel),
            activeTabByGroup = newActiveTab,
            dockGroupWeights = newGroupWeights,
            dockGroupMeta = newGroupMeta,
            preset = WorkbenchPreset.Custom
        )
    }

    private fun createDockGroup(state: WorkbenchLayoutState, groupId: String, zone: DockZone, order: Int): WorkbenchLayoutState {
        val existingGroups = state.dockGroupsByZone(zone)
        val existingIds = existingGroups.map { it.id }
        if (groupId in existingIds) return state
        val newGroupWeights = state.dockGroupWeights + (groupId to 1f)
        val newGroupMeta = state.dockGroupMeta + (groupId to DockGroupMeta(groupId, zone, order))
        val newDockZoneSizeDp = if (zone != DockZone.Floating && state.dockZoneSizeDp[zone] == null) {
            val defaultSize = when (zone) {
                DockZone.Left, DockZone.Right -> SIDE_PANEL_MIN_DP
                DockZone.Bottom -> BOTTOM_PANEL_MIN_DP
                else -> 0f
            }
            state.dockZoneSizeDp + (zone to defaultSize)
        } else state.dockZoneSizeDp
        return state.copy(
            dockGroupWeights = newGroupWeights,
            dockGroupMeta = newGroupMeta,
            dockZoneSizeDp = newDockZoneSizeDp,
            preset = WorkbenchPreset.Custom
        )
    }

    private fun reorderPanel(state: WorkbenchLayoutState, panelId: WorkbenchPanelId, newOrder: Int): WorkbenchLayoutState {
        val panel = state.panels[panelId] ?: return state
        return updatePanel(state, panel.copy(order = newOrder), markCustom = true)
    }

    private fun reorderDockGroup(state: WorkbenchLayoutState, groupId: String, newOrder: Int): WorkbenchLayoutState {
        val panelsInGroup = state.panels.values.filter { it.tabGroupId == groupId }
        if (panelsInGroup.isEmpty()) return state
        val updatedPanels = state.panels.toMutableMap()
        for (p in panelsInGroup) {
            updatedPanels[p.id] = p.copy(order = newOrder + (p.order - panelsInGroup.minOf { it.order }))
        }
        return state.copy(panels = updatedPanels, preset = WorkbenchPreset.Custom)
    }

    private fun bringFloatingToFront(state: WorkbenchLayoutState, panelId: WorkbenchPanelId): WorkbenchLayoutState {
        val panel = state.panels[panelId] ?: return state
        if (panel.zone != DockZone.Floating) return state
        val newZ = state.nextFloatingZIndex
        return state.copy(
            panels = state.panels + (panelId to panel.copy(floatingZIndex = newZ)),
            nextFloatingZIndex = newZ + 1
        )
    }

    private fun resizeFloatingPanel(state: WorkbenchLayoutState, panelId: WorkbenchPanelId, widthDp: Float, heightDp: Float): WorkbenchLayoutState {
        val panel = state.panels[panelId] ?: return state
        if (panel.zone != DockZone.Floating) return state
        val clampedWidth = max(widthDp, FLOATING_MIN_WIDTH_DP)
        val clampedHeight = max(heightDp, FLOATING_MIN_HEIGHT_DP)
        return updatePanel(state, panel.copy(floatingWidthDp = clampedWidth, floatingHeightDp = clampedHeight), markCustom = true)
    }

    private fun activateOverlayPanel(state: WorkbenchLayoutState, panelId: WorkbenchPanelId): WorkbenchLayoutState {
        val panel = state.panels[panelId] ?: return state
        if (panel.visibility != PanelVisibility.Expanded) return state
        return state.copy(
            activeOverlayPanelId = panelId,
            preset = WorkbenchPreset.Custom
        )
    }

    private fun resizePanelDelta(state: WorkbenchLayoutState, panelId: WorkbenchPanelId, deltaDp: Float, availableWidthDp: Float): WorkbenchLayoutState {
        val panel = state.panels[panelId] ?: return state
        return resizePanel(state, panelId, panel.sizeDp + deltaDp, availableWidthDp)
    }

    private fun resizeDockSplit(
        state: WorkbenchLayoutState,
        zone: DockZone,
        beforeGroupId: String,
        afterGroupId: String,
        deltaDp: Float,
        availableMainAxisDp: Float,
    ): WorkbenchLayoutState {
        val beforeWeight = state.dockGroupWeights[beforeGroupId] ?: return state
        val afterWeight = state.dockGroupWeights[afterGroupId] ?: return state
        if (availableMainAxisDp <= 0f) return state
        val deltaWeight = deltaDp / availableMainAxisDp
        val minWeight = GROUP_MIN_DP / availableMainAxisDp
        val totalWeight = beforeWeight + afterWeight
        var newBefore = beforeWeight + deltaWeight
        var newAfter = afterWeight - deltaWeight
        if (newBefore < minWeight) {
            newAfter = totalWeight - minWeight
            newBefore = minWeight
        }
        if (newAfter < minWeight) {
            newBefore = totalWeight - minWeight
            newAfter = minWeight
        }
        return state.copy(
            dockGroupWeights = state.dockGroupWeights
                + (beforeGroupId to newBefore)
                + (afterGroupId to newAfter),
            preset = WorkbenchPreset.Custom
        )
    }

    private fun resizeDockZone(state: WorkbenchLayoutState, zone: DockZone, deltaDp: Float, availableMainAxisDp: Float): WorkbenchLayoutState {
        val groups = state.dockGroupsByZone(zone)
        if (groups.isEmpty()) return state
        val currentSize = state.dockZoneSizeDp[zone] ?: when (zone) {
            DockZone.Left, DockZone.Right -> SIDE_PANEL_MIN_DP
            DockZone.Bottom -> BOTTOM_PANEL_MIN_DP
            else -> return state
        }
        val newSize = currentSize + deltaDp
        val otherZone = when (zone) {
            DockZone.Left -> DockZone.Right
            DockZone.Right -> DockZone.Left
            else -> null
        }
        val clampedSize = when (zone) {
            DockZone.Left, DockZone.Right -> {
                val otherSideWidth = if (otherZone != null) state.actualSideWidthDp(otherZone) else 0f
                val maxForEditor = availableMainAxisDp - EDITOR_MIN_DP - otherSideWidth
                if (maxForEditor < SIDE_PANEL_MIN_DP) return state
                newSize.coerceIn(SIDE_PANEL_MIN_DP, min(SIDE_PANEL_MAX_DP, maxForEditor))
            }
            DockZone.Bottom -> {
                val maxBottomDp = availableMainAxisDp * BOTTOM_PANEL_MAX_RATIO
                newSize.coerceIn(BOTTOM_PANEL_MIN_DP, maxBottomDp)
            }
            DockZone.Floating -> return state
        }
        return state.copy(
            dockZoneSizeDp = state.dockZoneSizeDp + (zone to clampedSize),
            preset = WorkbenchPreset.Custom,
        )
    }

    private fun clampFloatingPanels(state: WorkbenchLayoutState, maxWidthDp: Float, maxHeightDp: Float): WorkbenchLayoutState {
        val updatedPanels = state.panels.toMutableMap()
        for ((id, panel) in state.panels) {
            if (panel.zone != DockZone.Floating || panel.visibility != PanelVisibility.Expanded) continue
            val clampedW = panel.floatingWidthDp.coerceIn(FLOATING_MIN_WIDTH_DP, maxWidthDp)
            val clampedH = panel.floatingHeightDp.coerceIn(FLOATING_MIN_HEIGHT_DP, maxHeightDp)
            val (clampedX, clampedY) = clampFloatingPosition(
                panel.floatingX, panel.floatingY,
                clampedW, clampedH,
                maxWidthDp, maxHeightDp,
            )
            updatedPanels[id] = panel.copy(
                floatingX = clampedX,
                floatingY = clampedY,
                floatingWidthDp = clampedW,
                floatingHeightDp = clampedH,
            )
        }
        return state.copy(panels = updatedPanels)
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
        val updatedPanels = state.panels.mapValues { (id, panel) ->
            when (id) {
                WorkbenchPanelId.ChapterNavigator -> panel.copy(
                    zone = DockZone.Left,
                    visibility = PanelVisibility.Expanded,
                    sizeDp = 320f
                )
                else -> panel.copy(visibility = PanelVisibility.Collapsed)
            }
        }
        return state.copy(
            panels = updatedPanels,
            activeTabByGroup = state.activeTabByGroup,
            dockZoneSizeDp = state.dockZoneSizeDp + (DockZone.Left to 320f),
            dockGroupWeights = state.dockGroupWeights + ("left-nav" to 1f),
            dockGroupMeta = state.dockGroupMeta + ("left-nav" to DockGroupMeta("left-nav", DockZone.Left, 0)),
            preset = WorkbenchPreset.ChapterWriting
        )
    }

    private fun aiWritingPreset(state: WorkbenchLayoutState): WorkbenchLayoutState {
        val updatedPanels = state.panels.mapValues { (id, panel) ->
            when (id) {
                WorkbenchPanelId.AiAssistant -> panel.copy(
                    zone = DockZone.Right,
                    visibility = PanelVisibility.Expanded,
                    sizeDp = 400f
                )
                else -> panel.copy(visibility = PanelVisibility.Collapsed)
            }
        }
        return state.copy(
            panels = updatedPanels,
            activeTabByGroup = state.activeTabByGroup,
            dockZoneSizeDp = state.dockZoneSizeDp + (DockZone.Right to 400f),
            dockGroupWeights = state.dockGroupWeights + ("right-tools" to 1f),
            dockGroupMeta = state.dockGroupMeta + ("right-tools" to DockGroupMeta("right-tools", DockZone.Right, 0)),
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
            dockZoneSizeDp = state.dockZoneSizeDp + (DockZone.Left to 320f) + (DockZone.Right to 380f),
            dockGroupWeights = state.dockGroupWeights
                + ("left-nav" to 1f)
                + (searchTabGroup to 1f),
            dockGroupMeta = state.dockGroupMeta +
                ("left-nav" to DockGroupMeta("left-nav", DockZone.Left, 0)) +
                (searchTabGroup to DockGroupMeta(searchTabGroup, DockZone.Right, 0)),
            preset = WorkbenchPreset.ResearchWriting
        )
    }

    fun computePresentationState(
        state: WorkbenchLayoutState,
        maxWidthDp: Float,
        maxHeightDp: Float,
    ): WorkbenchPresentationState {
        val leftExpanded = state.panels.values.filter {
            it.zone == DockZone.Left && it.visibility == PanelVisibility.Expanded
        }
        val rightExpanded = state.panels.values.filter {
            it.zone == DockZone.Right && it.visibility == PanelVisibility.Expanded
        }
        val bottomExpanded = state.panels.values.filter {
            it.zone == DockZone.Bottom && it.visibility == PanelVisibility.Expanded
        }

        val overlayIds: List<WorkbenchPanelId>
        val isOverlayMode: Boolean

        when {
            maxWidthDp < 840 -> {
                val allExpanded = leftExpanded + rightExpanded + bottomExpanded
                overlayIds = allExpanded.map { it.id }
                isOverlayMode = true
            }
            maxWidthDp < 1200 -> {
                if (leftExpanded.isNotEmpty() && rightExpanded.isNotEmpty()) {
                    overlayIds = rightExpanded.map { it.id }
                    isOverlayMode = false
                } else {
                    overlayIds = emptyList()
                    isOverlayMode = false
                }
            }
            else -> {
                overlayIds = emptyList()
                isOverlayMode = false
            }
        }

        if (overlayIds.isEmpty()) {
            return WorkbenchPresentationState(isOverlayMode = isOverlayMode)
        }

        val allExpanded = leftExpanded + rightExpanded + bottomExpanded
        val preferredActive = state.activeOverlayPanelId
        val activeId = when {
            preferredActive != null && preferredActive in overlayIds -> preferredActive
            else -> {
                val firstOverlay = allExpanded.firstOrNull { it.id in overlayIds }
                firstOverlay?.id ?: overlayIds.first()
            }
        }

        return WorkbenchPresentationState(
            overlayPanelIds = overlayIds,
            activeOverlayPanelId = activeId,
            isOverlayMode = isOverlayMode,
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
            preset = WorkbenchPreset.FocusWriting,
            dockZoneSizeDp = mapOf(
                DockZone.Left to 320f,
                DockZone.Right to 400f,
            ),
            dockGroupWeights = mapOf(
                "left-nav" to 1f,
                "right-tools" to 1f,
                "right-outline" to 1f,
            ),
        )
    }

    fun migrateFromV1(
        panels: Map<WorkbenchPanelId, WorkbenchPanelState>,
        activeTabByGroup: Map<String, WorkbenchPanelId>,
        preset: WorkbenchPreset,
        dockGroupSizes: Map<String, Float>,
        activeOverlayPanelId: WorkbenchPanelId?,
    ): WorkbenchLayoutState {
        val dockZoneSizeDp = mutableMapOf<DockZone, Float>()
        val dockGroupWeights = mutableMapOf<String, Float>()
        val dockGroupMeta = mutableMapOf<String, DockGroupMeta>()
        for ((groupId, size) in dockGroupSizes) {
            val groupPanels = panels.values.filter { it.tabGroupId == groupId }
            val zone = groupPanels.firstOrNull()?.zone ?: DockZone.Left
            val order = groupPanels.minOfOrNull { it.order } ?: 0
            dockGroupMeta[groupId] = DockGroupMeta(groupId, zone, order)
            dockGroupWeights[groupId] = 1f
            when (zone) {
                DockZone.Left, DockZone.Right -> {
                    val existing = dockZoneSizeDp[zone]
                    if (existing == null || size > existing) {
                        dockZoneSizeDp[zone] = size
                    }
                }
                DockZone.Bottom -> {
                    val existing = dockZoneSizeDp[zone]
                    if (existing == null || size > existing) {
                        dockZoneSizeDp[zone] = size
                    }
                }
                else -> {}
            }
        }
        for (panel in panels.values) {
            if (panel.tabGroupId.isNotEmpty() && panel.tabGroupId !in dockGroupMeta) {
                dockGroupMeta[panel.tabGroupId] = DockGroupMeta(panel.tabGroupId, panel.zone, panel.order)
                dockGroupWeights[panel.tabGroupId] = 1f
            }
        }
        val maxZIndex = panels.values
            .filter { it.zone == DockZone.Floating }
            .maxOfOrNull { it.floatingZIndex } ?: 0
        val nextFloatingZIndex = maxZIndex + 1
        return WorkbenchLayoutState(
            panels = panels,
            activeTabByGroup = activeTabByGroup,
            preset = preset,
            nextFloatingZIndex = nextFloatingZIndex,
            dockZoneSizeDp = dockZoneSizeDp,
            dockGroupWeights = dockGroupWeights,
            dockGroupMeta = dockGroupMeta,
            activeOverlayPanelId = activeOverlayPanelId,
        )
    }

    fun clampFloatingPosition(
        x: Float, y: Float,
        widthDp: Float, heightDp: Float,
        maxWidthDp: Float, maxHeightDp: Float,
    ): Pair<Float, Float> {
        val visibleTitleBarDp = 32f
        val maxX = if (maxWidthDp.isFinite()) maxWidthDp - visibleTitleBarDp else Float.MAX_VALUE
        val maxY = if (maxHeightDp.isFinite()) maxHeightDp - FLOATING_TITLE_BAR_DP else Float.MAX_VALUE
        val minX = if (maxWidthDp.isFinite() && widthDp > visibleTitleBarDp) -(widthDp - visibleTitleBarDp) else 0f
        val clampedX = if (maxWidthDp.isFinite()) {
            x.coerceIn(minX, max(0f, maxX))
        } else x
        val clampedY = if (maxHeightDp.isFinite()) {
            y.coerceIn(0f, max(0f, maxY))
        } else y
        return clampedX to clampedY
    }
}
