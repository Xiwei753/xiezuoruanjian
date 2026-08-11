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
import com.xiwei.sujian.core.designsystem.component.SujianSection
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
    state: SettingsUiState,
    onIntent: (SettingsIntent) -> Unit,
    modifier: Modifier = Modifier,
) {
    val context = LocalContext.current
    val settings = state.settings
    val dims = LocalSujianDimensions.current
    val clearedText = stringResource(id = R.string.diagnostics_cleared)
    val clearFailedText = stringResource(id = R.string.diagnostics_clear_failed)
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
            ExportDiagnosticsButton(context = context, modifier = Modifier.fillMaxWidth())
            Spacer(modifier = Modifier.height(dims.space8))
            SujianOutlinedButton(
                text = stringResource(id = R.string.btn_clear_logs),
                onClick = {
                    // Issue #612 评论 3.4：清空失败（writer 超时/中断）必须显示失败，
                    // 不得假装已清空；成功才同时清内存 ring buffer 并提示成功。
                    if (DiagnosticsLogger.clearLogs()) {
                        EditorEventRingBuffer.clear()
                        Toast.makeText(context, clearedText, Toast.LENGTH_SHORT).show()
                    } else {
                        Toast.makeText(context, clearFailedText, Toast.LENGTH_SHORT).show()
                    }
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
