package com.xiwei.sujian.feature.stats.data
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.stats.data.interop.StatsBridge
import com.xiwei.sujian.feature.stats.data.model.ProjectWritingStatsSummary
import com.xiwei.sujian.feature.stats.data.model.WritingStatsSummary
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update

/**
 * WritingStatsRepository — 统计仓库层
 *
 * 对统计领域 Bridge 的封装，提供统一的统计读取接口。
 * UI 层通过此 Repository 访问统计数据，不直接引用 AppServiceProvider 或 BridgeResult。
 *
 * #602 Phase 5：从 ProjectRepository 移入 recordWritingEvent/processWritingEvent/flushWritingStats。
 *
 * #618 六：revision 只读计数 — 每次统计事件成功写入递增一次。统计页 ViewModel 用它判断
 * 是否真的需要重新查询（revision 未变则复用已加载数据，不再每次切回都重跑两遍 Core 查询）。
 * 统计数据只由本地写入事件产生（app-meta/stats 路径在 Core 同步中全量黑名单，同步不会
 * 替换统计文件），因此同步完成不需要额外 invalidate；若未来出现外部替换路径，
 * 在应用新数据的位置调用 [invalidate] 即可，它内部同样只递增 revision。
 */
class WritingStatsRepository(
    private val statsBridge: StatsBridge,
) {
    private val _revision = MutableStateFlow(0L)

    /** 统计数据变更计数：事件成功写入即递增，供 UI 判断是否需要重新读取。 */
    val revision: StateFlow<Long> = _revision.asStateFlow()

    private fun markChanged() {
        _revision.update { it + 1L }
    }

    /** 外部路径（如同步应用新数据）替换统计数据后调用 — 仅递增 revision。 */
    fun invalidate() {
        markChanged()
    }

    fun getWritingStatsSummary(
        startDate: String,
        endDate: String,
    ): WritingStatsSummary? {
        return when (val result = statsBridge.getWritingStatsSummary(startDate, endDate)) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    fun getWritingStatsByProject(
        startDate: String,
        endDate: String,
    ): ProjectWritingStatsSummary? {
        return when (val result = statsBridge.getWritingStatsByProject(startDate, endDate)) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    fun recordWritingEvent(
        deviceId: String,
        projectId: String,
        volumeId: String,
        chapterId: String,
        source: String,
        insertedChars: Int,
        deletedChars: Int,
        pastedChars: Int,
        aiInsertedChars: Int,
        durationSeconds: Int,
        sessionId: String,
    ): BridgeResult<Boolean> {
        val result =
            statsBridge.recordWritingEvent(
                deviceId, projectId, volumeId, chapterId,
                source, insertedChars, deletedChars, pastedChars, aiInsertedChars, durationSeconds, sessionId,
            )
        if (result is BridgeResult.Success) {
            markChanged()
        }
        return result
    }

    fun processWritingEvent(
        deviceId: String,
        platform: String,
        projectId: String,
        volumeId: String,
        chapterId: String,
        oldText: String,
        newText: String,
        durationSeconds: UInt,
        sessionId: String,
    ): BridgeResult<Boolean> {
        val result =
            statsBridge.processWritingEvent(
                deviceId, platform, projectId, volumeId, chapterId, oldText, newText,
                durationSeconds, sessionId,
            )
        if (result is BridgeResult.Success) {
            markChanged()
        }
        return result
    }

    fun flushWritingStats() {
        statsBridge.flushWritingStats()
    }
}
