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
import com.xiwei.sujian.feature.settings.data.model.LocalSettings
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * #630 评论13/评论15: 行级 LazyColumn — 每个真实设置控件是独立 item，有稳定 key。
 * 使用 [SettingsFieldRowContainer] 的 isFirst/isLast 保持 M3 高色阶卡片视觉。
 * 每个 item 只 collect 自己的 row-level StateFlow，避免整分类重组。
 *
 * 结构拆分（detekt LongMethod 阈值 80）：
 * - [diagnosticsSwitchItem]：通用诊断开关 item（enabled / verbose 共享同一模式）
 * - [CopyDeviceInfoButton]：复制设备信息按钮（含 Clipboard 操作）
 * - [ClearLogsButton]、[ExportDiagnosticsButton]：已有独立 Composable
 */
fun LazyListScope.diagnosticsSettingsItems(vm: SettingsViewModel) {
    diagnosticsSwitchItem(
        key = "diagnostics.enabled",
        vm = vm,
        isChecked = { it.enabled },
        onCheckedChange = { checked, current ->
            DiagnosticsLogger.setEnabled(checked)
            EditorEventRingBuffer.setEnabled(checked)
            if (!checked) DiagnosticsLogger.setVerbose(false)
            current.copy(
                diagnosticsEnabled = checked,
                diagnosticsVerbose = if (checked) current.diagnosticsVerbose else false,
            )
        },
    )

    diagnosticsSwitchItem(
        key = "diagnostics.verbose",
        vm = vm,
        isChecked = { it.verbose },
        onCheckedChange = { checked, current ->
            DiagnosticsLogger.setVerbose(checked)
            current.copy(diagnosticsVerbose = checked)
        },
        switchEnabled = { it.enabled },
    )

    item(key = "diagnostics.export") {
        val context = LocalContext.current
        SettingsGroupItemContainer(isLast = false) {
            SettingsFieldRowContainer(isFirst = false, isLast = false) {
                ExportDiagnosticsButton(context = context, modifier = Modifier.fillMaxWidth())
            }
        }
    }

    item(key = "diagnostics.clear") {
        val context = LocalContext.current
        val clearedText = stringResource(id = R.string.diagnostics_cleared)
        val clearFailedText = stringResource(id = R.string.diagnostics_clear_failed)
        SettingsGroupItemContainer(isLast = false) {
            SettingsFieldRowContainer(isFirst = false, isLast = false) {
                ClearLogsButton(
                    context = context,
                    clearedText = clearedText,
                    clearFailedText = clearFailedText,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        }
    }

    item(key = "diagnostics.copy_device_info") {
        val context = LocalContext.current
        val deviceInfoCopiedText = stringResource(id = R.string.diagnostics_device_info_copied)
        SettingsGroupItemContainer(isLast = true) {
            SettingsFieldRowContainer(isFirst = false, isLast = true) {
                CopyDeviceInfoButton(
                    context = context,
                    copiedText = deviceInfoCopiedText,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        }
    }
}

// ── 辅助 item：诊断开关（enabled / verbose 共享模式） ──

private fun LazyListScope.diagnosticsSwitchItem(
    key: String,
    vm: SettingsViewModel,
    isChecked: (DiagnosticsSectionState) -> Boolean,
    onCheckedChange: (Boolean, LocalSettings) -> LocalSettings,
    switchEnabled: ((DiagnosticsSectionState) -> Boolean)? = null,
) {
    item(key = key) {
        val enabled by vm.diagnosticsEnabledRow.collectAsStateWithLifecycle()
        val verbose by vm.diagnosticsVerboseRow.collectAsStateWithLifecycle()
        val state = DiagnosticsSectionState(enabled = enabled, verbose = verbose)
        SettingsGroupItemContainer(isLast = false) {
            SettingsFieldRowContainer(isFirst = false, isLast = false) {
                SujianSwitchRow(
                    title =
                        stringResource(
                            id =
                                when (key) {
                                    "diagnostics.enabled" -> R.string.pref_diagnostics_enabled
                                    else -> R.string.pref_diagnostics_verbose
                                },
                        ),
                    checked = isChecked(state),
                    onCheckedChange = { checked ->
                        vm.handleIntent(
                            SettingsIntent.UpdateLocal { current ->
                                onCheckedChange(checked, current)
                            },
                        )
                    },
                    enabled = switchEnabled?.invoke(state) ?: true,
                )
            }
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
