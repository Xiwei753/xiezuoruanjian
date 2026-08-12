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
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import com.xiwei.sujian.R
import com.xiwei.sujian.app.di.LocalSujianAppDependencies
import com.xiwei.sujian.core.designsystem.component.SujianCard
import com.xiwei.sujian.core.designsystem.theme.LocalSujianDimensions

/**
 * #618 六：统计页 UI。
 *
 * 数据状态放进 NavEntry 级 [StatsViewModel]（复用 Navigation 3 多 back stack 的
 * ViewModelStore），页面重新进入时只做一次 revision 判断，不再每次切回都重新跑
 * 两遍 Core 查询。查询与状态读取全部走容器里的唯一 [WritingStatsRepository]，
 * 不再绕回 AppServiceProvider 取第二个 Bridge 入口。
 */
@Composable
fun StatsScreen(modifier: Modifier = Modifier) {
    val deps = LocalSujianAppDependencies.current
    val vm: StatsViewModel =
        viewModel(factory = StatsViewModel.Factory(deps.statsRepository))
    val uiState by vm.uiState.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val dims = LocalSujianDimensions.current

    LaunchedEffect(Unit) {
        vm.refreshIfNeeded()
    }

    if (uiState.loading) {
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

        uiState.summary?.let { s ->
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

        if (uiState.projects.isNotEmpty()) {
            item {
                Text(stringResource(id = R.string.stats_by_project), style = MaterialTheme.typography.titleSmall)
                Spacer(modifier = Modifier.height(dims.space8))
            }
            items(uiState.projects, key = { it.projectId ?: "" }) { item ->
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

        if (uiState.summary == null && uiState.projects.isEmpty()) {
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
