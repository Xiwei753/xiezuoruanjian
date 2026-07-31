package com.xiwei.sujian.ui.compose.workbench.state

import android.content.Context
import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.floatPreferencesKey
import androidx.datastore.preferences.core.intPreferencesKey
import androidx.datastore.preferences.core.stringPreferencesKey
import com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta
import com.xiwei.sujian.ui.compose.workbench.model.DockZone
import com.xiwei.sujian.ui.compose.workbench.model.LAYOUT_SNAPSHOT_VERSION
import com.xiwei.sujian.ui.compose.workbench.model.PanelVisibility
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchLayoutState
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelId
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelState
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPreset
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.withContext

data class WindowWidthBucket(val value: String) {
    companion object {
        val Compact = WindowWidthBucket("compact")
        val Medium = WindowWidthBucket("medium")
        val Expanded = WindowWidthBucket("expanded")
        val Large = WindowWidthBucket("large")

        fun fromDp(widthDp: Int): WindowWidthBucket = when {
            widthDp < 600 -> Compact
            widthDp < 840 -> Medium
            widthDp < 1200 -> Expanded
            else -> Large
        }
    }
}

data class LayoutStorageKey(
    val deviceId: String,
    val orientation: String,
    val windowWidthBucket: WindowWidthBucket,
    val windowMode: String,
) {
    fun toStorageKey(): String = "${deviceId}|${orientation}|${windowWidthBucket.value}|${windowMode}"
}

private val Context.workbenchDataStore: DataStore<Preferences> by androidx.datastore.preferences.preferencesDataStore(
    name = "workbench_layout_prefs"
)

interface WorkbenchLayoutStore {
    suspend fun saveLayout(key: LayoutStorageKey, state: WorkbenchLayoutState)

    suspend fun loadLayout(key: LayoutStorageKey): WorkbenchLayoutState?
}

