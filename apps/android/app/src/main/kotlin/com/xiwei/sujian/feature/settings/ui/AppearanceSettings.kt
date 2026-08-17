package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.xiwei.sujian.R
import com.xiwei.sujian.core.designsystem.component.SujianDropdownMenu
import com.xiwei.sujian.core.designsystem.component.SujianSlider
import com.xiwei.sujian.core.designsystem.testing.SujianSemanticIds

/**
 * #630 评论13 项2: 扁平 LazyColumn — 向父 [LazyListScope] 注册行，
 * 每个 [SettingsFieldGroup] 是独立 item，有稳定 key。
 */
fun LazyListScope.appearanceSettingsItems(vm: SettingsViewModel) {
    // 主题分组
    item(key = "appearance.theme_group") {
        val state by vm.appearanceState.collectAsStateWithLifecycle()
        SettingsGroupItemContainer(isLast = false, isFirst = true) {
            SettingsFieldGroup(title = stringResource(id = R.string.pref_category_theme)) {
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
                Spacer(modifier = Modifier.height(8.dp))
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
    }

    // 字体与排版分组
    item(key = "appearance.font_group") {
        val state by vm.appearanceState.collectAsStateWithLifecycle()
        SettingsGroupItemContainer(isLast = true) {
            var fontSize by rememberSaveable(state.fontSize) { mutableFloatStateOf(state.fontSize) }
            var lineSpacing by rememberSaveable(state.lineSpacing) { mutableFloatStateOf(state.lineSpacing) }

            SettingsFieldGroup(title = stringResource(id = R.string.pref_category_font_layout)) {
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
                Spacer(modifier = Modifier.height(8.dp))
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
}
