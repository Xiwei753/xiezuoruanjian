package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
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
 * #633 评论 5379618506：外观设置 — 一个逻辑字段组 = 一张 High 内卡。
 *
 * 主题分组: 标题 + 主题模式 + 颜色来源（一张 SettingsInnerCard）
 * 字体与排版分组: 标题 + 字号 + 行距（一张 SettingsInnerCard）
 */
@Composable
fun AppearanceSettingsContent(vm: SettingsViewModel) {
    val mode by vm.appearanceModeRow.collectAsStateWithLifecycle()
    val source by vm.colorSourceRow.collectAsStateWithLifecycle()

    SettingsInnerCard {
        SettingsFieldGroupTitle(title = stringResource(id = R.string.pref_category_theme))
        SujianDropdownMenu(
            label = stringResource(id = R.string.pref_theme_mode),
            selectedIndex =
                when (mode) {
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
                val m =
                    when (index) {
                        1 -> "light"
                        2 -> "dark"
                        else -> "system"
                    }
                vm.handleIntent(SettingsIntent.UpdateLocal { it.copy(appearanceMode = m) })
            },
            modifier = Modifier.fillMaxWidth(),
        )
        SujianDropdownMenu(
            label = stringResource(id = R.string.pref_hint_color_theme),
            selectedIndex =
                when (source) {
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
                val s =
                    when (index) {
                        1 -> "android_dynamic"
                        2 -> "saved_palette"
                        else -> "built_in"
                    }
                if (s == "android_dynamic") {
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
                                colorSource = s,
                                dynamicColorEnabled = false,
                            )
                        },
                    )
                }
            },
            modifier = Modifier.fillMaxWidth(),
        )
    }

    val currentFontSize by vm.fontSizeRow.collectAsStateWithLifecycle()
    var fontSize by rememberSaveable(currentFontSize) { mutableFloatStateOf(currentFontSize) }
    val spacing by vm.lineSpacingRow.collectAsStateWithLifecycle()
    var lineSpacing by rememberSaveable(spacing) { mutableFloatStateOf(spacing) }

    SettingsInnerCard {
        SettingsFieldGroupTitle(title = stringResource(id = R.string.pref_category_font_layout))
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
