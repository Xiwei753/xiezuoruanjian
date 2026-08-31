package com.xiwei.sujian.feature.stats.data.interop
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.core.interop.common.toModel
import com.xiwei.sujian.core.interop.project.toModel
import com.xiwei.sujian.feature.project.data.model.ProjectStats
import com.xiwei.sujian.feature.stats.data.model.ChapterWritingStatsSummary
import com.xiwei.sujian.feature.stats.data.model.DeviceWritingStatsSummary
import com.xiwei.sujian.feature.stats.data.model.ProjectWritingStatsSummary
import com.xiwei.sujian.feature.stats.data.model.WritingSpeedCurve
import com.xiwei.sujian.feature.stats.data.model.WritingStatsSummary

/**
 * 统计 领域 Bridge。
 *
 * 从 AppServiceBridge 拆出，负责写作统计相关操作。
 */
class StatsBridge internal constructor(private val holder: WriterAppServiceHolder) {
    fun getWritingStatsSummary(
        startDate: String,
        endDate: String,
    ): BridgeResult<WritingStatsSummary> =
        holder.wrapResult {
            holder.service.getWritingStatsSummary(startDate, endDate).toModel()
        }

    fun getWritingWritingStatsSummary(
        startDate: String,
        endDate: String,
    ): BridgeResult<WritingStatsSummary> = getWritingStatsSummary(startDate, endDate)

    fun getWritingStatsByProject(
        startDate: String,
        endDate: String,
    ): BridgeResult<ProjectWritingStatsSummary> =
        holder.wrapResult {
            holder.service.getWritingStatsByProject(startDate, endDate).toModel()
        }

    fun getWritingStatsByChapter(
        startDate: String,
        endDate: String,
    ): BridgeResult<ChapterWritingStatsSummary> =
        holder.wrapResult {
            holder.service.getWritingStatsByChapter(startDate, endDate).toModel()
        }

    fun getWritingStatsByDevice(
        startDate: String,
        endDate: String,
    ): BridgeResult<DeviceWritingStatsSummary> =
        holder.wrapResult {
            holder.service.getWritingStatsByDevice(startDate, endDate).toModel()
        }

    fun getWritingSpeedCurve(
        startDate: String,
        endDate: String,
        bucketMinutes: Int,
    ): BridgeResult<WritingSpeedCurve> =
        holder.wrapResult {
            holder.service.getWritingSpeedCurve(startDate, endDate, bucketMinutes.toUInt()).toModel()
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
    ): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.recordWritingEvent(
                deviceId, projectId, volumeId, chapterId, source, insertedChars, deletedChars,
                pastedChars, aiInsertedChars, durationSeconds, sessionId,
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
    ): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.processWritingEvent(
                deviceId, platform, projectId, volumeId, chapterId, oldText, newText,
                durationSeconds, sessionId,
            )
        }

    fun flushWritingStats(): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.flushWritingStats()
        }

    fun getProjectStats(projectId: String): BridgeResult<ProjectStats> =
        holder.wrapResult {
            holder.service.getProjectStats(projectId).toModel()
        }
}