class WorkbenchLayoutRepository(
    private val context: Context,
) : WorkbenchLayoutStore {
    override suspend fun saveLayout(key: LayoutStorageKey, state: WorkbenchLayoutState) {
        withContext(Dispatchers.IO) {
            context.workbenchDataStore.edit { prefs ->
                val prefix = key.toStorageKey()
                val dynamicKeysToRemove = prefs.asMap().keys.filter { k ->
                    k.name.startsWith(prefix) && (
                        k.name == "${prefix}.activeOverlay" ||
                        k.name == "${prefix}.allGroupIds" ||
                        k.name.contains("${prefix}.groupWeight.") ||
                        k.name.contains("${prefix}.groupMeta.") ||
                        k.name.contains("${prefix}.activeTab.") ||
                        k.name.contains("${prefix}.zoneSize.")
                    )
                }
                for (k in dynamicKeysToRemove) {
                    prefs.remove(k)
                }
                prefs[intPreferencesKey("${prefix}.snapshotVersion")] = state.snapshotVersion
                for (panel in state.panels.values) {
                    val p = "${prefix}.panel.${panel.id.name}"
                    prefs[stringPreferencesKey("${p}.zone")] = panel.zone.name
                    prefs[stringPreferencesKey("${p}.visibility")] = panel.visibility.name
                    prefs[floatPreferencesKey("${p}.sizeDp")] = panel.sizeDp
                    prefs[stringPreferencesKey("${p}.tabGroupId")] = panel.tabGroupId
                    prefs[intPreferencesKey("${p}.order")] = panel.order
                    prefs[floatPreferencesKey("${p}.floatingX")] = panel.floatingX
                    prefs[floatPreferencesKey("${p}.floatingY")] = panel.floatingY
                    prefs[floatPreferencesKey("${p}.floatingWidthDp")] = panel.floatingWidthDp
                    prefs[floatPreferencesKey("${p}.floatingHeightDp")] = panel.floatingHeightDp
                    prefs[intPreferencesKey("${p}.floatingZIndex")] = panel.floatingZIndex
                }
                prefs[stringPreferencesKey("${prefix}.preset")] = state.preset.name
                for ((groupId, panelId) in state.activeTabByGroup) {
                    prefs[stringPreferencesKey("${prefix}.activeTab.${groupId}")] = panelId.name
                }
                for ((zone, size) in state.dockZoneSizeDp) {
                    prefs[floatPreferencesKey("${prefix}.zoneSize.${zone.name}")] = size
                }
                for ((groupId, weight) in state.dockGroupWeights) {
                    prefs[floatPreferencesKey("${prefix}.groupWeight.${groupId}")] = weight
                }
                val allGroupIds = (state.dockGroupWeights.keys + state.dockGroupMeta.keys).distinct()
                prefs[stringPreferencesKey("${prefix}.allGroupIds")] = allGroupIds.joinToString(",")
                for ((groupId, meta) in state.dockGroupMeta) {
                    prefs[stringPreferencesKey("${prefix}.groupMeta.${groupId}.zone")] = meta.zone.name
                    prefs[intPreferencesKey("${prefix}.groupMeta.${groupId}.order")] = meta.order
                }
                if (state.activeOverlayPanelId != null) {
                    prefs[stringPreferencesKey("${prefix}.activeOverlay")] = state.activeOverlayPanelId.name
                }
                prefs[intPreferencesKey("${prefix}.nextFloatingZIndex")] = state.nextFloatingZIndex
            }
        }
    }

    override suspend fun loadLayout(key: LayoutStorageKey): WorkbenchLayoutState? {
        return withContext(Dispatchers.IO) {
            try {
                val prefs = context.workbenchDataStore.data.first()
                val prefix = key.toStorageKey()
                val presetStr = prefs[stringPreferencesKey("${prefix}.preset")] ?: return@withContext null
                val preset = WorkbenchPreset.entries.find { it.name == presetStr } ?: WorkbenchPreset.Custom
                val snapshotVersion = prefs[intPreferencesKey("${prefix}.snapshotVersion")] ?: 1

                val panels = WorkbenchPanelId.entries.associateWith { id ->
                    val p = "${prefix}.panel.${id.name}"
                    val zoneStr = prefs[stringPreferencesKey("${p}.zone")] ?: DockZone.Left.name
                    val visStr = prefs[stringPreferencesKey("${p}.visibility")] ?: PanelVisibility.Collapsed.name
                    WorkbenchPanelState(
                        id = id,
                        zone = DockZone.entries.find { it.name == zoneStr } ?: DockZone.Left,
                        visibility = PanelVisibility.entries.find { it.name == visStr } ?: PanelVisibility.Collapsed,
                        sizeDp = prefs[floatPreferencesKey("${p}.sizeDp")] ?: 320f,
                        tabGroupId = prefs[stringPreferencesKey("${p}.tabGroupId")] ?: "",
                        order = prefs[intPreferencesKey("${p}.order")] ?: 0,
                        floatingX = prefs[floatPreferencesKey("${p}.floatingX")] ?: 0f,
                        floatingY = prefs[floatPreferencesKey("${p}.floatingY")] ?: 0f,
                        floatingWidthDp = prefs[floatPreferencesKey("${p}.floatingWidthDp")] ?: 420f,
                        floatingHeightDp = prefs[floatPreferencesKey("${p}.floatingHeightDp")] ?: 560f,
                        floatingZIndex = prefs[intPreferencesKey("${p}.floatingZIndex")] ?: 0,
                    )
                }

                val activeTabByGroup = mutableMapOf<String, WorkbenchPanelId>()
                for (groupId in panels.values.map { it.tabGroupId }.distinct().filter { it.isNotEmpty() }) {
                    val activeStr = prefs[stringPreferencesKey("${prefix}.activeTab.${groupId}")]
                    if (activeStr != null) {
                        val panelId = WorkbenchPanelId.entries.find { it.name == activeStr }
                        if (panelId != null) {
                            activeTabByGroup[groupId] = panelId
                        }
                    }
                }

                val activeOverlayStr = prefs[stringPreferencesKey("${prefix}.activeOverlay")]
                val activeOverlayPanelId = activeOverlayStr?.let {
                    WorkbenchPanelId.entries.find { id -> id.name == it }
                }

                if (snapshotVersion < 2) {
                    val dockGroupSizes = mutableMapOf<String, Float>()
                    val allGroupIds = panels.values.map { it.tabGroupId }.distinct().filter { it.isNotEmpty() }
                    for (groupId in allGroupIds) {
                        val size = prefs[floatPreferencesKey("${prefix}.groupSize.${groupId}")]
                        if (size != null) {
                            dockGroupSizes[groupId] = size
                        }
                    }
                    return@withContext WorkbenchReducer.migrateFromV1(
                        panels = panels,
                        activeTabByGroup = activeTabByGroup,
                        preset = preset,
                        dockGroupSizes = dockGroupSizes,
                        activeOverlayPanelId = activeOverlayPanelId,
                    )
                }

                val dockZoneSizeDp = mutableMapOf<DockZone, Float>()
                for (zone in listOf(DockZone.Left, DockZone.Right, DockZone.Bottom)) {
                    val size = prefs[floatPreferencesKey("${prefix}.zoneSize.${zone.name}")]
                    if (size != null) {
                        dockZoneSizeDp[zone] = size
                    }
                }

                val dockGroupWeights = mutableMapOf<String, Float>()
                val allGroupIdsStr = prefs[stringPreferencesKey("${prefix}.allGroupIds")]
                val allGroupIds = if (allGroupIdsStr != null) {
                    allGroupIdsStr.split(",").filter { it.isNotEmpty() }
                } else {
                    panels.values.map { it.tabGroupId }.distinct().filter { it.isNotEmpty() }
                }
                for (groupId in allGroupIds) {
                    val weight = prefs[floatPreferencesKey("${prefix}.groupWeight.${groupId}")]
                    if (weight != null) {
                        dockGroupWeights[groupId] = weight
                    }
                }

                val dockGroupMeta = mutableMapOf<String, DockGroupMeta>()
                for (groupId in allGroupIds) {
                    val zoneStr = prefs[stringPreferencesKey("${prefix}.groupMeta.${groupId}.zone")]
                    val order = prefs[intPreferencesKey("${prefix}.groupMeta.${groupId}.order")]
                    if (zoneStr != null && order != null) {
                        val zone = DockZone.entries.find { it.name == zoneStr } ?: DockZone.Left
                        dockGroupMeta[groupId] = DockGroupMeta(groupId, zone, order)
                    }
                }

                val savedNextFloatingZIndex = prefs[intPreferencesKey("${prefix}.nextFloatingZIndex")] ?: 1
                val maxPanelZ = panels.values
                    .filter { it.zone == DockZone.Floating && it.visibility == PanelVisibility.Expanded }
                    .maxOfOrNull { it.floatingZIndex } ?: 0
                val nextFloatingZIndex = maxOf(savedNextFloatingZIndex, maxPanelZ + 1)

                WorkbenchLayoutState(
                    panels = panels,
                    activeTabByGroup = activeTabByGroup,
                    preset = preset,
                    nextFloatingZIndex = nextFloatingZIndex,
                    dockZoneSizeDp = dockZoneSizeDp,
                    dockGroupWeights = dockGroupWeights,
                    dockGroupMeta = dockGroupMeta,
                    activeOverlayPanelId = activeOverlayPanelId,
                )
            } catch (_: Exception) {
                null
            }
        }
    }
}
