package com.xiwei.sujian.data

import com.xiwei.sujian.model.ProjectWritingStatsSummary
import com.xiwei.sujian.model.WritingStatsSummary

/**
 * StatsRepository — 统计仓库层
 *
 * 对统计领域 Bridge 的封装，提供统一的统计读取接口。
 * UI 层通过此 Repository 访问统计数据，不直接引用 BridgeProvider 或 BridgeResult。
 */
class StatsRepository(
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
}
