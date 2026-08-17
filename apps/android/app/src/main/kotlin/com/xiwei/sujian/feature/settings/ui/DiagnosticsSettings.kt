package com.xiwei.sujian.feature.settings.ui

import android.widget.Toast
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
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
 * #630 评论13/评论15: 行级 LazyColumn — 每个真实设置控件是独立 item，有稳定 key。
 */
fun LazyListScope.diagnosticsSettingsItems(vm: SettingsViewModel) {
    // ── 诊断分组标题 ──
    item(key = "diagnostics.settings_title") {
        SettingsGroupItemContainer(isLast = false, isFirst = true) {
            SettingsFieldGroupTitle(title = stringResource(id = R.string.pref_category_diagnostics))
        }
    }

    // 诊断开关
    item(key = "diagnostics.enabled") {
        val state by vm.diagnosticsState.collectAsStateWithLifecycle()
        SettingsGroupItemContainer(isLast = false) {
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
        }
    }

    // 详细日志开关
    item(key = "diagnostics.verbose") {
        val state by vm.diagnosticsState.collectAsStateWithLifecycle()
        SettingsGroupItemContainer(isLast = false) {
            SujianSwitchRow(
                title = stringResource(id = R.string.pref_diagnostics_verbose),
                checked = state.verbose,
                onCheckedChange = { checked ->
                    DiagnosticsLogger.setVerbose(checked)
                    vm.handleIntent(SettingsIntent.UpdateLocal { it.copy(diagnosticsVerbose = checked) })
                },
                enabled = state.enabled,
            )
        }
    }

    // 导出诊断按钮
    item(key = "diagnostics.export") {
        val context = LocalContext.current
        SettingsGroupItemContainer(isLast = false) {
            ExportDiagnosticsButton(context = context, modifier = Modifier.fillMaxWidth())
        }
    }

    // 清空日志按钮
    item(key = "diagnostics.clear") {
        val context = LocalContext.current
        val clearedText = stringResource(id = R.string.diagnostics_cleared)
        val clearFailedText = stringResource(id = R.string.diagnostics_clear_failed)
        SettingsGroupItemContainer(isLast = false) {
            ClearLogsButton(
                context = context,
                clearedText = clearedText,
                clearFailedText = clearFailedText,
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }

    // 复制设备信息按钮
    item(key = "diagnostics.copy_device_info") {
        val context = LocalContext.current
        val deviceInfoCopiedText = stringResource(id = R.string.diagnostics_device_info_copied)
        SettingsGroupItemContainer(isLast = true) {
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
