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
import com.xiwei.sujian.core.designsystem.component.SujianSlider
import com.xiwei.sujian.core.designsystem.component.SujianSwitchRow
import com.xiwei.sujian.core.designsystem.testing.SujianSemanticIds

/**
 * #633 评论 5379618506：编辑器设置 — 一个逻辑字段组 = 一张 High 内卡。
 *
 * 自动缩进分组: 开关 + 宽度（一张 SettingsInnerCard）
 * 编辑器行为分组: 标题 + 打字动画开关 + 打字动画时长 + 光标平滑开关 + 光标平滑时长（一张 SettingsInnerCard）
 *
 * 8dp 间距由 [SettingsExpandedShell] 的 spacedBy 统一产生。
 * 每张内卡内部各自 collect 自己需要的 row-level StateFlow。
 */
@Composable
fun EditorSettingsContent(vm: SettingsViewModel) {
    val autoIndent by vm.autoIndentRow.collectAsStateWithLifecycle()
    val autoIndentWidth by vm.autoIndentWidthRow.collectAsStateWithLifecycle()
    var autoIndentWidthState by rememberSaveable(autoIndentWidth) { mutableFloatStateOf(autoIndentWidth) }

    SettingsInnerCard {
        SujianSwitchRow(
            title = stringResource(id = R.string.pref_auto_indent),
            checked = autoIndent,
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

    SettingsInnerCard {
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
