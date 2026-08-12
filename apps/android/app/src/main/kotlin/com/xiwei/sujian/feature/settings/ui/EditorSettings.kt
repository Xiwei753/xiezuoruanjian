package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import com.xiwei.sujian.R
import com.xiwei.sujian.core.designsystem.component.SujianSection
import com.xiwei.sujian.core.designsystem.component.SujianSlider
import com.xiwei.sujian.core.designsystem.component.SujianSwitchRow
import com.xiwei.sujian.core.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.core.designsystem.theme.LocalSujianDimensions

@Composable
fun EditorSettings(
    state: EditorSectionState,
    onIntent: (SettingsIntent) -> Unit,
    modifier: Modifier = Modifier,
) {
    val dims = LocalSujianDimensions.current
    var autoIndentWidth by rememberSaveable(state.autoIndentWidth) { mutableFloatStateOf(state.autoIndentWidth) }
    var typingDuration by rememberSaveable(state.typingAnimationDurationMs.toFloat()) {
        mutableFloatStateOf(state.typingAnimationDurationMs.toFloat())
    }
    var cursorDuration by rememberSaveable(state.smoothCursorDurationMs.toFloat()) {
        mutableFloatStateOf(state.smoothCursorDurationMs.toFloat())
    }

    androidx.compose.foundation.layout.Column(
        modifier = modifier.padding(dims.space16),
        verticalArrangement = Arrangement.spacedBy(dims.space16),
    ) {
        SujianSection(
            title = stringResource(id = R.string.pref_category_editor),
            semanticId = SujianSemanticIds.SettingsEditorSection,
        ) {
            SujianSwitchRow(
                title = stringResource(id = R.string.pref_auto_indent),
                checked = state.autoIndentEnabled,
                onCheckedChange = { checked ->
                    onIntent(SettingsIntent.UpdateLocal { it.copy(autoIndentEnabled = checked) })
                },
            )
            Spacer(modifier = Modifier.height(dims.space8))
            SujianSlider(
                title = stringResource(id = R.string.pref_auto_indent_width),
                value = autoIndentWidth,
                onValueChange = { autoIndentWidth = it },
                onValueChangeFinished = {
                    onIntent(SettingsIntent.UpdateLocal { it.copy(autoIndentWidth = autoIndentWidth) })
                },
                valueRange = 0f..8f,
                steps = 15,
                valueLabel = stringResource(id = R.string.auto_indent_width_chars, autoIndentWidth),
                modifier = Modifier.fillMaxWidth(),
            )
        }

        SujianSection(title = stringResource(id = R.string.pref_category_editor_behavior)) {
            SujianSwitchRow(
                title = stringResource(id = R.string.pref_editor_typing_animation),
                checked = state.typingAnimationEnabled,
                onCheckedChange = { checked ->
                    onIntent(SettingsIntent.UpdateLocal { it.copy(editorTypingAnimationEnabled = checked) })
                },
                semanticId = SujianSemanticIds.SettingsTypingAnimation,
            )
            Spacer(modifier = Modifier.height(dims.space8))
            SujianSlider(
                title = stringResource(id = R.string.pref_editor_typing_animation_duration),
                value = typingDuration,
                onValueChange = { typingDuration = it },
                onValueChangeFinished = {
                    onIntent(
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
            Spacer(modifier = Modifier.height(dims.space8))
            SujianSwitchRow(
                title = stringResource(id = R.string.pref_editor_smooth_cursor),
                checked = state.smoothCursorEnabled,
                onCheckedChange = { checked ->
                    onIntent(SettingsIntent.UpdateLocal { it.copy(editorSmoothCursorEnabled = checked) })
                },
            )
            Spacer(modifier = Modifier.height(dims.space8))
            SujianSlider(
                title = stringResource(id = R.string.pref_editor_smooth_cursor_duration),
                value = cursorDuration,
                onValueChange = { cursorDuration = it },
                onValueChangeFinished = {
                    onIntent(
                        SettingsIntent.UpdateLocal { it.copy(editorSmoothCursorDurationMs = cursorDuration.toInt()) },
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
