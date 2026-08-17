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
 * #630 评论13/评论15: 行级 LazyColumn — 每个真实设置控件是独立 item，有稳定 key。
 * 使用 [SettingsFieldRowContainer] 的 isFirst/isLast 保持 M3 高色阶卡片视觉。
 * 每个 item 只 collect 自己的 row-level StateFlow，避免整分类重组。
 */
fun LazyListScope.editorSettingsItems(vm: SettingsViewModel) {
    // 自动缩进开关
    item(key = "editor.auto_indent") {
        val checked by vm.autoIndentRow.collectAsStateWithLifecycle()
        SettingsGroupItemContainer(isLast = false) {
            SettingsFieldRowContainer(isFirst = true, isLast = false) {
                SujianSwitchRow(
                    title = stringResource(id = R.string.pref_auto_indent),
                    checked = checked,
                    onCheckedChange = { c ->
                        vm.handleIntent(SettingsIntent.UpdateLocal { it.copy(autoIndentEnabled = c) })
                    },
                )
            }
        }
    }

    // 自动缩进宽度
    item(key = "editor.auto_indent_width") {
        val width by vm.autoIndentWidthRow.collectAsStateWithLifecycle()
        var autoIndentWidth by rememberSaveable(width) { mutableFloatStateOf(width) }
        SettingsGroupItemContainer(isLast = true) {
            SettingsFieldRowContainer(isFirst = false, isLast = true) {
                SujianSlider(
                    title = stringResource(id = R.string.pref_auto_indent_width),
                    value = autoIndentWidth,
                    onValueChange = { autoIndentWidth = it },
                    onValueChangeFinished = {
                        vm.handleIntent(SettingsIntent.UpdateLocal { it.copy(autoIndentWidth = autoIndentWidth) })
                    },
                    valueRange = 0f..8f,
                    steps = 15,
                    valueLabel = stringResource(id = R.string.auto_indent_width_chars, autoIndentWidth),
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        }
    }

    // ── 编辑器行为分组标题 ──
    item(key = "editor.behavior_title") {
        SettingsGroupItemContainer(isLast = false) {
            SettingsFieldRowContainer(isFirst = true, isLast = false) {
                SettingsFieldGroupTitle(title = stringResource(id = R.string.pref_category_editor_behavior))
            }
        }
    }

    // 打字动画开关
    item(key = "editor.typing_animation") {
        val checked by vm.typingAnimationRow.collectAsStateWithLifecycle()
        SettingsGroupItemContainer(isLast = false) {
            SettingsFieldRowContainer(isFirst = false, isLast = false) {
                SujianSwitchRow(
                    title = stringResource(id = R.string.pref_editor_typing_animation),
                    checked = checked,
                    onCheckedChange = { c ->
                        vm.handleIntent(SettingsIntent.UpdateLocal { it.copy(editorTypingAnimationEnabled = c) })
                    },
                    semanticId = SujianSemanticIds.SettingsTypingAnimation,
                )
            }
        }
    }

    // 打字动画时长
    item(key = "editor.typing_duration") {
        val enabled by vm.typingAnimationRow.collectAsStateWithLifecycle()
        val durationMs by vm.typingAnimationDurationRow.collectAsStateWithLifecycle()
        var typingDuration by rememberSaveable(durationMs.toFloat()) {
            mutableFloatStateOf(durationMs.toFloat())
        }
        SettingsGroupItemContainer(isLast = false) {
            SettingsFieldRowContainer(isFirst = false, isLast = false) {
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
                    enabled = enabled,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        }
    }

    // 光标平滑开关
    item(key = "editor.smooth_cursor") {
        val checked by vm.smoothCursorRow.collectAsStateWithLifecycle()
        SettingsGroupItemContainer(isLast = false) {
            SettingsFieldRowContainer(isFirst = false, isLast = false) {
                SujianSwitchRow(
                    title = stringResource(id = R.string.pref_editor_smooth_cursor),
                    checked = checked,
                    onCheckedChange = { c ->
                        vm.handleIntent(SettingsIntent.UpdateLocal { it.copy(editorSmoothCursorEnabled = c) })
                    },
                )
            }
        }
    }

    // 光标平滑时长
    item(key = "editor.cursor_duration") {
        val enabled by vm.smoothCursorRow.collectAsStateWithLifecycle()
        val durationMs by vm.smoothCursorDurationRow.collectAsStateWithLifecycle()
        var cursorDuration by rememberSaveable(durationMs.toFloat()) {
            mutableFloatStateOf(durationMs.toFloat())
        }
        SettingsGroupItemContainer(isLast = true) {
            SettingsFieldRowContainer(isFirst = false, isLast = true) {
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
                    enabled = enabled,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        }
    }
}
