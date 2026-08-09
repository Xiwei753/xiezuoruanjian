package com.xiwei.sujian.feature.stats.ui

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
import androidx.compose.ui.res.stringResource
import com.xiwei.sujian.R
import com.xiwei.sujian.core.designsystem.component.SujianCard
import com.xiwei.sujian.core.designsystem.theme.LocalSujianDimensions
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.stats.data.WritingStatsRepositoryProvider
import com.xiwei.sujian.feature.stats.data.model.ProjectWritingStatsItem
import com.xiwei.sujian.feature.stats.data.model.WritingStatsSummary
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.time.LocalDate
import java.time.format.DateTimeFormatter

@Composable
fun StatsScreen(modifier: Modifier = Modifier) {
    val context = LocalContext.current
    val dims = LocalSujianDimensions.current
    var summary by remember { mutableStateOf<WritingStatsSummary?>(null) }
    var projectItems by remember { mutableStateOf<List<ProjectWritingStatsItem>>(emptyList()) }
    var isLoading by remember { mutableStateOf(true) }

    LaunchedEffect(Unit) {
        val today = LocalDate.now()
        val startDate = today.minusDays(30).format(DateTimeFormatter.ISO_LOCAL_DATE)
        val endDate = today.format(DateTimeFormatter.ISO_LOCAL_DATE)

        withContext(Dispatchers.IO) {
            try {
                val bridge = WritingStatsRepositoryProvider.getStatsBridge(context)
                when (val result = bridge.getWritingStatsSummary(startDate, endDate)) {
                    is BridgeResult.Success -> summary = result.data
                    else -> {}
                }
                when (val result = bridge.getWritingStatsByProject(startDate, endDate)) {
                    is BridgeResult.Success -> projectItems = result.data.projects ?: emptyList()
                    else -> {}
                }
            } catch (_: Exception) {
            }
        }
        isLoading = false
    }

    if (isLoading) {
        Box(modifier = modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
            Text(stringResource(id = R.string.loading), style = MaterialTheme.typography.bodyLarge)
        }
        return
    }

    LazyColumn(
        contentPadding = PaddingValues(horizontal = dims.space16, vertical = dims.space8),
        modifier = modifier.fillMaxSize(),
    ) {
        item {
            Text(stringResource(id = R.string.stats_recent_30_days), style = MaterialTheme.typography.titleMedium)
            Spacer(modifier = Modifier.height(dims.space16))
        }

        summary?.let { s ->
            item {
                SujianCard(
                    modifier = Modifier.fillMaxWidth().padding(bottom = dims.space12),
                ) {
                    Column(modifier = Modifier.padding(dims.space16)) {
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.SpaceBetween,
                        ) {
                            StatItem(stringResource(id = R.string.stats_total_words_label), "${s.totalWordCount}")
                            StatItem(stringResource(id = R.string.stats_active_days), "${s.activeDays}")
                            StatItem(
                                stringResource(id = R.string.stats_total_duration),
                                formatDuration(s.totalTimeSeconds, context),
                            )
                        }
                        Spacer(modifier = Modifier.height(dims.space12))
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.SpaceBetween,
                        ) {
                            StatItem(stringResource(id = R.string.stats_manual_input), "${s.totalHumanTypedChars ?: 0}")
                            StatItem(
                                stringResource(id = R.string.stats_active_duration),
                                formatDuration(s.totalActiveSeconds ?: 0L, context),
                            )
                            StatItem(stringResource(id = R.string.stats_session_count), "${s.totalSessions ?: 0}")
                        }
                    }
                }
            }
        }

        if (projectItems.isNotEmpty()) {
            item {
                Text(stringResource(id = R.string.stats_by_project), style = MaterialTheme.typography.titleSmall)
                Spacer(modifier = Modifier.height(dims.space8))
            }
            items(projectItems, key = { it.projectId ?: "" }) { item ->
                SujianCard(
                    modifier = Modifier.fillMaxWidth().padding(bottom = dims.space8),
                ) {
                    Column(modifier = Modifier.padding(dims.space16)) {
                        Text(item.projectTitle ?: "", style = MaterialTheme.typography.titleSmall)
                        Text(
                            stringResource(
                                R.string.stats_project_item,
                                item.netDeltaChars ?: 0,
                                formatDuration(item.activeSeconds ?: 0, context),
                            ),
                            style = MaterialTheme.typography.bodySmall,
                        )
                    }
                }
            }
        }

        if (summary == null && projectItems.isEmpty()) {
            item {
                Box(modifier = Modifier.fillMaxSize().padding(dims.space32), contentAlignment = Alignment.Center) {
                    Text(stringResource(id = R.string.stats_no_data), style = MaterialTheme.typography.bodyLarge)
                }
            }
        }
    }
}

@Composable
private fun StatItem(
    label: String,
    value: String,
) {
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        Text(value, style = MaterialTheme.typography.titleMedium)
        Text(label, style = MaterialTheme.typography.bodySmall)
    }
}

private fun formatDuration(
    seconds: Long,
    context: android.content.Context,
): String {
    if (seconds < 60) return context.getString(R.string.duration_seconds, seconds)
    val minutes = seconds / 60
    if (minutes < 60) return context.getString(R.string.duration_minutes, minutes)
    val hours = minutes / 60
    val remainMinutes = minutes % 60
    return if (remainMinutes > 0) {
        context.getString(
            R.string.duration_hours_minutes,
            hours,
            remainMinutes,
        )
    } else {
        context.getString(R.string.duration_hours, hours)
    }
}
