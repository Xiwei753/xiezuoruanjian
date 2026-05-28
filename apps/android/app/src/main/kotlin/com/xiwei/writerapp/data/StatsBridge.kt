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

class StatsBridge(private val nativeBridge: NativeCoreBridge) {
    fun getProjectStats(projectId: String): BridgeResult<ProjectStats> = nativeBridge.getProjectStats(projectId).toBridgeResult()
    fun flushWritingStats() = nativeBridge.flushWritingStats()

    fun getWritingStatsSummary(startDate: String, endDate: String): BridgeResult<StatsSummary> {
        val json = nativeBridge.getWritingStatsSummary(startDate, endDate) ?: return BridgeResult.Error(com.xiwei.writerapp.model.BridgeError(message = "Not loaded or empty"))
        return try {
            val summary = com.google.gson.Gson().fromJson(json, StatsSummary::class.java)
            BridgeResult.Success(summary)
        } catch (e: Exception) {
            BridgeResult.Error(com.xiwei.writerapp.model.BridgeError(message = e.message ?: "Parsing error"))
        }
    }

    fun getWritingStatsByProject(startDate: String, endDate: String): BridgeResult<ProjectStatsSummary> {
        val res = nativeBridge.getWritingStatsByProject(startDate, endDate)
        return when (res) {
            is NativeResult.Success -> {
                val jsonStr = com.google.gson.Gson().toJson(res.data)
                try {
                    val summary = com.google.gson.Gson().fromJson(jsonStr, ProjectStatsSummary::class.java)
                    BridgeResult.Success(summary)
                } catch (e: Exception) {
                    BridgeResult.Error(com.xiwei.writerapp.model.BridgeError(message = e.message ?: "Parsing error"))
                }
            }
            is NativeResult.Error -> BridgeResult.Error(com.xiwei.writerapp.model.BridgeError(message = res.message))
            NativeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }
}
