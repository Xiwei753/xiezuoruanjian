package com.xiwei.writerapp.data

import com.xiwei.writerapp.model.ProjectStats

data class StatsSummary(
    @com.google.gson.annotations.SerializedName("total_human_typed_chars")
    val totalHumanTypedChars: Int? = 0,
    @com.google.gson.annotations.SerializedName("total_active_seconds")
    val totalActiveSeconds: Int? = 0
)

data class ProjectStatsDetail(
    @com.google.gson.annotations.SerializedName("project_title")
    val projectTitle: String?,
    @com.google.gson.annotations.SerializedName("human_typed_chars")
    val humanTypedChars: Int? = 0
)

data class ProjectStatsSummary(
    val projects: List<ProjectStatsDetail>? = emptyList()
)

class StatsBridge internal constructor(private val nativeBridge: NativeCoreBridge) {
    fun getProjectStats(projectId: String): BridgeResult<ProjectStats> = nativeBridge.getProjectStats(projectId).toBridgeResult()
    fun flushWritingStats() = nativeBridge.flushWritingStats()

    fun getWritingStatsSummary(startDate: String, endDate: String): BridgeResult<StatsSummary> =
        nativeBridge.getWritingStatsSummary(startDate, endDate).toBridgeResult()

    fun getWritingStatsByProject(startDate: String, endDate: String): BridgeResult<ProjectStatsSummary> =
        nativeBridge.getWritingStatsByProject(startDate, endDate).toBridgeResult()
}
