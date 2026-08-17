package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.xiwei.sujian.R
import com.xiwei.sujian.core.designsystem.component.SujianDropdownMenu
import com.xiwei.sujian.core.designsystem.component.SujianSlider
import com.xiwei.sujian.core.designsystem.testing.SujianSemanticIds

/**
 * #630 评论13/评论15: 行级 LazyColumn — 每个真实设置控件是独立 item，有稳定 key。
 * 通过 [SettingsGroupItemContainer] 的 isFirst/isLast 保持 M3 卡片视觉。
 */
fun LazyListScope.appearanceSettingsItems(vm: SettingsViewModel) {
    // ── 主题分组标题 ──
    item(key = "appearance.theme_title") {
        val isGroupFirst = true
        SettingsGroupItemContainer(isLast = false, isFirst = isGroupFirst) {
            SettingsFieldGroupTitle(stringResource(id = R.string.pref_category_theme))
        }
    }

    // 主题模式
    item(key = "appearance.theme_mode") {
        val appearanceMode by remember {
            derivedStateOf {
                vm.appearanceState.value.appearanceMode
            }
        }
        val state by vm.appearanceState.collectAsStateWithLifecycle()
        SettingsGroupItemContainer(isLast = false) {
            SujianDropdownMenu(
                label = stringResource(id = R.string.pref_theme_mode),
                selectedIndex =
                    when (state.appearanceMode) {
                        "light" -> 1
                        "dark" -> 2
                        else -> 0
                    },
                options =
                    listOf(
                        stringResource(id = R.string.theme_system),
                        stringResource(id = R.string.theme_light),
                        stringResource(id = R.string.theme_dark),
                    ),
                onSelected = { index ->
                    val mode =
                        when (index) {
                            1 -> "light"
                            2 -> "dark"
                            else -> "system"
                        }
                    vm.handleIntent(SettingsIntent.UpdateLocal { it.copy(appearanceMode = mode) })
                },
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }

    // 颜色来源
    item(key = "appearance.color_source") {
        val state by vm.appearanceState.collectAsStateWithLifecycle()
        SettingsGroupItemContainer(isLast = true) {
            SujianDropdownMenu(
                label = stringResource(id = R.string.pref_hint_color_theme),
                selectedIndex =
                    when (state.colorSource) {
                        "android_dynamic" -> 1
                        "saved_palette" -> 2
                        else -> 0
                    },
                options =
                    listOf(
                        stringResource(id = R.string.pref_color_source_builtin),
                        stringResource(id = R.string.pref_use_dynamic_color),
                        stringResource(id = R.string.pref_hint_saved_palette),
                    ),
                onSelected = { index ->
                    val source =
                        when (index) {
                            1 -> "android_dynamic"
                            2 -> "saved_palette"
                            else -> "built_in"
                        }
                    if (source == "android_dynamic") {
                        vm.handleIntent(
                            SettingsIntent.UpdateLocal {
                                it.copy(
                                    colorSource = "android_dynamic",
                                    dynamicColorEnabled = true,
                                )
                            },
                        )
                    } else {
                        vm.handleIntent(
                            SettingsIntent.UpdateLocal {
                                it.copy(
                                    colorSource = source,
                                    dynamicColorEnabled = false,
                                )
                            },
                        )
                    }
                },
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }

    // ── 字体与排版分组标题 ──
    item(key = "appearance.font_title") {
        SettingsGroupItemContainer(isLast = false) {
            SettingsFieldGroupTitle(stringResource(id = R.string.pref_category_font_layout))
        }
    }

    // 字号
    item(key = "appearance.font_size") {
        val state by vm.appearanceState.collectAsStateWithLifecycle()
        var fontSize by rememberSaveable(state.fontSize) { mutableFloatStateOf(state.fontSize) }
        SettingsGroupItemContainer(isLast = false) {
            SujianSlider(
                title = stringResource(id = R.string.pref_font_size),
                value = fontSize,
                onValueChange = { fontSize = it },
                onValueChangeFinished = { vm.handleIntent(SettingsIntent.UpdateFontSize(fontSize)) },
                valueRange = 12f..72f,
                steps = 59,
                valueLabel = "${fontSize.toInt()}sp",
                semanticId = SujianSemanticIds.SettingsFontSize,
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }

    // 行距
    item(key = "appearance.line_spacing") {
        val state by vm.appearanceState.collectAsStateWithLifecycle()
        var lineSpacing by rememberSaveable(state.lineSpacing) { mutableFloatStateOf(state.lineSpacing) }
        SettingsGroupItemContainer(isLast = true) {
            SujianSlider(
                title = stringResource(id = R.string.pref_line_spacing),
                value = lineSpacing,
                onValueChange = { lineSpacing = it },
                onValueChangeFinished = {
                    vm.handleIntent(
                        SettingsIntent.UpdateLocal { it.copy(editorLineSpacingMultiplier = lineSpacing) },
                    )
                },
                valueRange = 1f..3f,
                steps = 19,
                valueLabel = String.format(java.util.Locale.ROOT, "%.1fx", lineSpacing),
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}
