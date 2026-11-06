package com.xiwei.sujian.feature.starmap.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Slider
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextMeasurer
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.xiwei.sujian.app.di.LocalSujianAppDependencies
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.json.JSONObject

/**
 * Issue #1036：星图创作记录 / 手写证明回放。
 *
 * 只渲染 Core 返回的历史图状态；拖动时间轴、播放、设备分支切换都重新向
 * Core 请求目标位置，Android 端不重算事件顺序，也不修改真实星图。
 */
@Composable
fun StarMapProofReplayDialog(
    starMapId: String,
    onDismiss: () -> Unit,
) {
    val dependencies = LocalSujianAppDependencies.current
    val repository = dependencies.statsRepository
    val scope = rememberCoroutineScope()
    val textMeasurer = rememberTextMeasurer()
    var frame by remember(starMapId) { mutableStateOf<JSONObject?>(null) }
    var deviceId by remember(starMapId) { mutableStateOf("") }
    var position by remember(starMapId) { mutableIntStateOf(0) }
    var totalEvents by remember(starMapId) { mutableIntStateOf(0) }
    var playing by remember(starMapId) { mutableStateOf(false) }
    var speed by remember(starMapId) { mutableFloatStateOf(1f) }
    var error by remember(starMapId) { mutableStateOf<String?>(null) }

    suspend fun requestFrame(target: Int) {
        val request = JSONObject()
            .put("starmapId", starMapId)
            .put("deviceId", deviceId)
            .put("targetPosition", target.coerceAtLeast(0))
            .put("includeTrajectory", true)
        val raw = repository.replayStarMapProof(request.toString())
        if (raw == null) {
            error = "无法读取星图创作记录"
            return
        }
        val parsed = runCatching { JSONObject(raw) }.getOrNull()
        if (parsed == null) {
            error = "星图创作记录格式错误"
            return
        }
        if (deviceId.isEmpty()) {
            deviceId = parsed.optString("deviceId")
        }
        frame = parsed
        totalEvents = parsed.optInt("totalEvents", 0)
        position = parsed.optInt("position", target)
        error = null
    }

    LaunchedEffect(starMapId) { requestFrame(0) }

    LaunchedEffect(playing, speed) {
        while (playing && position < totalEvents) {
            delay((200f / speed.coerceAtLeast(0.25f)).toLong())
            requestFrame((position + 1).coerceAtMost(totalEvents))
            if (position >= totalEvents) playing = false
        }
    }

    val graph = frame?.optJSONObject("state")?.optJSONObject("graph")
    val nodes = graph?.optJSONArray("nodes")
    val edges = graph?.optJSONArray("edges")
    val event = frame?.optJSONObject("event")
    val historyComplete = frame?.optBoolean("historyComplete", true) ?: true

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("星图创作记录") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Canvas(
                    modifier = Modifier
                        .fillMaxWidth()
                        .height(300.dp),
                ) {
                    val graphNodes = nodes ?: return@Canvas
                    var minX = 0f
                    var minY = 0f
                    var maxX = 1f
                    var maxY = 1f
                    var seen = false
                    val points = HashMap<String, Offset>()
                    for (index in 0 until graphNodes.length()) {
                        val node = graphNodes.optJSONObject(index) ?: continue
                        val point = node.optJSONObject("position") ?: continue
                        val x = point.optDouble("x", 0.0).toFloat()
                        val y = point.optDouble("y", 0.0).toFloat()
                        points[node.optString("id")] = Offset(x, y)
                        if (!seen) {
                            minX = x; maxX = x; minY = y; maxY = y; seen = true
                        }
                        minX = minOf(minX, x); maxX = maxOf(maxX, x)
                        minY = minOf(minY, y); maxY = maxOf(maxY, y)
                    }
                    if (!seen) return@Canvas
                    val spanX = (maxX - minX).coerceAtLeast(1f)
                    val spanY = (maxY - minY).coerceAtLeast(1f)
                    val scale = minOf((size.width - 48f) / spanX, (size.height - 48f) / spanY)
                        .coerceIn(0.2f, 2.2f)
                    fun screen(point: Offset): Offset = Offset(
                        size.width / 2 + (point.x - (minX + maxX) / 2) * scale,
                        size.height / 2 + (point.y - (minY + maxY) / 2) * scale,
                    )
                    val edgeColor = Color(0xFF9AA4B2)
                    val graphEdges = edges ?: return@Canvas
                    for (index in 0 until graphEdges.length()) {
                        val edge = graphEdges.optJSONObject(index) ?: continue
                        val from = edge.optJSONObject("from")
                            ?.optJSONObject("target")?.optString("nodeId").orEmpty()
                        val to = edge.optJSONObject("to")
                            ?.optJSONObject("target")?.optString("nodeId").orEmpty()
                        val start = points[from] ?: continue
                        val end = points[to] ?: continue
                        drawLine(edgeColor, screen(start), screen(end), strokeWidth = 2f)
                    }
                    val activeId = event?.optString("objectId").orEmpty()
                    for (index in 0 until graphNodes.length()) {
                        val node = graphNodes.optJSONObject(index) ?: continue
                        val point = points[node.optString("id")] ?: continue
                        val center = screen(point)
                        val active = node.optString("id") == activeId
                        drawCircle(
                            color = if (active) Color(0xFF4F6DF5) else Color(0xFFD8DEF0),
                            radius = if (active) 14f else 10f,
                            center = center,
                        )
                        drawText(
                            textMeasurer = textMeasurer,
                            text = node.optString("title").ifEmpty { node.optString("id") },
                            topLeft = center + Offset(12f, -8f),
                            style = TextStyle(fontSize = 10.sp, color = Color(0xFF30343B)),
                            maxLines = 1,
                        )
                    }
                    val embeds = graph?.optJSONArray("embeds")
                    if (embeds != null) {
                        for (index in 0 until embeds.length()) {
                            val embed = embeds.optJSONObject(index) ?: continue
                            val point = embed.optJSONObject("position") ?: continue
                            val center = screen(
                                Offset(
                                    point.optDouble("x", 0.0).toFloat(),
                                    point.optDouble("y", 0.0).toFloat(),
                                ),
                            )
                            drawCircle(
                                color = Color(0xFFE7E0FF),
                                radius = 16f,
                                center = center,
                            )
                            drawCircle(
                                color = Color(0xFF6F7FD8),
                                radius = 16f,
                                center = center,
                                style = androidx.compose.ui.graphics.drawscope.Stroke(width = 2f),
                            )
                        }
                    }
                }
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    OutlinedButton(
                        onClick = {
                            if (position >= totalEvents) position = 0
                            playing = !playing
                        },
                        enabled = totalEvents > 0,
                    ) {
                        Text(if (playing) "暂停" else "播放")
                    }
                    Text("${position} / ${totalEvents}")
                    OutlinedButton(onClick = { speed = if (speed >= 4f) 0.5f else speed * 2f }) {
                        Text("${speed}x")
                    }
                }
                Slider(
                    value = position.toFloat(),
                    onValueChange = { value ->
                        playing = false
                        position = value.toInt()
                    },
                    onValueChangeFinished = {
                        scope.launch { requestFrame(position) }
                    },
                    valueRange = 0f..totalEvents.coerceAtLeast(1).toFloat(),
                )
                Text(
                    text = event?.let {
                        "${it.optString("actor")} · ${it.optString("entrypoint")} · " +
                            "${it.optString("operation")} ${it.optString("objectType")}"
                    } ?: "起始状态",
                    style = MaterialTheme.typography.bodySmall,
                )
                if (!historyComplete) {
                    Text(
                        text = "此前历史不完整（从真实基线开始）",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                error?.let {
                    Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error)
                }
            }
        },
        confirmButton = {
            TextButton(
                onClick = {
                    scope.launch { requestFrame(position) }
                },
            ) {
                Text("跳到该时刻")
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text("关闭") }
        },
    )
}
