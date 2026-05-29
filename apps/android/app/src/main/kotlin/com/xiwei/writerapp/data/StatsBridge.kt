package com.xiwei.writerapp.data

import com.google.gson.Gson
import com.xiwei.writerapp.model.ProjectWritingStatsSummary
import com.xiwei.writerapp.model.ProjectStats
import com.xiwei.writerapp.model.WritingStatsSummary

class StatsBridge(private val appService: AppServiceBridge) {
    private val gson = Gson()

    fun getWritingStatsSummary(startDate: String, endDate: String): BridgeResult<WritingStatsSummary> {
        val res = appService.getWritingStatsSummary(startDate, endDate)
        if (res is BridgeResult.Success) {
            val s = gson.fromJson(res.data, WritingStatsSummary::class.java)
            return BridgeResult.Success(s)
        }
        return res as BridgeResult.Error
    }

    fun getWritingWritingStatsSummary(startDate: String, endDate: String): BridgeResult<WritingStatsSummary> {
        return getWritingStatsSummary(startDate, endDate)
    }

    fun getWritingStatsByProject(startDate: String, endDate: String): BridgeResult<ProjectWritingStatsSummary> {
        val res = appService.getWritingStatsByProject(startDate, endDate)
        if (res is BridgeResult.Success) {
            val s = gson.fromJson(res.data, ProjectWritingStatsSummary::class.java)
            return BridgeResult.Success(s)
        }
        return res as BridgeResult.Error
    }

    fun getWritingSpeedCurve(startDate: String, endDate: String, bucketMinutes: Int): BridgeResult<String> {
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
