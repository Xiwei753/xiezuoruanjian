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
    val dockZoneSizeDp: Map<DockZone, Float> = emptyMap(),
    val dockGroupWeights: Map<String, Float> = emptyMap(),
    val dockGroupMeta: Map<String, DockGroupMeta> = emptyMap(),
    val activeOverlayPanelId: WorkbenchPanelId? = null,
    val snapshotVersion: Int = LAYOUT_SNAPSHOT_VERSION,
) {
    fun dockGroupsByZone(zone: DockZone): List<DockGroupState> {
        val panelsInZone = panels.values
            .filter { it.zone == zone && it.visibility == PanelVisibility.Expanded }
        val panelDerivedGroups = panelsInZone
            .groupBy { it.tabGroupId }
            .map { (groupId, groupPanels) ->
                val sorted = groupPanels.sortedBy { it.order }
                val meta = dockGroupMeta[groupId]
                DockGroupState(
                    id = groupId,
                    zone = zone,
                    order = meta?.order ?: sorted.firstOrNull()?.order ?: 0,
                    weight = dockGroupWeights[groupId] ?: 1f,
                    activePanelId = activeTabByGroup[groupId] ?: sorted.firstOrNull()?.id,
                    panelIds = sorted.map { it.id },
                )
            }
        val panelGroupIds = panelDerivedGroups.map { it.id }.toSet()
        val standaloneGroups = dockGroupMeta.values
            .filter { it.zone == zone && it.id !in panelGroupIds }
            .map { meta ->
                DockGroupState(
                    id = meta.id,
                    zone = zone,
                    order = meta.order,
                    weight = dockGroupWeights[meta.id] ?: 1f,
                    activePanelId = null,
                    panelIds = emptyList(),
                )
            }
        return (panelDerivedGroups + standaloneGroups).sortedBy { it.order }
    }

    fun actualSideWidthDp(zone: DockZone): Float {
        val groups = dockGroupsByZone(zone)
        if (groups.isEmpty()) return 0f
        return dockZoneSizeDp[zone] ?: 0f
    }
}

const val LAYOUT_SNAPSHOT_VERSION = 2

data class DockGroupMeta(
    val id: String,
    val zone: DockZone,
    val order: Int,
)
