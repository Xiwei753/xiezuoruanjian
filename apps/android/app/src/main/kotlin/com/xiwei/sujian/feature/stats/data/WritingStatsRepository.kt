package com.xiwei.sujian.feature.stats.data
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.stats.data.model.ProjectWritingStatsSummary
import com.xiwei.sujian.feature.stats.data.model.WritingStatsSummary
import com.xiwei.sujian.feature.stats.data.interop.StatsBridge

/**
 * WritingStatsRepository — 统计仓库层
 *
 * 对统计领域 Bridge 的封装，提供统一的统计读取接口。
 * UI 层通过此 Repository 访问统计数据，不直接引用 AppServiceProvider 或 BridgeResult。
 *
 * #602 Phase 5：从 ProjectRepository 移入 recordWritingEvent/processWritingEvent/flushWritingStats。
 */
class WritingStatsRepository(
    private val statsBridge: StatsBridge,
) {
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
        return statsBridge.recordWritingEvent(
            deviceId, projectId, volumeId, chapterId,
            source, insertedChars, deletedChars, pastedChars, aiInsertedChars, durationSeconds, sessionId,
        )
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
        return statsBridge.processWritingEvent(
            deviceId, platform, projectId, volumeId, chapterId, oldText, newText,
            durationSeconds, sessionId,
        )
    }

    fun flushWritingStats() {
        statsBridge.flushWritingStats()
    }
}
