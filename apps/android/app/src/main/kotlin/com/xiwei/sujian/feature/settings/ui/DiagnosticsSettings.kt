package com.xiwei.sujian.feature.settings.ui

import android.widget.Toast
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import com.xiwei.sujian.R
import com.xiwei.sujian.core.designsystem.component.SujianOutlinedButton
import com.xiwei.sujian.core.designsystem.component.SujianSwitchRow
import com.xiwei.sujian.core.designsystem.theme.LocalSujianDimensions
import com.xiwei.sujian.core.diagnostics.DiagnosticsExporter
import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.feature.editor.diagnostics.EditorEventRingBuffer
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

@Composable
fun DiagnosticsSettings(
    state: DiagnosticsSectionState,
    onIntent: (SettingsIntent) -> Unit,
    modifier: Modifier = Modifier,
) {
    val context = LocalContext.current
    val dims = LocalSujianDimensions.current
    val clearedText = stringResource(id = R.string.diagnostics_cleared)
    val clearFailedText = stringResource(id = R.string.diagnostics_clear_failed)
    val deviceInfoCopiedText = stringResource(id = R.string.diagnostics_device_info_copied)

    androidx.compose.foundation.layout.Column(
        modifier = modifier.padding(dims.space16),
        verticalArrangement = Arrangement.spacedBy(dims.space16),
    ) {
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
                    // 保持整份 LocalSettings 的其余字段：只在当前真相上改诊断开关，
                    // 关闭诊断时同时关闭 verbose（与旧 settings.copy 语义一致）。
                    onIntent(
                        SettingsIntent.UpdateLocal { current ->
                            current.copy(
                                diagnosticsEnabled = checked,
                                diagnosticsVerbose = if (checked) current.diagnosticsVerbose else false,
                            )
                        },
                    )
                },
            )
            Spacer(modifier = Modifier.height(dims.space8))
            SujianSwitchRow(
                title = stringResource(id = R.string.pref_diagnostics_verbose),
                checked = state.verbose,
                onCheckedChange = { checked ->
                    DiagnosticsLogger.setVerbose(checked)
                    onIntent(SettingsIntent.UpdateLocal { it.copy(diagnosticsVerbose = checked) })
                },
                enabled = state.enabled,
            )
            Spacer(modifier = Modifier.height(dims.space8))
            ExportDiagnosticsButton(context = context, modifier = Modifier.fillMaxWidth())
            Spacer(modifier = Modifier.height(dims.space8))
            ClearLogsButton(
                context = context,
                clearedText = clearedText,
                clearFailedText = clearFailedText,
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

/**
 * Issue #612 收口：清空日志按钮。
 *
 * clearLogs 要等 writer 线程处理 ClearBarrier（最坏 [PersistentLogWriter] 5s 超时），
 * 必须在后台线程执行，避免阻塞主线程触发 ANR（与导出按钮同模式，见 f87672b9）；
 * isClearing 防止重复点击。失败（超时/中断/删除失败）如实显示失败，不得假装已清空
 * （Issue #612 评论 3.4）；成功才同时清内存 ring buffer 并提示成功。
 */
@Composable
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
 *
 * 导出（flush 屏障 + 多来源采集 + zip）是数 MB 级 I/O，必须在后台线程执行，
 * 避免阻塞 UI 线程触发 ANR；isExporting 防止重复点击并发导出，完成后回主线程
 * 分享或提示失败。flushBlocking 的失败语义由 DiagnosticsExporter.export() 内部
 * 统一处理（失败直接返回 null，不打缺日志的 zip）。
 */
@Composable
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
                    // shareZip 需要 Activity context 启动分享，回主线程执行。
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
