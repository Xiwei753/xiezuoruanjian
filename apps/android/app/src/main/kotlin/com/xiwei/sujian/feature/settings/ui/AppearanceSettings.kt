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
import com.xiwei.sujian.core.designsystem.testing.SujianSemanticIds

/**
 * #630 评论13/评论15/评论5324547885项2: 行级 LazyColumn — 每个真实设置控件是独立 item，有稳定 key。
 * 使用 [SettingsExpandedRowContainer] 替代旧的 [SettingsGroupItemContainer] +
 * [SettingsFieldRowContainer] 嵌套；展开内容在外层 Low 内缩 High 表面里连续拼接。
 * 每个 item 只 collect 自己的 row-level StateFlow，避免整分类重组。
 * 展开字段使用 [SettingsExpandedItemContent] 统一 fadeIn100/fadeOut70/placement120。
 */
fun LazyListScope.appearanceSettingsItems(
    vm: SettingsViewModel,
    closeOuterGroup: Boolean,
) {
    // ── 主题分组标题 ──
    item(key = "appearance.theme_title", contentType = CONTENT_TYPE_EXPANDED_GROUP_TITLE) {
        SettingsExpandedItemContent {
            SettingsExpandedRowContainer(
                closeOuterGroup = false,
                firstInCategory = true,
                lastInCategory = false,
                firstInGroup = true,
                lastInGroup = false,
            ) {
                SettingsFieldGroupTitle(stringResource(id = R.string.pref_category_theme))
            }
        }
    }

    // 主题模式
    item(key = "appearance.theme_mode", contentType = CONTENT_TYPE_EXPANDED_FIELD) {
        val mode by vm.appearanceModeRow.collectAsStateWithLifecycle()
        SettingsExpandedItemContent {
            SettingsExpandedRowContainer(
                closeOuterGroup = false,
                firstInCategory = false,
                lastInCategory = false,
                firstInGroup = false,
                lastInGroup = false,
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
    }

    // 颜色来源
    item(key = "appearance.color_source", contentType = CONTENT_TYPE_EXPANDED_FIELD) {
        val source by vm.colorSourceRow.collectAsStateWithLifecycle()
        SettingsExpandedItemContent {
            SettingsExpandedRowContainer(
                closeOuterGroup = false,
                firstInCategory = false,
                lastInCategory = false,
                firstInGroup = false,
                lastInGroup = true,
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
    }

    // ── 字体与排版分组标题 ──
    item(key = "appearance.font_title", contentType = CONTENT_TYPE_EXPANDED_GROUP_TITLE) {
        SettingsExpandedItemContent {
            SettingsExpandedRowContainer(
                closeOuterGroup = false,
                firstInCategory = false,
                lastInCategory = false,
                firstInGroup = true,
                lastInGroup = false,
            ) {
                SettingsFieldGroupTitle(stringResource(id = R.string.pref_category_font_layout))
            }
        }
    }

    // 字号
    item(key = "appearance.font_size", contentType = CONTENT_TYPE_EXPANDED_FIELD) {
        val currentFontSize by vm.fontSizeRow.collectAsStateWithLifecycle()
        var fontSize by rememberSaveable(currentFontSize) { mutableFloatStateOf(currentFontSize) }
        SettingsExpandedItemContent {
            SettingsExpandedRowContainer(
                closeOuterGroup = false,
                firstInCategory = false,
                lastInCategory = false,
                firstInGroup = false,
                lastInGroup = false,
            ) {
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
    }

    // 行距
    item(key = "appearance.line_spacing", contentType = CONTENT_TYPE_EXPANDED_FIELD) {
        val spacing by vm.lineSpacingRow.collectAsStateWithLifecycle()
        var lineSpacing by rememberSaveable(spacing) { mutableFloatStateOf(spacing) }
        SettingsExpandedItemContent {
            SettingsExpandedRowContainer(
                closeOuterGroup = closeOuterGroup,
                firstInCategory = false,
                lastInCategory = true,
                firstInGroup = false,
                lastInGroup = true,
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
}
