package com.xiwei.sujian.feature.search.data

import android.content.Context
import com.google.gson.Gson
import com.xiwei.sujian.R
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.core.interop.common.MessageKeyMapper
import com.xiwei.sujian.core.interop.common.RepositoryException
import com.xiwei.sujian.core.interop.search.SearchBridge
import com.xiwei.sujian.feature.search.data.model.SearchContext
import com.xiwei.sujian.feature.search.data.model.SearchPage
import com.xiwei.sujian.feature.sync.data.SyncFailureKind

/**
 * 搜索仓库 — 只负责 Core 查询与 JSON 反序列化，不持有 UI 状态。
 *
 * 调用方必须在 IO 线程执行；本类不自行切线程，便于 Controller 把
 * 搜索请求与编辑输入完全隔离。
 */
class SearchRepository(
    private val context: Context,
    private val searchBridge: SearchBridge,
) {
    private val gson = Gson()

    fun searchPage(
        query: String,
        scope: String,
        limit: Int,
        cursor: String?,
        searchContext: SearchContext,
    ): SearchPage {
        val pageJson =
            when (val result = searchBridge.globalSearchPage(query, scope, limit, cursor, searchContext.toJson())) {
                is BridgeResult.Success -> result.data
                is BridgeResult.Error -> throw RepositoryException(
                    context.getString(
                        R.string.search_failed,
                        MessageKeyMapper.resolveMessage(
                            context,
                            result.envelope.messageKey,
                            result.envelope.messageArgs,
                            result.envelope.errorCode,
                        ),
                    ),
                )
                BridgeResult.NotLoaded -> throw RepositoryException(
                    context.getString(R.string.search_native_not_loaded),
                    SyncFailureKind.NativeUnavailable,
                )
            }

        return try {
            gson.fromJson(pageJson, SearchPage::class.java) ?: SearchPage()
        } catch (e: Exception) {
            throw RepositoryException(
                context.getString(R.string.search_failed, e.message ?: ""),
            )
        }
    }
}
