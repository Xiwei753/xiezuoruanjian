package com.xiwei.sujian.storage.recovery

import com.xiwei.sujian.app.WorkspaceAppState
import com.xiwei.sujian.feature.starmap.data.StarMapRepository
import com.xiwei.sujian.feature.stats.data.WritingStatsRepository
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * RecoveryChangeSink — 恢复完成后通知所有相关组件数据已变更。
 *
 * 聚合 project 列表、recent edits、starmap cache、stats 的刷新入口，
 * 避免恢复器各自直接依赖这些组件。位于 `:app` 的 `storage/recovery` 包，
 * 可依赖 :app 的 DI 与 Repository，但不放 Composable（UI 接入由上层负责）。
 *
 * #649 评论 5560685734 要求 5：本类只负责"恢复后刷新 UI/缓存"，与评论 5559759935
 * 中的镜像变更入口（MirrorChangeSink）撞名，故改名 `RecoveryChangeSink` 以区分。
 */
interface RecoveryChangeSink {
    /** 通知 project 列表、recent edits、starmap cache、stats 全部刷新。 */
    suspend fun everythingChanged()
}

/**
 * 默认实现：注入 [WorkspaceAppState] 与各 Repository，在 IO 调度执行刷新。
 *
 * - [WorkspaceAppState.refreshProjectSummaries] / [WorkspaceAppState.refreshRecentEdits]
 *   内部各自 `viewModelScope.launch`，调用本身不阻塞；
 * - [StarMapRepository.invalidateCache] / [WritingStatsRepository.invalidate] 是同步轻量操作。
 *
 * #649 评论 5559763924：接收 [WorkspaceAppState] 接口而非具体 [com.xiwei.sujian.app.SujianAppState]，
 * 让作品页（持有接口）与设置页（通过 CompositionLocal 拿到具体类）都能构造 Sink。
 */
class DefaultRecoveryChangeSink(
    private val appState: WorkspaceAppState,
    private val starMapRepository: StarMapRepository,
    private val writingStatsRepository: WritingStatsRepository,
) : RecoveryChangeSink {
    override suspend fun everythingChanged() {
        withContext(Dispatchers.IO) {
            appState.refreshProjectSummaries()
            appState.refreshRecentEdits()
            starMapRepository.invalidateCache()
            writingStatsRepository.invalidate()
        }
    }
}
