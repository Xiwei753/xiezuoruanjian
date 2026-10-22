package com.xiwei.sujian.core.interop.search

import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.core.interop.common.BridgeResult

/**
 * 全局搜索领域 Bridge — Kotlin 到 Core UniFFI 的唯一搜索入口。
 *
 * Core 返回 ResultEnvelope 包裹的 JSON 字符串；JSON 形状由
 * `SearchResultPage`/`SearchResult` 的 camelCase 契约定义。
 */
class SearchBridge internal constructor(private val holder: WriterAppServiceHolder) {
    fun globalSearchPage(
        query: String,
        scope: String,
        limit: Int,
        cursor: String?,
        contextJson: String?,
    ): BridgeResult<String> =
        holder.wrapResult {
            holder.service.globalSearchPage(
                query,
                scope,
                limit.toUInt(),
                cursor,
                contextJson,
            )
        }
}
