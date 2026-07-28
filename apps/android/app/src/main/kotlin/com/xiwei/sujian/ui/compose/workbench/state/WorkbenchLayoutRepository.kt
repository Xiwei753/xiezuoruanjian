package com.xiwei.sujian.ui.compose.workbench.state

import android.content.Context
import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.floatPreferencesKey
import androidx.datastore.preferences.core.intPreferencesKey
import androidx.datastore.preferences.core.stringPreferencesKey
import com.xiwei.sujian.ui.compose.workbench.model.DockZone
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

class WorkbenchLayoutRepository(
    private val context: Context,
) {
    suspend fun saveLayout(key: LayoutStorageKey, state: WorkbenchLayoutState) {
        withContext(Dispatchers.IO) {
            context.workbenchDataStore.edit { prefs ->
                val prefix = key.toStorageKey()
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
                }
                prefs[stringPreferencesKey("${prefix}.preset")] = state.preset.name
                for ((groupId, panelId) in state.activeTabByGroup) {
                    prefs[stringPreferencesKey("${prefix}.activeTab.${groupId}")] = panelId.name
                }
            }
        }
    }

    suspend fun loadLayout(key: LayoutStorageKey): WorkbenchLayoutState? {
        return withContext(Dispatchers.IO) {
            try {
                val prefs = context.workbenchDataStore.data.first()
                val prefix = key.toStorageKey()
                val presetStr = prefs[stringPreferencesKey("${prefix}.preset")] ?: return@withContext null
                val preset = WorkbenchPreset.entries.find { it.name == presetStr } ?: WorkbenchPreset.Custom

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

                WorkbenchLayoutState(panels = panels, activeTabByGroup = activeTabByGroup, preset = preset)
            } catch (_: Exception) {
                null
            }
        }
    }
}
