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
import uniffi.writer_core.EditorChangeDto
import uniffi.writer_core.EditorChangeStatsInputDto
import uniffi.writer_core.EditorTransactionCauseDto
import uniffi.writer_core.WritingActivitySegmentInputDto

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

    fun recordEditorChangeStats(
        projectId: String,
        volumeId: String,
        chapterId: String,
        cause: EditorTransactionCauseDto,
        insertedChars: Int,
        deletedChars: Int,
        eventId: String,
        occurredAtMs: Long,
        changes: List<EditorChangeDto> = emptyList(),
        sessionId: String = "",
        baselineText: String? = null,
        baseRevision: Long = 0,
        newRevision: Long = 0,
        oldSelectionAnchor: Int = 0,
        oldSelectionHead: Int = 0,
        newSelectionAnchor: Int = 0,
        newSelectionHead: Int = 0,
    ): BridgeResult<Unit> =
        holder.wrapResult {
            holder.service.recordEditorChangeStats(
                EditorChangeStatsInputDto(
                    platform = "android",
                    projectId = projectId,
                    volumeId = volumeId,
                    chapterId = chapterId,
                    cause = cause,
                    insertedChars = insertedChars.toUInt(),
                    deletedChars = deletedChars.toUInt(),
                    eventId = eventId,
                    occurredAtMs = occurredAtMs,
                    changes = changes,
                    sessionId = sessionId,
                    baselineText = baselineText,
                    baseRevision = baseRevision.toULong(),
                    newRevision = newRevision.toULong(),
                    oldSelectionAnchor = oldSelectionAnchor.toUInt(),
                    oldSelectionHead = oldSelectionHead.toUInt(),
                    newSelectionAnchor = newSelectionAnchor.toUInt(),
                    newSelectionHead = newSelectionHead.toUInt(),
                    syncSourceDeviceId = "",
                    parentEventId = "",
                ),
            )
        }

    fun getProjectStats(projectId: String): BridgeResult<ProjectStats> =
        holder.wrapResult {
            holder.service.getProjectStats(projectId).toModel()
        }

    /** Issue #1008：按章节回放手写证明，返回 Core 已校验的帧 JSON。 */
    fun replayChapterProof(requestJson: String): BridgeResult<String> =
        holder.wrapResult {
            holder.service.replayChapterProofJson(requestJson)
        }

    /** Issue #1008：导出手写证明包；包含历史正文，调用方需先提示用户。 */
    fun exportChapterProof(
        chapterId: String,
        deviceId: String,
    ): BridgeResult<String> =
        holder.wrapResult {
            holder.service.exportChapterProofJson(chapterId, deviceId)
        }

    /** Issue #981：上报一段已测量的前台写作计时，Core 负责幂等落盘与字/时聚合。 */
    fun recordWritingActivitySegment(input: WritingActivitySegmentInputDto): BridgeResult<Unit> =
        holder.wrapResult {
            holder.service.recordWritingActivitySegment(input)
        }
}
