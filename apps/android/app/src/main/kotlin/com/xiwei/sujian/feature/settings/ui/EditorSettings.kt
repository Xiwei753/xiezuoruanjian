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
 */
fun LazyListScope.editorSettingsItems(vm: SettingsViewModel) {
    // ── 编辑器基础分组标题 ──
    item(key = "editor.basic_title") {
        SettingsGroupItemContainer(isLast = false, isFirst = true) {
            SettingsFieldGroupTitle(
                title = stringResource(id = R.string.pref_category_editor),
                semanticId = SujianSemanticIds.SettingsEditorSection,
            )
        }
    }

    // 自动缩进开关
    item(key = "editor.auto_indent") {
        val state by vm.editorState.collectAsStateWithLifecycle()
        SettingsGroupItemContainer(isLast = false) {
            SujianSwitchRow(
                title = stringResource(id = R.string.pref_auto_indent),
                checked = state.autoIndentEnabled,
                onCheckedChange = { checked ->
                    vm.handleIntent(SettingsIntent.UpdateLocal { it.copy(autoIndentEnabled = checked) })
                },
            )
        }
    }

    // 自动缩进宽度
    item(key = "editor.auto_indent_width") {
        val state by vm.editorState.collectAsStateWithLifecycle()
        var autoIndentWidth by rememberSaveable(
            state.autoIndentWidth,
        ) { mutableFloatStateOf(state.autoIndentWidth) }
        SettingsGroupItemContainer(isLast = true) {
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

    // ── 编辑器行为分组标题 ──
    item(key = "editor.behavior_title") {
        SettingsGroupItemContainer(isLast = false) {
            SettingsFieldGroupTitle(title = stringResource(id = R.string.pref_category_editor_behavior))
        }
    }

    // 打字动画开关
    item(key = "editor.typing_animation") {
        val state by vm.editorState.collectAsStateWithLifecycle()
        SettingsGroupItemContainer(isLast = false) {
            SujianSwitchRow(
                title = stringResource(id = R.string.pref_editor_typing_animation),
                checked = state.typingAnimationEnabled,
                onCheckedChange = { checked ->
                    vm.handleIntent(SettingsIntent.UpdateLocal { it.copy(editorTypingAnimationEnabled = checked) })
                },
                semanticId = SujianSemanticIds.SettingsTypingAnimation,
            )
        }
    }

    // 打字动画时长
    item(key = "editor.typing_duration") {
        val state by vm.editorState.collectAsStateWithLifecycle()
        var typingDuration by rememberSaveable(state.typingAnimationDurationMs.toFloat()) {
            mutableFloatStateOf(state.typingAnimationDurationMs.toFloat())
        }
        SettingsGroupItemContainer(isLast = false) {
            SujianSlider(
                title = stringResource(id = R.string.pref_editor_typing_animation_duration),
                value = typingDuration,
                onValueChange = { typingDuration = it },
                onValueChangeFinished = {
                    vm.handleIntent(
                        SettingsIntent.UpdateLocal {
                            it.copy(
                                editorTypingAnimationDurationMs = typingDuration.toInt(),
                            )
                        },
                    )
                },
                valueRange = 30f..1000f,
                steps = 96,
                valueLabel = "${typingDuration.toInt()}ms",
                enabled = state.typingAnimationEnabled,
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }

    // 光标平滑开关
    item(key = "editor.smooth_cursor") {
        val state by vm.editorState.collectAsStateWithLifecycle()
        SettingsGroupItemContainer(isLast = false) {
            SujianSwitchRow(
                title = stringResource(id = R.string.pref_editor_smooth_cursor),
                checked = state.smoothCursorEnabled,
                onCheckedChange = { checked ->
                    vm.handleIntent(SettingsIntent.UpdateLocal { it.copy(editorSmoothCursorEnabled = checked) })
                },
            )
        }
    }

    // 光标平滑时长
    item(key = "editor.cursor_duration") {
        val state by vm.editorState.collectAsStateWithLifecycle()
        var cursorDuration by rememberSaveable(state.smoothCursorDurationMs.toFloat()) {
            mutableFloatStateOf(state.smoothCursorDurationMs.toFloat())
        }
        SettingsGroupItemContainer(isLast = true) {
            SujianSlider(
                title = stringResource(id = R.string.pref_editor_smooth_cursor_duration),
                value = cursorDuration,
                onValueChange = { cursorDuration = it },
                onValueChangeFinished = {
                    vm.handleIntent(
                        SettingsIntent.UpdateLocal {
                            it.copy(
                                editorSmoothCursorDurationMs = cursorDuration.toInt(),
                            )
                        },
                    )
                },
                valueRange = 30f..1000f,
                steps = 96,
                valueLabel = "${cursorDuration.toInt()}ms",
                enabled = state.smoothCursorEnabled,
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}
