package com.xiwei.writerapp.data

import com.xiwei.writerapp.model.ProjectStats

class StatsBridge(private val nativeBridge: NativeCoreBridge) {
    fun getProjectStats(projectId: String): BridgeResult<ProjectStats> = nativeBridge.getProjectStats(projectId).toBridgeResult()
    fun flushWritingStats() = nativeBridge.flushWritingStats()
}
