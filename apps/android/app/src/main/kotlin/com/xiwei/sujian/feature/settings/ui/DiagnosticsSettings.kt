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
 * #632 评论 5377052579：诊断设置 — 每个重控件一个 Lazy item。
 *
 * 诊断分组: 标题 + 开关 + 详细开关
 * 操作分组: 导出 + 清空 + 复制设备信息
 *
 * 用 [SettingsExpandedFieldContainer] + [ExpandedFieldPosition] 让同一字段组的
 * 多个 item 视觉上连成一张大卡。每个 item 只 collect 自己需要的 row-level StateFlow。
 */
fun LazyListScope.diagnosticsSettingsItems(
    vm: SettingsViewModel,
    closeOuterGroup: Boolean,
) {
    // ── 诊断分组（标题 + 开关 + 详细开关）— 每个 item 独立 ──

    item(key = "diagnostics.title", contentType = CONTENT_TYPE_FIELD_TITLE) {
        SettingsExpandedFieldContainer(
            position = ExpandedFieldPosition.First,
            closeOuterGroup = false,
        ) {
            SettingsFieldGroupTitle(title = stringResource(id = R.string.pref_category_diagnostics))
        }
    }

    item(key = "diagnostics.enabled", contentType = CONTENT_TYPE_SWITCH) {
        val enabled by vm.diagnosticsEnabledRow.collectAsStateWithLifecycle()
        SettingsExpandedFieldContainer(
            position = ExpandedFieldPosition.Middle,
            closeOuterGroup = false,
        ) {
            SujianSwitchRow(
                title = stringResource(id = R.string.pref_diagnostics_enabled),
                checked = enabled,
                onCheckedChange = { checked ->
                    DiagnosticsLogger.setEnabled(checked)
                    EditorEventRingBuffer.setEnabled(checked)
                    if (!checked) DiagnosticsLogger.setVerbose(false)
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

    item(key = "diagnostics.verbose", contentType = CONTENT_TYPE_SWITCH) {
        val enabled by vm.diagnosticsEnabledRow.collectAsStateWithLifecycle()
        val verbose by vm.diagnosticsVerboseRow.collectAsStateWithLifecycle()
        SettingsExpandedFieldContainer(
            position = ExpandedFieldPosition.Last,
            closeOuterGroup = false,
        ) {
            SujianSwitchRow(
                title = stringResource(id = R.string.pref_diagnostics_verbose),
                checked = verbose,
                enabled = enabled,
                onCheckedChange = { checked ->
                    DiagnosticsLogger.setVerbose(checked)
                    vm.handleIntent(
                        SettingsIntent.UpdateLocal { current ->
                            current.copy(diagnosticsVerbose = checked)
                        },
                    )
                },
            )
        }
    }

    // ── 操作分组（导出 + 清空 + 复制设备信息）— 每个 item 独立 ──

    diagnosticsActionItems(vm, closeOuterGroup)
}

// ── 诊断动作按钮 item：导出 / 清空 / 复制设备信息 — 每个按钮独立 item ──

private fun LazyListScope.diagnosticsActionItems(
    vm: SettingsViewModel,
    closeOuterGroup: Boolean,
) {
    item(key = "diagnostics.actions.export", contentType = CONTENT_TYPE_BUTTON) {
        val context = LocalContext.current
        SettingsExpandedFieldContainer(
            position = ExpandedFieldPosition.First,
            closeOuterGroup = false,
        ) {
            ExportDiagnosticsButton(
                context = context,
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }

    item(key = "diagnostics.actions.clear", contentType = CONTENT_TYPE_BUTTON) {
        val context = LocalContext.current
        val clearedText = stringResource(id = R.string.diagnostics_cleared)
        val clearFailedText = stringResource(id = R.string.diagnostics_clear_failed)
        SettingsExpandedFieldContainer(
            position = ExpandedFieldPosition.Middle,
            closeOuterGroup = false,
        ) {
            ClearLogsButton(
                context = context,
                clearedText = clearedText,
                clearFailedText = clearFailedText,
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }

    item(key = "diagnostics.actions.copy_device_info", contentType = CONTENT_TYPE_BUTTON) {
        val context = LocalContext.current
        val deviceInfoCopiedText = stringResource(id = R.string.diagnostics_device_info_copied)
        SettingsExpandedFieldContainer(
            position = ExpandedFieldPosition.Last,
            closeOuterGroup = closeOuterGroup,
        ) {
            CopyDeviceInfoButton(
                context = context,
                copiedText = deviceInfoCopiedText,
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}

// ── 按钮 Composable ──

@androidx.compose.runtime.Composable
private fun CopyDeviceInfoButton(
    context: android.content.Context,
    copiedText: String,
    modifier: Modifier = Modifier,
) {
    SujianOutlinedButton(
        text = stringResource(id = R.string.btn_copy_device_info),
        onClick = {
            val json = DiagnosticsExporter.getDeviceInfoJson(context)
            val clipboard =
                context.getSystemService(android.content.Context.CLIPBOARD_SERVICE)
                    as android.content.ClipboardManager
            @Suppress("HardcodedText")
            clipboard.setPrimaryClip(android.content.ClipData.newPlainText("device_info", json))
            Toast.makeText(context, copiedText, Toast.LENGTH_SHORT).show()
        },
        modifier = modifier,
    )
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
