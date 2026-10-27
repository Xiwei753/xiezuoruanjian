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
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import com.xiwei.sujian.feature.stats.data.WritingStatsRepository
import java.text.DateFormat
import java.util.Date
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.json.JSONObject

/**
 * Issue #1008：手写证明回放。
 *
 * 只渲染 Core `replayChapterProof` 返回的正文、选区和校验结果；拖动时间轴
 * 按章节内事件位置定位，播放间隔由相邻事件真实时间差和倍速计算。
 */
@Composable
internal fun ProofReplayDialog(
    chapterId: String,
    repository: WritingStatsRepository,
    onDismiss: () -> Unit,
) {
    var deviceId by remember(chapterId) { mutableStateOf("") }
    var branches by remember(chapterId) { mutableStateOf(emptyList<String>()) }
    var currentPosition by remember(chapterId) { mutableIntStateOf(-1) }
    var totalEvents by remember(chapterId) { mutableIntStateOf(0) }
    var framePosition by remember(chapterId) { mutableIntStateOf(-1) }
    var timestampMs by remember(chapterId) { mutableLongStateOf(0L) }
    var nextTimestampMs by remember(chapterId) { mutableLongStateOf(0L) }
    var playbackSpeed by remember(chapterId) { mutableFloatStateOf(1f) }
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
            val branchArray = frame.optJSONArray("branches")
            branches =
                if (branchArray == null) {
                    emptyList()
                } else {
                    (0 until branchArray.length()).mapNotNull {
                        branchArray.optJSONObject(it)?.optString("deviceId")?.takeIf { id -> id.isNotEmpty() }
                    }
                }
            currentPosition = frame.optInt("position", 0)
            framePosition = currentPosition
            totalEvents = frame.optInt("totalEvents", 0)
            timestampMs = frame.optLong("timestampMs", 0L)
            nextTimestampMs = frame.optLong("nextTimestampMs", 0L)
            source = frame.optString("source")
            valid = frame.optBoolean("valid")
            errors = message
        }.onFailure { errors = it.message ?: "回放数据解析失败" }
    }

    LaunchedEffect(chapterId, deviceId, currentPosition) {
        val request =
            JSONObject()
                .put("chapterId", chapterId)
                .put("deviceId", deviceId)
        if (currentPosition >= 0) {
            request.put("targetIndex", currentPosition)
        }
        applyFrame(repository.replayChapterProof(request.toString()))
    }

    LaunchedEffect(
        playing,
        currentPosition,
        framePosition,
        totalEvents,
        playbackSpeed,
        timestampMs,
        nextTimestampMs,
    ) {
        if (!playing) return@LaunchedEffect
        if (currentPosition < 0) {
            currentPosition = 0
            return@LaunchedEffect
        }
        if (currentPosition != framePosition) return@LaunchedEffect
        while (playing) {
            if (currentPosition >= totalEvents) {
                playing = false
                break
            }
            val delta = nextTimestampMs - timestampMs
            val baseDelay = if (delta in 1L..60_000L) delta else 260L
            val wait = (baseDelay / playbackSpeed).toLong().coerceIn(40L, 2_000L)
            delay(wait)
            if (!playing) break
            currentPosition += 1
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
                            if (!playing && currentPosition < 0) currentPosition = 0
                            playing = !playing
                        },
                        enabled = totalEvents > 0,
                    ) { Text(if (playing) "暂停" else "播放") }
                    OutlinedButton(
                        onClick = {
                            playing = false
                            if (currentPosition > 0) currentPosition -= 1
                        },
                        enabled = currentPosition > 0,
                    ) { Text("上一步") }
                    OutlinedButton(
                        onClick = {
                            playing = false
                            if (currentPosition < totalEvents) currentPosition += 1
                        },
                        enabled = currentPosition < totalEvents,
                    ) { Text("下一步") }
                }
                if (branches.size > 1) {
                    Row(
                        horizontalArrangement = Arrangement.spacedBy(4.dp),
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        Text("设备分支", style = MaterialTheme.typography.bodySmall)
                        branches.forEach { id ->
                            TextButton(
                                onClick = {
                                    playing = false
                                    deviceId = id
                                    currentPosition = -1
                                },
                            ) {
                                Text(
                                    id,
                                    color =
                                        if (deviceId == id) {
                                            MaterialTheme.colorScheme.primary
                                        } else {
                                            MaterialTheme.colorScheme.onSurface
                                        },
                                )
                            }
                        }
                    }
                }
                Row(
                    horizontalArrangement = Arrangement.spacedBy(4.dp),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text("倍速", style = MaterialTheme.typography.bodySmall)
                    listOf(0.5f, 1f, 2f, 4f).forEach { speed ->
                        TextButton(onClick = { playbackSpeed = speed }) {
                            Text(
                                if (speed == speed.toInt().toFloat()) "${speed.toInt()}x" else "${speed}x",
                                color =
                                    if (playbackSpeed == speed) {
                                        MaterialTheme.colorScheme.primary
                                    } else {
                                        MaterialTheme.colorScheme.onSurface
                                    },
                            )
                        }
                    }
                }
                Slider(
                    value = currentPosition.coerceAtLeast(0).toFloat(),
                    onValueChange = {
                        playing = false
                        currentPosition = it.toInt()
                    },
                    valueRange = 0f..totalEvents.coerceAtLeast(1).toFloat(),
                    steps = (totalEvents - 1).coerceAtLeast(0),
                    enabled = totalEvents > 0,
                )
                Text(
                    "事件 ${currentPosition.coerceAtLeast(0)} / $totalEvents · 来源 $source",
                    style = MaterialTheme.typography.bodySmall,
                )
                Text(
                    if (timestampMs > 0) {
                        "发生时间 " + DateFormat.getDateTimeInstance().format(Date(timestampMs))
                    } else {
                        "事件时间未知"
                    },
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
                                    repository.exportChapterProof(chapterId, deviceId)
                                        ?: "导出失败"
                            }
                        },
                        enabled = totalEvents > 0,
                    ) { Text("导出证明包（含历史正文）") }
                    TextButton(
                        onClick = onDismiss,
                    ) { Text("关闭") }
                }
            }
        }
    }
}
