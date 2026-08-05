package com.xiwei.sujian.ui.phone.portrait

import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.Saver
import androidx.compose.runtime.setValue
import androidx.navigation3.runtime.NavKey
import com.xiwei.sujian.model.SyncIndicatorState
import com.xiwei.sujian.ui.compose.navigation.SettingsSection

/**
 * 手机竖屏一级 UI 状态（一级入口 + 设置折叠分类）。
 *
 * 不持有自建 CoroutineScope、Context、仓库或同步 I/O；持久化通过
 * [saver] 与回调在状态变化时同步写出，配置变化/进程恢复由 Compose
 * save pass 读取当前状态完成恢复。
 */
@Stable
class PhonePortraitStateHolder(
    initialRoot: PhoneRoot = PhoneRoot.Works,
    private val onSaveSelectedRoot: (String) -> Unit = {},
    private val onSaveExpandedSections: (Set<SettingsSection>) -> Unit = {},
    initialExpandedSections: Set<SettingsSection> = emptySet(),
) {
    var selectedRoot by mutableStateOf(if (initialRoot == PhoneRoot.StarMap) PhoneRoot.Works else initialRoot)
        private set

    var expandedSettingsSections by mutableStateOf(initialExpandedSections)
        private set

    fun chromeSpec(
        route: NavKey?,
        workspaceLocation: WorkspaceLocation,
        syncState: SyncIndicatorState,
    ): PhoneChromeSpec =
        PhoneChromePolicy.resolve(route, selectedRoot, workspaceLocation, syncState)

    fun onEvent(event: PhonePortraitEvent) {
        when (event) {
            is PhonePortraitEvent.SelectRoot -> onSelectRoot(event.root)
            is PhonePortraitEvent.ToggleSettingsSection -> onToggleSettingsSection(event.section)
            is PhonePortraitEvent.OpenSettings -> { }
            is PhonePortraitEvent.OpenGlobalSearch -> { }
        }
    }

    private fun onSelectRoot(root: PhoneRoot) {
        // 星图未开放：入口保留但完全无响应，也绝不写入 StarMap。
        if (root == PhoneRoot.StarMap) return
        selectedRoot = root
        onSaveSelectedRoot(root.name)
    }

    private fun onToggleSettingsSection(section: SettingsSection) {
        val current = expandedSettingsSections
        val newSet = if (current.contains(section)) {
            current - section
        } else {
            current + section
        }
        expandedSettingsSections = newSet
        onSaveExpandedSections(newSet)
    }

    companion object {
        /**
         * 正式 rememberSaveable Saver：保存一级入口与设置折叠状态。
         * save pass 在组合离开前读取状态当前值，因此配置变化时
         * 用户停留在统计页/正文等位置都能被如实恢复；星图仍回退到作品页。
         */
        fun saver(
            onSaveSelectedRoot: (String) -> Unit = {},
            onSaveExpandedSections: (Set<SettingsSection>) -> Unit = {},
        ): Saver<PhonePortraitStateHolder, List<String>> = Saver(
            save = { holder ->
                buildList {
                    add(holder.selectedRoot.name)
                    addAll(holder.expandedSettingsSections.map { it.name }.sorted())
                }
            },
            restore = { encoded ->
                val rootName = encoded.firstOrNull()
                val sectionNames = encoded.drop(1)
                PhonePortraitStateHolder(
                    initialRoot = rootName?.let { name ->
                        PhoneRoot.entries.find { it.name == name }
                    } ?: PhoneRoot.Works,
                    onSaveSelectedRoot = onSaveSelectedRoot,
                    onSaveExpandedSections = onSaveExpandedSections,
                    initialExpandedSections = sectionNames.mapNotNull { name ->
                        runCatching { SettingsSection.valueOf(name) }.getOrNull()
                    }.toSet(),
                )
            },
        )
    }
}
