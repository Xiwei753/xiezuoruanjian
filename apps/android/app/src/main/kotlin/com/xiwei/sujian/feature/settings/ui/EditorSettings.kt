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
import com.xiwei.sujian.core.designsystem.component.SujianSlider
import com.xiwei.sujian.core.designsystem.component.SujianSwitchRow
import com.xiwei.sujian.core.designsystem.testing.SujianSemanticIds

/**
 * #631 字段组模式: 将原来的 6 个独立 item 合并为 2 个字段组 item。
 *
 * 自动缩进分组: 开关 + 宽度
 * 编辑器行为分组: 标题 + 打字动画开关 + 打字动画时长 + 光标平滑开关 + 光标平滑时长
 *
 * 使用 [SettingsFieldGroupContainer] 替代 [SettingsExpandedRowContainer]，
 * 使用 [CONTENT_TYPE_EXPANDED_FIELD_GROUP] 作为 contentType。
 * 每个字段组一个 item，组内多个字段普通布局（不做 animateItem）。
 * 每个 item 只 collect 自己需要的 row-level StateFlow，避免整分类重组。
 * 展开字段使用 [SettingsExpandedItemContent] 统一 fadeIn100/fadeOut70/placement120。
 */
fun LazyListScope.editorSettingsItems(
    vm: SettingsViewModel,
    closeOuterGroup: Boolean,
) {
    // ── 自动缩进分组（开关 + 宽度）— 一个字段组 item ──
    item(
        key = "editor.auto_indent_group",
        contentType = CONTENT_TYPE_EXPANDED_FIELD_GROUP,
    ) {
        val autoIndentChecked by vm.autoIndentRow.collectAsStateWithLifecycle()
        val autoIndentWidth by vm.autoIndentWidthRow.collectAsStateWithLifecycle()
        var autoIndentWidthState by rememberSaveable(autoIndentWidth) { mutableFloatStateOf(autoIndentWidth) }
        SettingsExpandedItemContent {
            SettingsFieldGroupContainer(
                closeOuterGroup = false,
                firstInGroup = true,
                lastInGroup = false,
            ) {
                SujianSwitchRow(
                    title = stringResource(id = R.string.pref_auto_indent),
                    checked = autoIndentChecked,
                    onCheckedChange = { c ->
                        vm.handleIntent(SettingsIntent.UpdateLocal { it.copy(autoIndentEnabled = c) })
                    },
                )
                SujianSlider(
                    title = stringResource(id = R.string.pref_auto_indent_width),
                    value = autoIndentWidthState,
                    onValueChange = { autoIndentWidthState = it },
                    onValueChangeFinished = {
                        vm.handleIntent(
                            SettingsIntent.UpdateLocal { it.copy(autoIndentWidth = autoIndentWidthState) },
                        )
                    },
                    valueRange = 0f..8f,
                    steps = 15,
                    valueLabel = stringResource(id = R.string.auto_indent_width_chars, autoIndentWidthState.toInt()),
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        }
    }

    // ── 编辑器行为分组（标题 + 打字动画开关 + 打字动画时长 + 光标平滑开关 + 光标平滑时长）— 一个字段组 item ──
    item(
        key = "editor.behavior_group",
        contentType = CONTENT_TYPE_EXPANDED_FIELD_GROUP,
    ) {
        val typingAnimationChecked by vm.typingAnimationRow.collectAsStateWithLifecycle()
        val typingAnimationDuration by vm.typingAnimationDurationRow.collectAsStateWithLifecycle()
        var typingDuration by rememberSaveable(typingAnimationDuration.toFloat()) {
            mutableFloatStateOf(typingAnimationDuration.toFloat())
        }
        val smoothCursorChecked by vm.smoothCursorRow.collectAsStateWithLifecycle()
        val smoothCursorDuration by vm.smoothCursorDurationRow.collectAsStateWithLifecycle()
        var cursorDuration by rememberSaveable(smoothCursorDuration.toFloat()) {
            mutableFloatStateOf(smoothCursorDuration.toFloat())
        }
        SettingsExpandedItemContent {
            SettingsFieldGroupContainer(
                closeOuterGroup = closeOuterGroup,
                firstInGroup = false,
                lastInGroup = true,
            ) {
                SettingsFieldGroupTitle(title = stringResource(id = R.string.pref_category_editor_behavior))
                SujianSwitchRow(
                    title = stringResource(id = R.string.pref_editor_typing_animation),
                    checked = typingAnimationChecked,
                    onCheckedChange = { c ->
                        vm.handleIntent(SettingsIntent.UpdateLocal { it.copy(editorTypingAnimationEnabled = c) })
                    },
                    semanticId = SujianSemanticIds.SettingsTypingAnimation,
                )
                SujianSlider(
                    title = stringResource(id = R.string.pref_editor_typing_animation_duration),
                    value = typingDuration,
                    onValueChange = { typingDuration = it },
                    onValueChangeFinished = {
                        vm.handleIntent(
                            SettingsIntent.UpdateLocal {
                                it.copy(editorTypingAnimationDurationMs = typingDuration.toInt())
                            },
                        )
                    },
                    valueRange = 30f..1000f,
                    steps = 96,
                    valueLabel = "${typingDuration.toInt()}ms",
                    enabled = typingAnimationChecked,
                    modifier = Modifier.fillMaxWidth(),
                )
                SujianSwitchRow(
                    title = stringResource(id = R.string.pref_editor_smooth_cursor),
                    checked = smoothCursorChecked,
                    onCheckedChange = { c ->
                        vm.handleIntent(SettingsIntent.UpdateLocal { it.copy(editorSmoothCursorEnabled = c) })
                    },
                )
                SujianSlider(
                    title = stringResource(id = R.string.pref_editor_smooth_cursor_duration),
                    value = cursorDuration,
                    onValueChange = { cursorDuration = it },
                    onValueChangeFinished = {
                        vm.handleIntent(
                            SettingsIntent.UpdateLocal {
                                it.copy(editorSmoothCursorDurationMs = cursorDuration.toInt())
                            },
                        )
                    },
                    valueRange = 30f..1000f,
                    steps = 96,
                    valueLabel = "${cursorDuration.toInt()}ms",
                    enabled = smoothCursorChecked,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        }
    }
}
