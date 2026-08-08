package com.xiwei.sujian.core.interop.project
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.core.interop.common.toModel
import com.xiwei.sujian.core.model.RecentEdit

/**
 * 最近编辑记录领域 Bridge。
 *
 * 从 ProjectBridge 拆出，专门负责最近编辑记录的查询、记录与刷盘。
 * Core 侧有 5 秒防抖（同一章节 5 秒内多次调用只记录一次），
 * Android 端无需额外防抖，直接委托即可。
 */
class RecentEditsBridge internal constructor(private val holder: WriterAppServiceHolder) {
    fun getRecentEdits(): BridgeResult<List<RecentEdit>> =
        holder.wrapResult {
            holder.service.getRecentEdits().map { it.toModel() }
        }

    fun recordRecentEdit(
        projectId: String,
        volumeId: String,
        chapterId: String,
    ): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.recordRecentEdit(projectId, volumeId, chapterId)
        }

    fun flushRecentEdits(): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.flushRecentEdits()
        }
}
