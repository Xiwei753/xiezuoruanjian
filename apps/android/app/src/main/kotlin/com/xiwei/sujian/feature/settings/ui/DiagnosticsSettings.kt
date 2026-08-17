package com.xiwei.sujian.feature.settings.ui

import android.widget.Toast
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.xiwei.sujian.R
import com.xiwei.sujian.core.designsystem.component.SujianOutlinedButton
import com.xiwei.sujian.core.designsystem.component.SujianSwitchRow
import com.xiwei.sujian.core.diagnostics.DiagnosticsExporter
import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.feature.editor.diagnostics.EditorEventRingBuffer
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * #630 评论13 项2: 扁平 LazyColumn — 向父 [LazyListScope] 注册行，
 * 每个 [SettingsFieldGroup] 是独立 item，有稳定 key。
 */
fun LazyListScope.diagnosticsSettingsItems(vm: SettingsViewModel) {
    item(key = "diagnostics.settings_group") {
        val state by vm.diagnosticsState.collectAsStateWithLifecycle()
        val context = LocalContext.current
        val clearedText = stringResource(id = R.string.diagnostics_cleared)
        val clearFailedText = stringResource(id = R.string.diagnostics_clear_failed)
        val deviceInfoCopiedText = stringResource(id = R.string.diagnostics_device_info_copied)

        SettingsGroupItemContainer(isLast = true, isFirst = true) {
            SettingsFieldGroup(title = stringResource(id = R.string.pref_category_diagnostics)) {
                SujianSwitchRow(
                    title = stringResource(id = R.string.pref_diagnostics_enabled),
                    checked = state.enabled,
                    onCheckedChange = { checked ->
                        DiagnosticsLogger.setEnabled(checked)
                        EditorEventRingBuffer.setEnabled(checked)
                        if (!checked) {
                            DiagnosticsLogger.setVerbose(false)
                        }
                        vm.handleIntent(
                            SettingsIntent.UpdateLocal { current ->
                                current.copy(
                                    diagnosticsEnabled = checked,
                                    diagnosticsVerbose = if (checked) current.diagnosticsVerbose else false,
                                )
                            },
                        )
                    },
                )
                Spacer(modifier = Modifier.height(8.dp))
                SujianSwitchRow(
                    title = stringResource(id = R.string.pref_diagnostics_verbose),
                    checked = state.verbose,
                    onCheckedChange = { checked ->
                        DiagnosticsLogger.setVerbose(checked)
                        vm.handleIntent(SettingsIntent.UpdateLocal { it.copy(diagnosticsVerbose = checked) })
                    },
                    enabled = state.enabled,
                )
                Spacer(modifier = Modifier.height(8.dp))
                ExportDiagnosticsButton(context = context, modifier = Modifier.fillMaxWidth())
                Spacer(modifier = Modifier.height(8.dp))
                ClearLogsButton(
                    context = context,
                    clearedText = clearedText,
                    clearFailedText = clearFailedText,
                    modifier = Modifier.fillMaxWidth(),
                )
                Spacer(modifier = Modifier.height(8.dp))
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
}

/**
 * Issue #612 收口：清空日志按钮。
 */
@androidx.compose.runtime.Composable
private fun ClearLogsButton(
    context: android.content.Context,
    clearedText: String,
    clearFailedText: String,
    modifier: Modifier = Modifier,
) {
    val clearingText = stringResource(id = R.string.diagnostics_clearing)
    var isClearing by remember { mutableStateOf(false) }
    val clearScope = rememberCoroutineScope()
    SujianOutlinedButton(
        text = if (isClearing) clearingText else stringResource(id = R.string.btn_clear_logs),
        enabled = !isClearing,
        onClick = {
            if (isClearing) return@SujianOutlinedButton
            isClearing = true
            clearScope.launch {
                val cleared =
                    withContext(Dispatchers.IO) {
                        DiagnosticsLogger.clearLogs()
                    }
                if (cleared) {
                    EditorEventRingBuffer.clear()
                    Toast.makeText(context, clearedText, Toast.LENGTH_SHORT).show()
                } else {
                    Toast.makeText(context, clearFailedText, Toast.LENGTH_SHORT).show()
                }
                isClearing = false
            }
        },
        modifier = modifier,
    )
}

/**
 * Issue #612 收口：导出按钮。
 */
@androidx.compose.runtime.Composable
private fun ExportDiagnosticsButton(
    context: android.content.Context,
    modifier: Modifier = Modifier,
) {
    val exportFailedText = stringResource(id = R.string.diagnostics_export_failed)
    val exportingText = stringResource(id = R.string.diagnostics_exporting)
    var isExporting by remember { mutableStateOf(false) }
    val exportScope = rememberCoroutineScope()
    SujianOutlinedButton(
        text = if (isExporting) exportingText else stringResource(id = R.string.btn_export_diagnostics),
        enabled = !isExporting,
        onClick = {
            if (isExporting) return@SujianOutlinedButton
            isExporting = true
            exportScope.launch {
                val zipFile =
                    withContext(Dispatchers.IO) {
                        DiagnosticsExporter.export(context.applicationContext)
                    }
                if (zipFile != null) {
                    DiagnosticsExporter.shareZip(context, zipFile)
                } else {
                    Toast.makeText(context, exportFailedText, Toast.LENGTH_SHORT).show()
                }
                isExporting = false
            }
        },
        modifier = modifier,
    )
}
