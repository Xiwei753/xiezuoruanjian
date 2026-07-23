package com.xiwei.sujian.ui.compose.settings

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.R
import com.xiwei.sujian.designsystem.component.SujianDropdownMenu
import com.xiwei.sujian.designsystem.component.SujianSection
import com.xiwei.sujian.designsystem.component.SujianSlider
import com.xiwei.sujian.designsystem.component.SujianSwitchRow

@Composable
fun AppearanceSettings(
    state: SettingsUiState,
    onIntent: (SettingsIntent) -> Unit,
    modifier: Modifier = Modifier,
) {
    val settings = state.settings
    var fontSize by remember { mutableFloatStateOf(state.fontSize) }
    var lineSpacing by remember { mutableFloatStateOf(settings.editorLineSpacingMultiplier) }
    val context = LocalContext.current
    var autoSaveDelay by remember { mutableFloatStateOf((settings.autoSaveDelayMs / 1000f)) }

    androidx.compose.foundation.layout.Column(
        modifier = modifier.padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        SujianSection(title = stringResource(id = R.string.pref_category_theme)) {
            SujianDropdownMenu(
                label = stringResource(id = R.string.pref_theme_mode),
                selectedIndex = when (settings.appearanceMode) {
                    "light" -> 1
                    "dark" -> 2
                    else -> 0
                },
                options = listOf(
                    stringResource(id = R.string.theme_system),
                    stringResource(id = R.string.theme_light),
                    stringResource(id = R.string.theme_dark),
                ),
                onSelected = { index ->
                    val mode = when (index) {
                        1 -> "light"
                        2 -> "dark"
                        else -> "system"
                    }
                    onIntent(SettingsIntent.UpdateLocal { it.copy(appearanceMode = mode) })
                },
                modifier = Modifier.fillMaxWidth(),
            )
            Spacer(modifier = Modifier.height(8.dp))
            SujianDropdownMenu(
                label = stringResource(id = R.string.pref_hint_color_theme),
                selectedIndex = when (settings.colorSource) {
                    "android_dynamic" -> 1
                    "saved_palette" -> 2
                    else -> 0
                },
                options = listOf(
                    stringResource(id = R.string.pref_color_source_builtin),
                    stringResource(id = R.string.pref_use_dynamic_color),
                    stringResource(id = R.string.pref_hint_saved_palette),
                ),
                onSelected = { index ->
                    val source = when (index) {
                        1 -> "android_dynamic"
                        2 -> "saved_palette"
                        else -> "built_in"
                    }
                    onIntent(SettingsIntent.UpdateLocal { it.copy(colorSource = source) })
                    if (source == "android_dynamic") {
                        com.xiwei.sujian.ui.compose.theme.ThemeStore.captureDynamicColorAndSave(context)
                        onIntent(SettingsIntent.CaptureDynamicColor)
                    }
                },
                modifier = Modifier.fillMaxWidth(),
            )
            Spacer(modifier = Modifier.height(8.dp))
            SujianSwitchRow(
                title = stringResource(id = R.string.pref_use_dynamic_color),
                checked = settings.dynamicColorEnabled,
                onCheckedChange = { checked ->
                    onIntent(SettingsIntent.UpdateLocal { it.copy(dynamicColorEnabled = checked) })
                    if (checked) {
                        com.xiwei.sujian.ui.compose.theme.ThemeStore.captureDynamicColorAndSave(context)
                        onIntent(SettingsIntent.CaptureDynamicColor)
                    }
                },
            )
        }

        SujianSection(title = stringResource(id = R.string.pref_category_font_layout)) {
            SujianSlider(
                title = stringResource(id = R.string.pref_font_size),
                value = fontSize,
                onValueChange = { fontSize = it },
                onValueChangeFinished = { onIntent(SettingsIntent.UpdateFontSize(fontSize)) },
                valueRange = 12f..72f,
                steps = 59,
                valueLabel = "${fontSize.toInt()}sp",
                modifier = Modifier.fillMaxWidth(),
            )
            Spacer(modifier = Modifier.height(8.dp))
            SujianSlider(
                title = stringResource(id = R.string.pref_line_spacing),
                value = lineSpacing,
                onValueChange = { lineSpacing = it },
                onValueChangeFinished = {
                    onIntent(SettingsIntent.UpdateLocal { it.copy(editorLineSpacingMultiplier = lineSpacing) })
                },
                valueRange = 1f..3f,
                steps = 19,
                valueLabel = String.format("%.1fx", lineSpacing),
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}
