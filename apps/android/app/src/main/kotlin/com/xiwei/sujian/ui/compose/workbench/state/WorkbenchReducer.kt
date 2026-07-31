package com.xiwei.sujian.ui.compose.workbench.state

import com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta
import com.xiwei.sujian.ui.compose.workbench.model.DockGroupState
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

    internal const val SIDE_PANEL_MIN_DP = 280f
    internal const val SIDE_PANEL_MAX_DP = 520f
    internal const val BOTTOM_PANEL_MIN_DP = 220f
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
            is WorkbenchAction.ActivateTab -> activateTab(state, action.tabGroupId, action.panelId)
            is WorkbenchAction.FloatPanel -> floatPanel(state, action.panelId)
            is WorkbenchAction.DockPanel -> dockPanel(state, action.panelId, action.zone)
            is WorkbenchAction.DockPanelAsNewGroup -> dockPanelAsNewGroup(state, action.panelId, action.zone, action.insertionOrder)
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
            is WorkbenchAction.ResizeDockSplit -> resizeDockSplit(state, action.zone, action.beforeGroupId, action.afterGroupId, action.deltaDp, action.availableMainAxisDp)
            is WorkbenchAction.ResizeDockZone -> resizeDockZone(state, action.zone, action.deltaDp, action.availableMainAxisDp, action.actualOtherSideWidthDp)
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
        val updated = updatePanel(state, panel.copy(visibility = newVisibility), markCustom = true)
        return if (newVisibility == PanelVisibility.Expanded) {
            normalizeActiveTabs(updated.copy(activeTabByGroup = updated.activeTabByGroup + (panel.tabGroupId to panelId)))
        } else {
            normalizeActiveTabs(updated)
        }
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
        val existingZoneGroups = updatedState.dockGroupMeta.values.filter { it.zone == panel.zone }
        val maxZoneOrder = existingZoneGroups.maxOfOrNull { it.order } ?: -1
        val syncedDockGroupMeta = if (panel.tabGroupId !in updatedState.dockGroupMeta) {
            updatedState.dockGroupMeta + (panel.tabGroupId to DockGroupMeta(panel.tabGroupId, panel.zone, maxZoneOrder + 1))
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
        val withActiveTab = updatedState.copy(
            dockGroupWeights = syncedDockGroupWeights,
            dockGroupMeta = syncedDockGroupMeta,
            dockZoneSizeDp = syncedDockZoneSizeDp,
            activeTabByGroup = updatedState.activeTabByGroup + (panel.tabGroupId to panelId),
        )
        return normalizeActiveTabs(withActiveTab)
    }

    private fun collapsePanel(state: WorkbenchLayoutState, panelId: WorkbenchPanelId): WorkbenchLayoutState {
        val panel = state.panels[panelId] ?: return state
        if (panel.visibility == PanelVisibility.Collapsed) return state
        val updated = updatePanel(state, panel.copy(visibility = PanelVisibility.Collapsed), markCustom = true)
        return normalizeActiveTabs(updated)
    }

    private fun hidePanel(state: WorkbenchLayoutState, panelId: WorkbenchPanelId): WorkbenchLayoutState {
        val panel = state.panels[panelId] ?: return state
        if (panel.visibility == PanelVisibility.Hidden) return state
        val newState = updatePanel(state, panel.copy(visibility = PanelVisibility.Hidden), markCustom = true)
        val overlayFixed = if (newState.activeOverlayPanelId == panelId) {
            val remaining = newState.panels.values.filter {
                it.visibility == PanelVisibility.Expanded && it.zone != DockZone.Floating
            }
            newState.copy(activeOverlayPanelId = remaining.firstOrNull()?.id)
        } else newState
        return normalizeActiveTabs(overlayFixed)
    }

    private fun movePanel(state: WorkbenchLayoutState, panelId: WorkbenchPanelId, zone: DockZone): WorkbenchLayoutState {
        val panel = state.panels[panelId] ?: return state
        val oldGroupId = panel.tabGroupId
        val movedState = updatePanel(state, panel.copy(zone = zone), markCustom = true)
        return cleanUpOldGroup(movedState, oldGroupId)
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
        val oldGroupId = panel.tabGroupId
        val movedState = state.copy(
            panels = state.panels + (panel.id to panel.copy(
                zone = DockZone.Floating,
                visibility = PanelVisibility.Expanded,
                floatingZIndex = newZ
            )),
            preset = WorkbenchPreset.Custom,
            nextFloatingZIndex = newZ + 1
        )
        return cleanUpOldGroup(movedState, oldGroupId)
    }

    private fun floatPanelAt(state: WorkbenchLayoutState, panelId: WorkbenchPanelId, x: Float, y: Float): WorkbenchLayoutState {
        val panel = state.panels[panelId] ?: return state
        val newZ = state.nextFloatingZIndex
        val oldGroupId = panel.tabGroupId
        val movedState = state.copy(
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
        return cleanUpOldGroup(movedState, oldGroupId)
    }

    private fun dockPanel(state: WorkbenchLayoutState, panelId: WorkbenchPanelId, zone: DockZone): WorkbenchLayoutState {
        val panel = state.panels[panelId] ?: return state
        if (panel.zone == zone) {
            return movePanelBetweenGroups(state, panelId, panel.tabGroupId)
        }
        val existingGroupsInZone = visibleDockGroupsByZone(state, zone)
        val targetGroupId = existingGroupsInZone.firstOrNull()?.id
        if (targetGroupId != null) {
            return movePanelBetweenGroups(
                state.copy(panels = state.panels + (panelId to panel.copy(zone = zone))),
                panelId,
                targetGroupId,
            )
        }
        return dockPanelAsNewGroup(state, panelId, zone, 0)
    }

    private fun dockPanelAsNewGroup(state: WorkbenchLayoutState, panelId: WorkbenchPanelId, zone: DockZone, insertionOrder: Int): WorkbenchLayoutState {
        val panel = state.panels[panelId] ?: return state
        val newTabGroupId = generateDockGroupId(state, zone)
        val zoneMeta = state.dockGroupMeta.values.filter { it.zone == zone }.sortedBy { it.order }.toMutableList()
        val maxOrder = zoneMeta.maxOfOrNull { it.order } ?: -1
        val newOrder = if (insertionOrder in 0..maxOrder + 1) insertionOrder else maxOrder + 1
        val newGroupMeta = DockGroupMeta(newTabGroupId, zone, maxOrder + 1)
        zoneMeta.add(newGroupMeta)
        val insertPos = newOrder.coerceIn(0, zoneMeta.size - 1)
        zoneMeta.remove(newGroupMeta)
        zoneMeta.add(insertPos, newGroupMeta)
        val reindexedZoneMeta = zoneMeta.mapIndexed { index, meta -> meta.id to meta.copy(order = index) }.toMap()
        val reindexedMeta = state.dockGroupMeta.filter { it.value.zone != zone } + reindexedZoneMeta
        val updatedDockGroupWeights = state.dockGroupWeights + (newTabGroupId to 1f)
        val updatedDockZoneSizeDp = if (zone != DockZone.Floating && state.dockZoneSizeDp[zone] == null) {
            val defaultSize = when (zone) {
                DockZone.Left, DockZone.Right -> SIDE_PANEL_MIN_DP
                DockZone.Bottom -> BOTTOM_PANEL_MIN_DP
                else -> 0f
            }
            state.dockZoneSizeDp + (zone to defaultSize)
        } else state.dockZoneSizeDp
        val updatedActiveTab = state.activeTabByGroup + (newTabGroupId to panelId)
        val movedState = state.copy(
            panels = state.panels + (panel.id to panel.copy(
                zone = zone,
                visibility = PanelVisibility.Expanded,
                tabGroupId = newTabGroupId,
            )),
            dockGroupWeights = updatedDockGroupWeights,
            dockGroupMeta = reindexedMeta,
            dockZoneSizeDp = updatedDockZoneSizeDp,
            activeTabByGroup = updatedActiveTab,
            preset = WorkbenchPreset.Custom,
        )
        return cleanUpOldGroup(movedState, panel.tabGroupId)
    }

    private fun generateDockGroupId(state: WorkbenchLayoutState, zone: DockZone): String {
        val existingIds = state.dockGroupMeta.keys
        var counter = (state.dockGroupMeta.values
            .filter { it.zone == zone }
            .mapNotNull { regexMatchInt(it.id) }
            .maxOrNull() ?: 0) + 1
        var candidate = "${zone.name.lowercase()}-group-$counter"
        while (candidate in existingIds) {
            counter++
            candidate = "${zone.name.lowercase()}-group-$counter"
        }
        return candidate
    }

    private fun regexMatchInt(s: String): Int? {
        val match = Regex("""-(\d+)$""").find(s)
        return match?.groupValues?.get(1)?.toIntOrNull()
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
        val movedState = state.copy(
            panels = state.panels + (panelId to panel.copy(tabGroupId = tabGroupId, zone = targetGroupZone)),
            activeTabByGroup = state.activeTabByGroup + (tabGroupId to panelId),
            dockGroupWeights = if (tabGroupId !in state.dockGroupWeights) state.dockGroupWeights + (tabGroupId to 1f) else state.dockGroupWeights,
            dockGroupMeta = if (tabGroupId !in state.dockGroupMeta) state.dockGroupMeta + (tabGroupId to DockGroupMeta(tabGroupId, targetGroupZone, 0)) else state.dockGroupMeta,
            preset = WorkbenchPreset.Custom,
        )
        return cleanUpOldGroup(movedState, panel.tabGroupId)
    }

    private fun movePanelBetweenGroups(state: WorkbenchLayoutState, panelId: WorkbenchPanelId, newTabGroupId: String): WorkbenchLayoutState {
        val panel = state.panels[panelId] ?: return state
        val targetGroupZone = state.panels.values
            .filter { it.tabGroupId == newTabGroupId && it.visibility == PanelVisibility.Expanded }
            .firstOrNull()?.zone ?: state.dockGroupMeta[newTabGroupId]?.zone ?: panel.zone
        val updatedPanel = panel.copy(tabGroupId = newTabGroupId, zone = targetGroupZone, visibility = PanelVisibility.Expanded)
        val newActiveTab = state.activeTabByGroup + (newTabGroupId to panelId)
        val newGroupWeights = if (newTabGroupId !in state.dockGroupWeights) {
            state.dockGroupWeights + (newTabGroupId to 1f)
        } else {
            state.dockGroupWeights
        }
        val newGroupMeta = if (newTabGroupId !in state.dockGroupMeta) {
            state.dockGroupMeta + (newTabGroupId to DockGroupMeta(newTabGroupId, targetGroupZone, 0))
        } else {
            state.dockGroupMeta
        }
        val movedState = state.copy(
            panels = state.panels + (panelId to updatedPanel),
            activeTabByGroup = newActiveTab,
            dockGroupWeights = newGroupWeights,
            dockGroupMeta = newGroupMeta,
            preset = WorkbenchPreset.Custom,
        )
        return cleanUpOldGroup(movedState, panel.tabGroupId)
    }

    private fun cleanUpOldGroup(state: WorkbenchLayoutState, oldGroupId: String): WorkbenchLayoutState {
        val oldZone = state.dockGroupMeta[oldGroupId]?.zone
        val remainingPanels = if (oldZone != null) {
            state.panels.values.filter { it.tabGroupId == oldGroupId && it.zone == oldZone }
        } else {
            state.panels.values.filter { it.tabGroupId == oldGroupId }
        }
        if (remainingPanels.isNotEmpty()) {
            val activeTab = state.activeTabByGroup[oldGroupId]
            val activePanel = if (activeTab != null) state.panels[activeTab] else null
            val activeTabLeftGroup = activePanel == null
                || activePanel.tabGroupId != oldGroupId
                || (oldZone != null && activePanel.zone != oldZone)
            if (activeTabLeftGroup) {
                val expandedRemaining = remainingPanels.filter { it.visibility == PanelVisibility.Expanded }
                return if (expandedRemaining.isNotEmpty()) {
                    state.copy(activeTabByGroup = state.activeTabByGroup + (oldGroupId to expandedRemaining.first().id))
                } else {
                    state.copy(activeTabByGroup = state.activeTabByGroup - oldGroupId)
                }
            }
            return state
        }
        var cleanedState = state.copy(
            dockGroupMeta = state.dockGroupMeta - oldGroupId,
            dockGroupWeights = state.dockGroupWeights - oldGroupId,
            activeTabByGroup = state.activeTabByGroup - oldGroupId,
        )
        if (oldZone != null) {
            cleanedState = cleanedState.copy(
                dockGroupMeta = reindexGroupOrders(cleanedState.dockGroupMeta, oldZone),
            )
        }
        return cleanedState
    }

    private fun reindexGroupOrders(meta: Map<String, DockGroupMeta>, zone: DockZone): Map<String, DockGroupMeta> {
        val zoneGroups = meta.values.filter { it.zone == zone }.sortedBy { it.order }
        val reindexed = zoneGroups.mapIndexed { index, group -> group.id to group.copy(order = index) }.toMap()
        val otherGroups = meta.filter { it.value.zone != zone }
        return otherGroups + reindexed
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
        val existingMeta = state.dockGroupMeta[groupId] ?: return state
        val zone = existingMeta.zone
        val zoneGroups = state.dockGroupMeta.values
            .filter { it.zone == zone }
            .sortedBy { it.order }
        val currentIdx = zoneGroups.indexOfFirst { it.id == groupId }
        if (currentIdx < 0) return state
        val mutableList = zoneGroups.toMutableList()
        mutableList.removeAt(currentIdx)
        val clampedOrder = newOrder.coerceIn(0, mutableList.size)
        mutableList.add(clampedOrder, existingMeta)
        val reindexed = mutableList.mapIndexed { index, meta -> meta.id to meta.copy(order = index) }.toMap()
        val otherGroups = state.dockGroupMeta.filter { it.value.zone != zone }
        return state.copy(
            dockGroupMeta = otherGroups + reindexed,
            preset = WorkbenchPreset.Custom,
        )
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

    private fun visibleDockGroupsByZone(state: WorkbenchLayoutState, zone: DockZone): List<DockGroupState> {
        return state.dockGroupsByZone(zone).filter { it.panelIds.isNotEmpty() }
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
        val visibleGroups = visibleDockGroupsByZone(state, zone)
        val visibleIds = visibleGroups.map { it.id }
        if (beforeGroupId !in visibleIds || afterGroupId !in visibleIds) return state
        val beforeIdx = visibleIds.indexOf(beforeGroupId)
        val afterIdx = visibleIds.indexOf(afterGroupId)
        if (kotlin.math.abs(beforeIdx - afterIdx) != 1) return state
        val zoneTotalWeight = visibleGroups.sumOf { (state.dockGroupWeights[it.id] ?: 1f).toDouble() }.toFloat()
        if (zoneTotalWeight <= 0f) return state
        val requiredDp = visibleGroups.size * GROUP_MIN_DP
        if (availableMainAxisDp < requiredDp) {
            val equalWeight = zoneTotalWeight / visibleGroups.size
            return state.copy(
                dockGroupWeights = state.dockGroupWeights + visibleGroups.associate { it.id to equalWeight },
                preset = WorkbenchPreset.Custom,
            )
        }
        val effectiveMinDp = min(GROUP_MIN_DP, availableMainAxisDp / visibleGroups.size)
        val deltaWeight = deltaDp * zoneTotalWeight / availableMainAxisDp
        val minWeight = effectiveMinDp * zoneTotalWeight / availableMainAxisDp
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
        if (newBefore < 0f || newAfter < 0f) {
            val equalWeight = zoneTotalWeight / visibleGroups.size
            return state.copy(
                dockGroupWeights = state.dockGroupWeights + visibleGroups.associate { it.id to equalWeight },
                preset = WorkbenchPreset.Custom,
            )
        }
        return state.copy(
            dockGroupWeights = state.dockGroupWeights
                + (beforeGroupId to newBefore)
                + (afterGroupId to newAfter),
            preset = WorkbenchPreset.Custom
        )
    }

    private fun resizeDockZone(state: WorkbenchLayoutState, zone: DockZone, deltaDp: Float, availableMainAxisDp: Float, actualOtherSideWidthDp: Float? = null): WorkbenchLayoutState {
        val hasExpandedPanels = state.panels.values.any { it.zone == zone && it.visibility == PanelVisibility.Expanded }
        if (!hasExpandedPanels) return state
        val currentSize = state.dockZoneSizeDp[zone] ?: when (zone) {
            DockZone.Left, DockZone.Right -> SIDE_PANEL_MIN_DP
            DockZone.Bottom -> BOTTOM_PANEL_MIN_DP
            else -> return state
        }
        val newSize = currentSize + deltaDp
        val clampedSize = when (zone) {
            DockZone.Left, DockZone.Right -> {
                val otherSideWidth = if (actualOtherSideWidthDp != null) actualOtherSideWidthDp else {
                    val otherZone = when (zone) {
                        DockZone.Left -> DockZone.Right
                        DockZone.Right -> DockZone.Left
                        else -> null
                    }
                    if (otherZone != null) state.actualSideWidthDp(otherZone) else 0f
                }
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
            WorkbenchPreset.FocusWriting -> computeDefaultLayout()
            WorkbenchPreset.ChapterWriting -> chapterWritingPreset()
            WorkbenchPreset.AiWriting -> aiWritingPreset()
            WorkbenchPreset.ResearchWriting -> researchWritingPreset()
            WorkbenchPreset.Custom -> state
        }
    }

    private fun updatePanel(state: WorkbenchLayoutState, panel: WorkbenchPanelState, markCustom: Boolean): WorkbenchLayoutState {
        return state.copy(
            panels = state.panels + (panel.id to panel),
            preset = if (markCustom) WorkbenchPreset.Custom else state.preset
        )
    }

    private fun normalizeActiveTabs(state: WorkbenchLayoutState): WorkbenchLayoutState {
        var updatedActiveTab = state.activeTabByGroup
        for ((groupId, activeId) in state.activeTabByGroup) {
            val activePanel = state.panels[activeId]
            val groupZone = state.dockGroupMeta[groupId]?.zone
            val zoneMismatch = groupZone != null && activePanel != null && activePanel.zone != groupZone
            if (activePanel == null || activePanel.tabGroupId != groupId || activePanel.visibility != PanelVisibility.Expanded || zoneMismatch) {
                val expandedInGroup = state.panels.values
                    .filter { it.tabGroupId == groupId && it.visibility == PanelVisibility.Expanded && (groupZone == null || it.zone == groupZone) }
                    .sortedBy { it.order }
                if (expandedInGroup.isNotEmpty()) {
                    updatedActiveTab = updatedActiveTab + (groupId to expandedInGroup.first().id)
                } else {
                    updatedActiveTab = updatedActiveTab - groupId
                }
            }
        }
        return state.copy(activeTabByGroup = updatedActiveTab)
    }

    private fun focusWritingPreset(): WorkbenchLayoutState {
        return computeDefaultLayout()
    }

    private fun chapterWritingPreset(): WorkbenchLayoutState {
        val base = computeDefaultLayout()
        val panels = base.panels + (WorkbenchPanelId.ChapterNavigator to base.panels.getValue(WorkbenchPanelId.ChapterNavigator).copy(
            visibility = PanelVisibility.Expanded,
            sizeDp = 320f,
        ))
        return base.copy(
            panels = panels,
            activeTabByGroup = mapOf("left-nav" to WorkbenchPanelId.ChapterNavigator),
            dockZoneSizeDp = mapOf(DockZone.Left to 320f),
            dockGroupWeights = mapOf("left-nav" to 1f),
            dockGroupMeta = mapOf("left-nav" to DockGroupMeta("left-nav", DockZone.Left, 0)),
            preset = WorkbenchPreset.ChapterWriting,
        )
    }

    private fun aiWritingPreset(): WorkbenchLayoutState {
        val base = computeDefaultLayout()
        val panels = base.panels + (WorkbenchPanelId.AiAssistant to base.panels.getValue(WorkbenchPanelId.AiAssistant).copy(
            visibility = PanelVisibility.Expanded,
            sizeDp = 400f,
        ))
        return base.copy(
            panels = panels,
            activeTabByGroup = mapOf("right-tools" to WorkbenchPanelId.AiAssistant),
            dockZoneSizeDp = mapOf(DockZone.Right to 400f),
            dockGroupWeights = mapOf("right-tools" to 1f),
            dockGroupMeta = mapOf("right-tools" to DockGroupMeta("right-tools", DockZone.Right, 0)),
            preset = WorkbenchPreset.AiWriting,
        )
    }

    private fun researchWritingPreset(): WorkbenchLayoutState {
        val searchTabGroup = "research-right"
        val base = computeDefaultLayout()
        val panels = base.panels +
            (WorkbenchPanelId.ChapterNavigator to base.panels.getValue(WorkbenchPanelId.ChapterNavigator).copy(
                visibility = PanelVisibility.Expanded,
                sizeDp = 320f,
            )) +
            (WorkbenchPanelId.Search to base.panels.getValue(WorkbenchPanelId.Search).copy(
                visibility = PanelVisibility.Expanded,
                sizeDp = 380f,
                tabGroupId = searchTabGroup,
            )) +
            (WorkbenchPanelId.Statistics to base.panels.getValue(WorkbenchPanelId.Statistics).copy(
                tabGroupId = searchTabGroup,
            ))
        return base.copy(
            panels = panels,
            activeTabByGroup = mapOf(
                "left-nav" to WorkbenchPanelId.ChapterNavigator,
                searchTabGroup to WorkbenchPanelId.Search,
            ),
            dockZoneSizeDp = mapOf(DockZone.Left to 320f, DockZone.Right to 380f),
            dockGroupWeights = mapOf("left-nav" to 1f, searchTabGroup to 1f),
            dockGroupMeta = mapOf(
                "left-nav" to DockGroupMeta("left-nav", DockZone.Left, 0),
                searchTabGroup to DockGroupMeta(searchTabGroup, DockZone.Right, 0),
            ),
            preset = WorkbenchPreset.ResearchWriting,
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
            dockGroupMeta = mapOf(
                "left-nav" to DockGroupMeta("left-nav", DockZone.Left, 0),
                "right-tools" to DockGroupMeta("right-tools", DockZone.Right, 0),
                "right-outline" to DockGroupMeta("right-outline", DockZone.Right, 1),
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
