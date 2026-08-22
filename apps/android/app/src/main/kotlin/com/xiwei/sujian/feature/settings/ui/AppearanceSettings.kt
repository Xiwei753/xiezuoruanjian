package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.lazy.LazyListScope
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

/**
 * #632 评论 5377052579：外观设置 — 每个重控件一个 Lazy item。
 *
 * 主题分组: 标题 + 主题模式 + 颜色来源
 * 字体与排版分组: 标题 + 字号 + 行距
 *
 * 用 [SettingsExpandedFieldContainer] + [ExpandedFieldPosition] 让同一字段组的
 * 多个 item 视觉上连成一张大卡。每个 item 只 collect 自己需要的 row-level StateFlow。
 */
fun LazyListScope.appearanceSettingsItems(
    vm: SettingsViewModel,
    closeOuterGroup: Boolean,
) {
    // ── 主题分组（标题 + 主题模式 + 颜色来源）— 每个 item 独立 ──

    item(key = "appearance.theme.title", contentType = CONTENT_TYPE_FIELD_TITLE) {
        SettingsExpandedFieldContainer(
            position = ExpandedFieldPosition.First,
            closeOuterGroup = false,
        ) {
            SettingsFieldGroupTitle(title = stringResource(id = R.string.pref_category_theme))
        }
    }

    item(key = "appearance.theme.mode", contentType = CONTENT_TYPE_TEXT_FIELD) {
        val mode by vm.appearanceModeRow.collectAsStateWithLifecycle()
        SettingsExpandedFieldContainer(
            position = ExpandedFieldPosition.Middle,
            closeOuterGroup = false,
        ) {
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
        }
    }

    item(key = "appearance.theme.source", contentType = CONTENT_TYPE_TEXT_FIELD) {
        val source by vm.colorSourceRow.collectAsStateWithLifecycle()
        SettingsExpandedFieldContainer(
            position = ExpandedFieldPosition.Last,
            closeOuterGroup = false,
        ) {
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
    }

    // ── 字体与排版分组（标题 + 字号 + 行距）— 每个 item 独立 ──

    item(key = "appearance.font.title", contentType = CONTENT_TYPE_FIELD_TITLE) {
        SettingsExpandedFieldContainer(
            position = ExpandedFieldPosition.First,
            closeOuterGroup = false,
        ) {
            SettingsFieldGroupTitle(title = stringResource(id = R.string.pref_category_font_layout))
        }
    }

    item(key = "appearance.font.size", contentType = CONTENT_TYPE_SLIDER) {
        val currentFontSize by vm.fontSizeRow.collectAsStateWithLifecycle()
        var fontSize by rememberSaveable(currentFontSize) { mutableFloatStateOf(currentFontSize) }
        SettingsExpandedFieldContainer(
            position = ExpandedFieldPosition.Middle,
            closeOuterGroup = false,
        ) {
            SujianSlider(
                title = stringResource(id = R.string.pref_font_size),
                value = fontSize,
                onValueChange = { fontSize = it },
                onValueChangeFinished = { vm.handleIntent(SettingsIntent.UpdateFontSize(fontSize)) },
                valueRange = 12f..72f,
                steps = 59,
                valueLabel = "${fontSize.toInt()}sp",
                semanticId = com.xiwei.sujian.core.designsystem.testing.SujianSemanticIds.SettingsFontSize,
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }

    item(key = "appearance.font.spacing", contentType = CONTENT_TYPE_SLIDER) {
        val spacing by vm.lineSpacingRow.collectAsStateWithLifecycle()
        var lineSpacing by rememberSaveable(spacing) { mutableFloatStateOf(spacing) }
        SettingsExpandedFieldContainer(
            position = ExpandedFieldPosition.Last,
            closeOuterGroup = closeOuterGroup,
        ) {
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
