package com.xiwei.sujian.ui.compose.settings

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.R
import com.xiwei.sujian.designsystem.component.SujianSection
import com.xiwei.sujian.designsystem.component.SujianSlider
import com.xiwei.sujian.designsystem.component.SujianSwitchRow

@Composable
fun EditorSettings(
    state: SettingsUiState,
    onIntent: (SettingsIntent) -> Unit,
    modifier: Modifier = Modifier,
) {
    val settings = state.settings
    var autoIndentWidth by remember { mutableFloatStateOf(settings.autoIndentWidth) }
    var typingDuration by remember { mutableFloatStateOf(settings.editorTypingAnimationDurationMs.toFloat()) }
    var cursorDuration by remember { mutableFloatStateOf(settings.editorSmoothCursorDurationMs.toFloat()) }

    androidx.compose.foundation.layout.Column(
        modifier = modifier.padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        SujianSection(title = stringResource(id = R.string.pref_category_editor)) {
            SujianSwitchRow(
                title = stringResource(id = R.string.pref_auto_indent),
                checked = settings.autoIndentEnabled,
                onCheckedChange = { checked ->
                    onIntent(SettingsIntent.UpdateLocal { it.copy(autoIndentEnabled = checked) })
                },
            )
            Spacer(modifier = Modifier.height(8.dp))
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
                checked = settings.editorTypingAnimationEnabled,
                onCheckedChange = { checked ->
                    onIntent(SettingsIntent.UpdateLocal { it.copy(editorTypingAnimationEnabled = checked) })
                },
            )
            Spacer(modifier = Modifier.height(8.dp))
            SujianSlider(
                title = stringResource(id = R.string.pref_editor_typing_animation_duration),
                value = typingDuration,
                onValueChange = { typingDuration = it },
                onValueChangeFinished = {
                    onIntent(SettingsIntent.UpdateLocal { it.copy(editorTypingAnimationDurationMs = typingDuration.toInt()) })
                },
                valueRange = 30f..1000f,
                steps = 96,
                valueLabel = "${typingDuration.toInt()}ms",
                enabled = settings.editorTypingAnimationEnabled,
                modifier = Modifier.fillMaxWidth(),
            )
            Spacer(modifier = Modifier.height(8.dp))
            SujianSwitchRow(
                title = stringResource(id = R.string.pref_editor_smooth_cursor),
                checked = settings.editorSmoothCursorEnabled,
                onCheckedChange = { checked ->
                    onIntent(SettingsIntent.UpdateLocal { it.copy(editorSmoothCursorEnabled = checked) })
                },
            )
            Spacer(modifier = Modifier.height(8.dp))
            SujianSlider(
                title = stringResource(id = R.string.pref_editor_smooth_cursor_duration),
                value = cursorDuration,
                onValueChange = { cursorDuration = it },
                onValueChangeFinished = {
                    onIntent(SettingsIntent.UpdateLocal { it.copy(editorSmoothCursorDurationMs = cursorDuration.toInt()) })
                },
                valueRange = 30f..1000f,
                steps = 96,
                valueLabel = "${cursorDuration.toInt()}ms",
                enabled = settings.editorSmoothCursorEnabled,
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}
