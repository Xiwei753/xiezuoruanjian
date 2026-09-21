package com.xiwei.sujian.feature.project.data
import android.content.Context
import com.xiwei.sujian.R
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.core.interop.common.MessageKeyMapper
import com.xiwei.sujian.core.interop.diagnostics.DiagnosticsInterop
import com.xiwei.sujian.feature.project.data.model.RecentEdit

/**
 * RecentEditsRepository — 最近编辑记录仓库层。
 *
 * 从 [com.xiwei.sujian.feature.project.data.ProjectRepository] 拆出，
 * 专门负责最近编辑记录的查询、记录与刷盘。
 */
class RecentEditsRepository(private val context: Context, private val appBridge: AppServiceBridge) {
    private val recentEditsBridge = appBridge.recentEditsBridge

    private fun BridgeResult.Error.localizedMessage(): String {
        return MessageKeyMapper.resolveMessage(context, envelope.messageKey, envelope.messageArgs, envelope.errorCode)
    }

    fun getRecentEdits(): List<RecentEdit> {
        return when (val result = recentEditsBridge.getRecentEdits()) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> {
                DiagnosticsInterop.w(
                    "RecentEditsRepository",
                    context.getString(R.string.repo_get_recent_edits_failed, result.localizedMessage()),
                )
                emptyList()
            }
            BridgeResult.NotLoaded -> emptyList()
        }
    }

    /**
     * #732 评论第5节：首页契约 singular — Android 产品层唯一一次"历史列表 → 最近一次"的转换。
     *
     * 读取 Core 已排序结果的 [firstOrNull]。Core 的 recent_edits.json 仍保留历史/去重能力，
     * [getRecentEdits] 通用 API 不删（历史记录查询）；首页自己的契约必须是单值。
     */
    fun getLatestRecentEdit(): RecentEdit? = getRecentEdits().firstOrNull()

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
