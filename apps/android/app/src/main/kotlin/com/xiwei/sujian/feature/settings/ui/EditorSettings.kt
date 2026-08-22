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
 * #632 评论 5377052579：编辑器设置 — 每个重控件一个 Lazy item。
 *
 * 自动缩进分组: 开关 + 宽度
 * 编辑器行为分组: 标题 + 打字动画开关 + 打字动画时长 + 光标平滑开关 + 光标平滑时长
 *
 * 用 [SettingsExpandedFieldContainer] + [ExpandedFieldPosition] 让同一字段组的
 * 多个 item 视觉上连成一张大卡。每个 item 只 collect 自己需要的 row-level StateFlow。
 */
fun LazyListScope.editorSettingsItems(
    vm: SettingsViewModel,
    closeOuterGroup: Boolean,
) {
    // ── 自动缩进分组（开关 + 宽度）— 每个 item 独立 ──

    item(key = "editor.auto_indent.switch", contentType = CONTENT_TYPE_SWITCH) {
        val autoIndentChecked by vm.autoIndentRow.collectAsStateWithLifecycle()
        SettingsExpandedFieldContainer(
            position = ExpandedFieldPosition.First,
            closeOuterGroup = false,
        ) {
            SujianSwitchRow(
                title = stringResource(id = R.string.pref_auto_indent),
                checked = autoIndentChecked,
                onCheckedChange = { c ->
                    vm.handleIntent(SettingsIntent.UpdateLocal { it.copy(autoIndentEnabled = c) })
                },
            )
        }
    }

    item(key = "editor.auto_indent.width", contentType = CONTENT_TYPE_SLIDER) {
        val autoIndentWidth by vm.autoIndentWidthRow.collectAsStateWithLifecycle()
        var autoIndentWidthState by rememberSaveable(autoIndentWidth) { mutableFloatStateOf(autoIndentWidth) }
        SettingsExpandedFieldContainer(
            position = ExpandedFieldPosition.Last,
            closeOuterGroup = false,
        ) {
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

    // ── 编辑器行为分组（标题 + 打字动画开关 + 打字动画时长 + 光标平滑开关 + 光标平滑时长）— 每个 item 独立 ──

    item(key = "editor.behavior.title", contentType = CONTENT_TYPE_FIELD_TITLE) {
        SettingsExpandedFieldContainer(
            position = ExpandedFieldPosition.First,
            closeOuterGroup = false,
        ) {
            SettingsFieldGroupTitle(title = stringResource(id = R.string.pref_category_editor_behavior))
        }
    }

    item(key = "editor.behavior.typing_animation", contentType = CONTENT_TYPE_SWITCH) {
        val typingAnimationChecked by vm.typingAnimationRow.collectAsStateWithLifecycle()
        SettingsExpandedFieldContainer(
            position = ExpandedFieldPosition.Middle,
            closeOuterGroup = false,
        ) {
            SujianSwitchRow(
                title = stringResource(id = R.string.pref_editor_typing_animation),
                checked = typingAnimationChecked,
                onCheckedChange = { c ->
                    vm.handleIntent(SettingsIntent.UpdateLocal { it.copy(editorTypingAnimationEnabled = c) })
                },
                semanticId = SujianSemanticIds.SettingsTypingAnimation,
            )
        }
    }

    item(key = "editor.behavior.typing_duration", contentType = CONTENT_TYPE_SLIDER) {
        val typingAnimationChecked by vm.typingAnimationRow.collectAsStateWithLifecycle()
        val typingAnimationDuration by vm.typingAnimationDurationRow.collectAsStateWithLifecycle()
        var typingDuration by rememberSaveable(typingAnimationDuration.toFloat()) {
            mutableFloatStateOf(typingAnimationDuration.toFloat())
        }
        SettingsExpandedFieldContainer(
            position = ExpandedFieldPosition.Middle,
            closeOuterGroup = false,
        ) {
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
        }
    }

    item(key = "editor.behavior.smooth_cursor", contentType = CONTENT_TYPE_SWITCH) {
        val smoothCursorChecked by vm.smoothCursorRow.collectAsStateWithLifecycle()
        SettingsExpandedFieldContainer(
            position = ExpandedFieldPosition.Middle,
            closeOuterGroup = false,
        ) {
            SujianSwitchRow(
                title = stringResource(id = R.string.pref_editor_smooth_cursor),
                checked = smoothCursorChecked,
                onCheckedChange = { c ->
                    vm.handleIntent(SettingsIntent.UpdateLocal { it.copy(editorSmoothCursorEnabled = c) })
                },
            )
        }
    }

    item(key = "editor.behavior.smooth_cursor_duration", contentType = CONTENT_TYPE_SLIDER) {
        val smoothCursorChecked by vm.smoothCursorRow.collectAsStateWithLifecycle()
        val smoothCursorDuration by vm.smoothCursorDurationRow.collectAsStateWithLifecycle()
        var cursorDuration by rememberSaveable(smoothCursorDuration.toFloat()) {
            mutableFloatStateOf(smoothCursorDuration.toFloat())
        }
        SettingsExpandedFieldContainer(
            position = ExpandedFieldPosition.Last,
            closeOuterGroup = closeOuterGroup,
        ) {
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
