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
 *
 * #618 六 复审：缓存键 = (revision, 查询结束日期)。ViewModel 可以长期留在 NavEntry 里，
 * 跨过零点后即使 revision 没变，查询区间（最近 30 天）已经变了，也必须刷新。
 * 查询开始时就固定快照 [queriedRevision]/[queriedEndDate]，提交结果只推进到快照值；
 * 提交后若 revision 已越过快照（查询期间又发生了写入），立即再跑一轮，直到拿到与
 * 当前 revision 对齐的数据，绝不把可能漏掉最新事件的数据冒充最新。
 */
class StatsViewModel(
    private val repository: WritingStatsRepository,
) : ViewModel() {
    private val _uiState = MutableStateFlow(StatsUiState())
    val uiState: StateFlow<StatsUiState> = _uiState.asStateFlow()

    /** 已加载数据对应的 revision（查询开始时的快照）。internal 供单测校验契约。 */
    internal var loadedRevision = -1L

    /** 已加载数据对应的查询结束日期（本地日期，缓存键的一部分）。 */
    internal var loadedEndDate: LocalDate? = null

    /** 查询在途时直接复用（底栏快速切换不叠加并发重复查询）。internal 供单测校验契约。 */
    internal var queryInFlight = false

    /** 时钟接缝：单测可拨动日期验证跨零点刷新；生产恒为 LocalDate.now()。 */
    internal var todayProvider: () -> LocalDate = { LocalDate.now() }

    /**
     * 只在数据确实变化时重新查询：revision 与查询日期都与已加载值相同且非加载中则直接复用。
     *
     * 成功与失败时都只把已加载状态推进到查询开始时的 [queriedRevision]/[queriedEndDate]
     * 快照：结果（哪怕是失败的空结果）只属于这个窗口。查询期间新写入的事件留待提交后的
     * revision 校验触发立即再跑一轮，不把“可能未包含最新事件”的数据冒充最新。查询失败
     * （BridgeResult 非 Success / 未加载原生库 / 未预期异常）如实显示空数据，不伪装成功；
     * 失败时也推进到查询快照而非查询结束时的当前 revision — 否则查询期间的写入会让递归
     * refreshIfNeeded() 立即命中缓存跳过，漏掉重跑；推进到快照则 revision 越过快照时
     * 递归不跳过，正确重跑，无写入时不递归，避免死循环。
     */
    fun refreshIfNeeded() {
        if (queryInFlight) return
        val today = todayProvider()
        if (loadedRevision == repository.revision.value && loadedEndDate == today && !_uiState.value.loading) {
            return
        }

        queryInFlight = true
        viewModelScope.launch {
            // 查询开始时就固定保存快照：结果只属于这个窗口。
            val queriedRevision = repository.revision.value
            val queriedEndDate = todayProvider()
            try {
                val result =
                    withContext(Dispatchers.IO) {
                        val start = queriedEndDate.minusDays(30).format(DateTimeFormatter.ISO_LOCAL_DATE)
                        val end = queriedEndDate.format(DateTimeFormatter.ISO_LOCAL_DATE)
                        repository.getWritingStatsSummary(start, end) to
                            repository.getWritingStatsByProject(start, end)?.projects.orEmpty()
                    }
                loadedRevision = queriedRevision
                loadedEndDate = queriedEndDate
                _uiState.value = StatsUiState(result.first, result.second, false)
            } catch (e: Exception) {
                // 失败也只推进到查询快照：查询期间若有写入，下方 revision 校验会触发
                // 重跑（loadedRevision=queriedRevision != 当前值，递归不命中缓存）；
                // 无写入则不递归，下次切回命中缓存，避免死循环。
                loadedRevision = queriedRevision
                loadedEndDate = queriedEndDate
                _uiState.value = StatsUiState(null, emptyList(), false)
            } finally {
                queryInFlight = false
            }
            // 提交结果后校验：查询期间又发生写入（revision 越过查询快照），
            // 本次数据可能不是最新，立即再跑一轮；否则结束。
            if (repository.revision.value != queriedRevision) {
                refreshIfNeeded()
            }
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
