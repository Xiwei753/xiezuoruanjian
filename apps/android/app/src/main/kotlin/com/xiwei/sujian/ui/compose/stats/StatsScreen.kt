package com.xiwei.sujian.ui.compose.stats

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.data.BridgeProvider
import com.xiwei.sujian.model.ProjectWritingStatsItem
import com.xiwei.sujian.model.WritingStatsSummary
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.time.LocalDate
import java.time.format.DateTimeFormatter

@Composable
fun StatsScreen(
    modifier: Modifier = Modifier
) {
    val context = LocalContext.current
    var summary by remember { mutableStateOf<WritingStatsSummary?>(null) }
    var projectItems by remember { mutableStateOf<List<ProjectWritingStatsItem>>(emptyList()) }
    var isLoading by remember { mutableStateOf(true) }

    LaunchedEffect(Unit) {
        val today = LocalDate.now()
        val startDate = today.minusDays(30).format(DateTimeFormatter.ISO_LOCAL_DATE)
        val endDate = today.format(DateTimeFormatter.ISO_LOCAL_DATE)

        withContext(Dispatchers.IO) {
            try {
                val bridge = BridgeProvider.getStatsBridge(context)
                when (val result = bridge.getWritingStatsSummary(startDate, endDate)) {
                    is com.xiwei.sujian.data.BridgeResult.Success -> summary = result.data
                    else -> {}
                }
                when (val result = bridge.getWritingStatsByProject(startDate, endDate)) {
                    is com.xiwei.sujian.data.BridgeResult.Success -> projectItems = result.data.projects ?: emptyList()
                    else -> {}
                }
            } catch (_: Exception) { }
        }
        isLoading = false
    }

    if (isLoading) {
        Box(modifier = modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
            Text("加载中...", style = MaterialTheme.typography.bodyLarge)
        }
        return
    }

    LazyColumn(
        contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
        modifier = modifier.fillMaxSize()
    ) {
        item {
            Text("近 30 天写作统计", style = MaterialTheme.typography.titleMedium)
            Spacer(modifier = Modifier.height(16.dp))
        }

        summary?.let { s ->
            item {
                Card(
                    modifier = Modifier.fillMaxWidth().padding(bottom = 12.dp),
                    colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow)
                ) {
                    Column(modifier = Modifier.padding(16.dp)) {
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.SpaceBetween
                        ) {
                            StatItem("总字数", "${s.totalWordCount}")
                            StatItem("活跃天数", "${s.activeDays}")
                            StatItem("总时长", formatDuration(s.totalTimeSeconds))
                        }
                        Spacer(modifier = Modifier.height(12.dp))
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.SpaceBetween
                        ) {
                            StatItem("手动输入", "${s.totalHumanTypedChars ?: 0}")
                            StatItem("活跃时长", formatDuration(s.totalActiveSeconds ?: 0L))
                            StatItem("会话数", "${s.totalSessions ?: 0}")
                        }
                    }
                }
            }
        }

        if (projectItems.isNotEmpty()) {
            item {
                Text("按作品统计", style = MaterialTheme.typography.titleSmall)
                Spacer(modifier = Modifier.height(8.dp))
            }
            items(projectItems, key = { it.projectId ?: "" }) { item ->
                Card(
                    modifier = Modifier.fillMaxWidth().padding(bottom = 8.dp),
                    colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow)
                ) {
                    Column(modifier = Modifier.padding(16.dp)) {
                        Text(item.projectTitle ?: "", style = MaterialTheme.typography.titleSmall)
                        Text("${item.netDeltaChars ?: 0} 字 · ${formatDuration(item.activeSeconds ?: 0)}", style = MaterialTheme.typography.bodySmall)
                    }
                }
            }
        }

        if (summary == null && projectItems.isEmpty()) {
            item {
                Box(modifier = Modifier.fillMaxSize().padding(32.dp), contentAlignment = Alignment.Center) {
                    Text("暂无统计数据", style = MaterialTheme.typography.bodyLarge)
                }
            }
        }
    }
}

@Composable
private fun StatItem(label: String, value: String) {
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        Text(value, style = MaterialTheme.typography.titleMedium)
        Text(label, style = MaterialTheme.typography.bodySmall)
    }
}

private fun formatDuration(seconds: Long): String {
    if (seconds < 60) return "${seconds}秒"
    val minutes = seconds / 60
    if (minutes < 60) return "${minutes}分钟"
    val hours = minutes / 60
    val remainMinutes = minutes % 60
    return if (remainMinutes > 0) "${hours}小时${remainMinutes}分钟" else "${hours}小时"
}
