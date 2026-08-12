package com.xiwei.sujian.feature.stats.ui

import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import com.xiwei.sujian.feature.stats.data.WritingStatsRepository
import com.xiwei.sujian.feature.stats.data.model.ProjectWritingStatsItem
import com.xiwei.sujian.feature.stats.data.model.WritingStatsSummary
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.time.LocalDate
import java.time.format.DateTimeFormatter

/**
 * 统计页 UI 状态 — 只读数据快照。
 */
data class StatsUiState(
    val summary: WritingStatsSummary? = null,
    val projects: List<ProjectWritingStatsItem> = emptyList(),
    val loading: Boolean = true,
)

/**
 * #618 六：统计页 ViewModel。
 *
 * Navigation 3 多 back stack 会保存 NavEntry 的 ViewModelStore，但 inactive tab 的
 * 页面内容会退出 Composition；没有 ViewModel 时普通 remember 全重建，每次从
 * “统计 → 作品 → 统计”回来都重新跑两遍 Core 查询。现在数据状态放进 NavEntry 的
 * ViewModelStore：页面重新进入只做一次很轻的 revision 判断（[WritingStatsRepository.revision]
 * 只在统计事件成功写入时递增），统计真的变了才重新读取。
 */
class StatsViewModel(
    private val repository: WritingStatsRepository,
) : ViewModel() {
    private val _uiState = MutableStateFlow(StatsUiState())
    val uiState: StateFlow<StatsUiState> = _uiState.asStateFlow()

    private var loadedRevision = -1L

    /**
     * 只在数据确实变化时重新查询：revision 与已加载 revision 相同且非加载中则直接复用。
     * 查询失败（BridgeResult 非 Success / 未加载原生库）如实显示空数据，不伪装成功；
     * 仍推进 loadedRevision，避免切回一次就重查一次的死循环。
     */
    fun refreshIfNeeded() {
        val revision = repository.revision.value
        if (loadedRevision == revision && !_uiState.value.loading) return

        viewModelScope.launch {
            val result =
                try {
                    withContext(Dispatchers.IO) {
                        val today = LocalDate.now()
                        val start = today.minusDays(30).format(DateTimeFormatter.ISO_LOCAL_DATE)
                        val end = today.format(DateTimeFormatter.ISO_LOCAL_DATE)
                        repository.getWritingStatsSummary(start, end) to
                            repository.getWritingStatsByProject(start, end)?.projects.orEmpty()
                    }
                } catch (e: Exception) {
                    null to emptyList<ProjectWritingStatsItem>()
                }
            loadedRevision = repository.revision.value
            _uiState.value = StatsUiState(result.first, result.second, false)
        }
    }

    class Factory(
        private val repository: WritingStatsRepository,
    ) : ViewModelProvider.Factory {
        override fun <T : ViewModel> create(modelClass: Class<T>): T {
            return modelClass.cast(StatsViewModel(repository)) as T
        }
    }
}
