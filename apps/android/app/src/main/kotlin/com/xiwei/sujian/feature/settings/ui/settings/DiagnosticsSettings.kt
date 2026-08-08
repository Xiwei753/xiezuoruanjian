package com.xiwei.sujian.feature.settings.ui

import android.widget.Toast
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import com.xiwei.sujian.R
import com.xiwei.sujian.app.diagnostics.DiagnosticsExporter
import com.xiwei.sujian.app.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.app.diagnostics.EditorEventRingBuffer
import com.xiwei.sujian.core.designsystem.component.SujianOutlinedButton
import com.xiwei.sujian.core.designsystem.component.SujianSection
import com.xiwei.sujian.core.designsystem.component.SujianSwitchRow
import com.xiwei.sujian.core.designsystem.theme.LocalSujianDimensions

@Composable
fun DiagnosticsSettings(
    state: SettingsUiState,
    onIntent: (SettingsIntent) -> Unit,
    modifier: Modifier = Modifier,
) {
    val context = LocalContext.current
    val settings = state.settings
    val dims = LocalSujianDimensions.current
    val exportFailedText = stringResource(id = R.string.diagnostics_export_failed)
    val clearedText = stringResource(id = R.string.diagnostics_cleared)
    val deviceInfoCopiedText = stringResource(id = R.string.diagnostics_device_info_copied)

    androidx.compose.foundation.layout.Column(
        modifier = modifier.padding(dims.space16),
        verticalArrangement = Arrangement.spacedBy(dims.space16),
    ) {
        SujianSection(title = stringResource(id = R.string.pref_category_diagnostics)) {
            SujianSwitchRow(
                title = stringResource(id = R.string.pref_diagnostics_enabled),
                checked = settings.diagnosticsEnabled,
                onCheckedChange = { checked ->
                    var s = settings.copy(diagnosticsEnabled = checked)
                    DiagnosticsLogger.setEnabled(checked)
                    EditorEventRingBuffer.setEnabled(checked)
                    if (!checked) {
                        s = s.copy(diagnosticsVerbose = false)
                        DiagnosticsLogger.setVerbose(false)
                    }
                    onIntent(SettingsIntent.UpdateLocal { s })
                },
            )
            Spacer(modifier = Modifier.height(dims.space8))
            SujianSwitchRow(
                title = stringResource(id = R.string.pref_diagnostics_verbose),
                checked = settings.diagnosticsVerbose,
                onCheckedChange = { checked ->
                    DiagnosticsLogger.setVerbose(checked)
                    onIntent(SettingsIntent.UpdateLocal { it.copy(diagnosticsVerbose = checked) })
                },
                enabled = settings.diagnosticsEnabled,
            )
            Spacer(modifier = Modifier.height(dims.space8))
            SujianOutlinedButton(
                text = stringResource(id = R.string.btn_export_diagnostics),
                onClick = {
                    DiagnosticsLogger.flush()
                    val zipFile = DiagnosticsExporter.export(context)
                    if (zipFile != null) {
                        DiagnosticsExporter.shareZip(context, zipFile)
                    } else {
                        Toast.makeText(context, exportFailedText, Toast.LENGTH_SHORT).show()
                    }
                },
                modifier = Modifier.fillMaxWidth(),
            )
            Spacer(modifier = Modifier.height(dims.space8))
            SujianOutlinedButton(
                text = stringResource(id = R.string.btn_clear_logs),
                onClick = {
                    DiagnosticsLogger.clearLogs()
                    EditorEventRingBuffer.clear()
                    Toast.makeText(context, clearedText, Toast.LENGTH_SHORT).show()
                },
                modifier = Modifier.fillMaxWidth(),
            )
            Spacer(modifier = Modifier.height(dims.space8))
            SujianOutlinedButton(
                text = stringResource(id = R.string.btn_copy_device_info),
                onClick = {
                    val deviceInfoJson = DiagnosticsExporter.getDeviceInfoJson(context)
                    val clipboard =
                        context.getSystemService(
                            android.content.Context.CLIPBOARD_SERVICE,
                        ) as android.content.ClipboardManager
                    @Suppress("HardcodedText")
                    clipboard.setPrimaryClip(android.content.ClipData.newPlainText("device_info", deviceInfoJson))
                    Toast.makeText(context, deviceInfoCopiedText, Toast.LENGTH_SHORT).show()
                },
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}
