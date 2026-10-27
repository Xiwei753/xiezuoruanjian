package com.xiwei.sujian.feature.editor.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Slider
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import com.xiwei.sujian.feature.stats.data.WritingStatsRepository
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.json.JSONObject

/**
 * Issue #1008：手写证明回放。
 *
 * 只渲染 Core `replayChapterProof` 返回的正文、选区和校验结果；拖动时间轴
 * 按设备内 seq 定位，不在 UI 侧推演编辑规则。
 */
@Composable
internal fun ProofReplayDialog(
    chapterId: String,
    repository: WritingStatsRepository,
    onDismiss: () -> Unit,
) {
    var currentSeq by remember(chapterId) { mutableIntStateOf(-1) }
    var lastSeq by remember(chapterId) { mutableIntStateOf(0) }
    var text by remember(chapterId) { mutableStateOf("") }
    var source by remember(chapterId) { mutableStateOf("") }
    var valid by remember(chapterId) { mutableStateOf(false) }
    var errors by remember(chapterId) { mutableStateOf("") }
    var playing by remember(chapterId) { mutableStateOf(false) }
    var exportJson by remember(chapterId) { mutableStateOf("") }
    val scope = rememberCoroutineScope()

    fun applyFrame(raw: String?) {
        if (raw == null) return
        runCatching {
            val frame = JSONObject(raw)
            val frameErrors = frame.optJSONArray("errors")
            val message =
                if (frameErrors == null) {
                    ""
                } else {
                    (0 until frameErrors.length()).joinToString("\n") {
                        frameErrors.optString(it)
                    }
                }
            text = frame.optString("text")
            currentSeq = frame.optInt("seq", 0)
            lastSeq = frame.optInt("lastSeq", 0)
            source = frame.optString("source")
            valid = frame.optBoolean("valid")
            errors = message
        }.onFailure { errors = it.message ?: "回放数据解析失败" }
    }

    LaunchedEffect(chapterId, currentSeq) {
        val request =
            JSONObject()
                .put("chapterId", chapterId)
                .put("deviceId", "")
        if (currentSeq >= 0) {
            request.put("targetSeq", currentSeq)
        }
        applyFrame(repository.replayChapterProof(request.toString()))
    }

    LaunchedEffect(playing, currentSeq, lastSeq) {
        if (!playing) return@LaunchedEffect
        while (playing) {
            delay(260)
            if (currentSeq >= lastSeq) {
                playing = false
            } else {
                currentSeq += 1
            }
        }
    }

    Dialog(onDismissRequest = onDismiss) {
        Surface(
            shape = MaterialTheme.shapes.large,
            tonalElevation = 6.dp,
            modifier = Modifier.fillMaxWidth().padding(16.dp),
        ) {
            Column(
                modifier = Modifier.padding(20.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Text("手写证明回放", style = MaterialTheme.typography.titleLarge)
                Row(
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Button(
                        onClick = {
                            if (!playing && currentSeq < 0) currentSeq = 0
                            playing = !playing
                        },
                        enabled = lastSeq > 0,
                    ) { Text(if (playing) "暂停" else "播放") }
                    OutlinedButton(
                        onClick = {
                            playing = false
                            if (currentSeq > 0) currentSeq -= 1
                        },
                        enabled = currentSeq > 0,
                    ) { Text("上一步") }
                    OutlinedButton(
                        onClick = {
                            playing = false
                            if (currentSeq < lastSeq) currentSeq += 1
                        },
                        enabled = currentSeq < lastSeq,
                    ) { Text("下一步") }
                }
                Slider(
                    value = currentSeq.toFloat(),
                    onValueChange = {
                        playing = false
                        currentSeq = it.toInt()
                    },
                    valueRange = 0f..lastSeq.coerceAtLeast(1).toFloat(),
                    steps = (lastSeq - 1).coerceAtLeast(0),
                    enabled = lastSeq > 0,
                )
                Text(
                    "事件 $currentSeq / $lastSeq · 来源 $source",
                    style = MaterialTheme.typography.bodySmall,
                )
                Text(
                    text = text.ifEmpty { "该章节还没有可回放的编辑事件" },
                    style = MaterialTheme.typography.bodyLarge,
                    modifier =
                        Modifier
                            .fillMaxWidth()
                            .heightIn(min = 220.dp, max = 420.dp)
                            .verticalScroll(rememberScrollState()),
                )
                if (errors.isNotEmpty()) {
                    Text(
                        errors,
                        color = MaterialTheme.colorScheme.error,
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
                Text(
                    if (valid) "哈希链校验通过" else "校验未通过或记录不完整",
                    color =
                        if (valid) {
                            MaterialTheme.colorScheme.primary
                        } else {
                            MaterialTheme.colorScheme.error
                        },
                    style = MaterialTheme.typography.bodySmall,
                )
                if (exportJson.isNotEmpty()) {
                    Text(
                        exportJson,
                        fontFamily = FontFamily.Monospace,
                        style = MaterialTheme.typography.bodySmall,
                        modifier =
                            Modifier
                                .fillMaxWidth()
                                .heightIn(max = 160.dp)
                                .verticalScroll(rememberScrollState()),
                    )
                }
                Row(
                    horizontalArrangement = Arrangement.End,
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    TextButton(
                        onClick = {
                            scope.launch {
                                // 证明包包含历史正文；按钮文案已明确提示。
                                exportJson =
                                    repository.exportChapterProof(chapterId, "")
                                        ?: "导出失败"
                            }
                        },
                        enabled = lastSeq > 0,
                    ) { Text("导出证明包（含历史正文）") }
                    TextButton(
                        onClick = onDismiss,
                    ) { Text("关闭") }
                }
            }
        }
    }
}
