package com.xiwei.sujian.data

import com.xiwei.sujian.model.ChapterWritingStatsSummary
import com.xiwei.sujian.model.DeviceWritingStatsSummary
import com.xiwei.sujian.model.ProjectWritingStatsSummary
import com.xiwei.sujian.model.ProjectStats
import com.xiwei.sujian.model.WritingSpeedCurve
import com.xiwei.sujian.model.WritingStatsSummary

class StatsBridge(private val appService: AppServiceBridge) {

    fun getWritingStatsSummary(startDate: String, endDate: String): BridgeResult<WritingStatsSummary> {
        return appService.getWritingStatsSummary(startDate, endDate)
    }

    fun getWritingWritingStatsSummary(startDate: String, endDate: String): BridgeResult<WritingStatsSummary> {
        return getWritingStatsSummary(startDate, endDate)
    }

    fun getWritingStatsByProject(startDate: String, endDate: String): BridgeResult<ProjectWritingStatsSummary> {
        return appService.getWritingStatsByProject(startDate, endDate)
    }

    fun getWritingStatsByChapter(startDate: String, endDate: String): BridgeResult<ChapterWritingStatsSummary> {
        return appService.getWritingStatsByChapter(startDate, endDate)
    }

    fun getWritingStatsByDevice(startDate: String, endDate: String): BridgeResult<DeviceWritingStatsSummary> {
        return appService.getWritingStatsByDevice(startDate, endDate)
    }

    fun getWritingSpeedCurve(startDate: String, endDate: String, bucketMinutes: Int): BridgeResult<WritingSpeedCurve> {
        return appService.getWritingSpeedCurve(startDate, endDate, bucketMinutes)
    }

    fun recordWritingEvent(deviceId: String, projectId: String, volumeId: String, chapterId: String, source: String, insertedChars: Int, deletedChars: Int, pastedChars: Int, aiInsertedChars: Int, sessionId: String): BridgeResult<Boolean> {
        return appService.recordWritingEvent(deviceId, projectId, volumeId, chapterId, source, insertedChars, deletedChars, pastedChars, aiInsertedChars, sessionId)
    }

    fun processWritingEvent(deviceId: String, platform: String, projectId: String, volumeId: String, chapterId: String, oldText: String, newText: String, sessionId: String): BridgeResult<Boolean> {
        return appService.processWritingEvent(deviceId, platform, projectId, volumeId, chapterId, oldText, newText, sessionId)
    }

    fun flushWritingStats(): BridgeResult<Boolean> = appService.flushWritingStats()

    fun getProjectStats(projectId: String): BridgeResult<ProjectStats> = appService.getProjectStats(projectId)
}
