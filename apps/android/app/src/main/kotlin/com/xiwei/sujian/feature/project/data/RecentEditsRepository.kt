package com.xiwei.sujian.feature.project.data
import android.content.Context
import com.xiwei.sujian.R
import com.xiwei.sujian.app.di.AppServiceProvider
import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.core.interop.common.MessageKeyMapper
import com.xiwei.sujian.feature.project.data.model.RecentEdit

/**
 * RecentEditsRepository — 最近编辑记录仓库层。
 *
 * 从 [com.xiwei.sujian.feature.project.data.ProjectRepository] 拆出，
 * 专门负责最近编辑记录的查询、记录与刷盘。
 */
class RecentEditsRepository(private val context: Context, bridge: AppServiceBridge? = null) {
    private val appBridge = bridge ?: AppServiceProvider.getAppServiceBridge(context)
    private val recentEditsBridge = appBridge.recentEditsBridge

    private fun BridgeResult.Error.localizedMessage(): String {
        return MessageKeyMapper.resolveMessage(context, envelope.messageKey, envelope.messageArgs, envelope.errorCode)
    }

    fun getRecentEdits(): List<RecentEdit> {
        return when (val result = recentEditsBridge.getRecentEdits()) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> {
                DiagnosticsLogger.w(
                    "RecentEditsRepository",
                    context.getString(R.string.repo_get_recent_edits_failed, result.localizedMessage()),
                )
                emptyList()
            }
            BridgeResult.NotLoaded -> emptyList()
        }
    }

    fun recordRecentEdit(
        projectId: String,
        volumeId: String,
        chapterId: String,
    ) {
        recentEditsBridge.recordRecentEdit(projectId, volumeId, chapterId)
    }

    fun flushRecentEdits() {
        recentEditsBridge.flushRecentEdits()
    }
}
