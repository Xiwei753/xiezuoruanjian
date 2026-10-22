package com.xiwei.sujian.feature.settings.ui

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.Saver
import androidx.compose.runtime.setValue
import com.xiwei.sujian.app.navigation.SettingsSection

/**
 * 设置分类展开状态（#618 四）— 每个 [SettingsSection] 一份独立布尔状态。
 *
 * 用 [mutableStateMapOf] 保存：切一个分类只让读了该 section 状态的 item 失效，
 * 不再因为替换整个 Set 让设置根节点和其它分类一起参与重组。
 *
 * [Saver] 只保存展开的 section name 列表，跨配置变更/进程恢复语义与旧的
 * `rememberSaveable(setOf<SettingsSection>())` 一致（unknown name 忽略）。
 */
class SettingsExpansionState {
    private val expandedSections = mutableStateMapOf<SettingsSection, Boolean>()
    private var lastExpandedSection by mutableStateOf<SettingsSection?>(null)

    fun isExpanded(section: SettingsSection): Boolean = expandedSections[section] ?: false

    fun setExpanded(
        section: SettingsSection,
        expanded: Boolean,
    ) {
        expandedSections[section] = expanded
        if (expanded) lastExpandedSection = section
    }

    /** 最近展开/当前所在分类 — 供设置页搜索入口构造来源上下文。 */
    fun currentSection(): SettingsSection? =
        lastExpandedSection ?: expandedSections.entries.firstOrNull { it.value }?.key

    /** 当前展开的 section name 列表（Saver 保存侧；抽成普通函数便于 JVM 单测）。 */
    internal fun expandedSectionNames(): List<String> =
        expandedSections
            .filterValues { it }
            .keys
            .map { it.name }

    companion object {
        val Saver: Saver<SettingsExpansionState, List<String>> =
            Saver(
                save = { state -> state.expandedSectionNames() },
                restore = { restored ->
                    SettingsExpansionState().apply {
                        restored.mapNotNull { name ->
                            runCatching { SettingsSection.valueOf(name) }.getOrNull()
                        }.forEach { section ->
                            expandedSections[section] = true
                        }
                    }
                },
            )
    }
}
