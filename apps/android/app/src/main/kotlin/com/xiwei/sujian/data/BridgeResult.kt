package com.xiwei.sujian.data


/**
 * Android 端 Bridge 调用结果密封类。
 *
 * - [Success]：Core 返回成功数据，envelope 携带完整信封
 * - [Error]：Core 或 Bridge 层返回错误，errorCode 与 Core Error.code() 对齐
 * - [NotLoaded]：原生库未加载（UnsatisfiedLinkError），UI 应提示用户
 *
 * 所有 Bridge 方法统一返回此类型，ViewModel 据此决定展示逻辑。
 */
sealed class BridgeResult<out T> {
    data class Success<out T>(val data: T, val envelope: ResultEnvelope<T> = ResultEnvelope.success(data)) : BridgeResult<T>()
    data class Error(
        val envelope: ResultEnvelope<Nothing>
    ) : BridgeResult<Nothing>() {
        val message: String get() = envelope.messageKey ?: envelope.errorCode ?: ""
        val code: String? get() = envelope.errorCode
    }
    object NotLoaded : BridgeResult<Nothing>() {
        val envelope: ResultEnvelope<Nothing> = ResultEnvelope.errorOf("NATIVE_NOT_LOADED", "Native library not loaded")
    }
}

data class ChangedEntity(
    val entityType: String,
    val entityId: String? = null
)

/**
 * 跨端 Bridge 信封 — 与 Core BridgeError 对齐的统一响应结构。
 *
 * - errorCode：与 Core Error.code() 返回的字符串一致，是跨端 API 契约
 * - messageKey：errorCode 到 i18n key 的映射，UI 层据此做本地化
 * - messageArgs：错误参数（与 Core Error.params() 对齐），供本地化模板填充
 * - changedPaths / changedEntities：本次操作影响的实体，供 UI 刷新
 */
data class ResultEnvelope<out T>(
    val success: Boolean,
    val data: T? = null,
    val errorCode: String? = null,
    val messageKey: String? = null,
    val messageArgs: Map<String, String> = emptyMap(),
    @Deprecated("Core 不再提供 user_message，使用 messageKey 或 rawError") val userMessage: String? = null,
    val rawError: String? = null,
    val warnings: List<String> = emptyList(),
    val changedPaths: List<String> = emptyList(),
    val changedEntities: List<ChangedEntity> = emptyList()
) {
    companion object {
        fun <T> success(data: T): ResultEnvelope<T> = ResultEnvelope(
            success = true,
            data = data
        )

        @Deprecated("使用 messageKey/rawError")
        fun error(errorCode: String, @Suppress("DEPRECATION") userMessage: String): ResultEnvelope<Nothing> = ResultEnvelope(
            success = false,
            errorCode = errorCode,
            messageKey = errorCodeToMessageKey(errorCode),
            userMessage = null,
            rawError = userMessage
        )

        fun errorOf(errorCode: String, rawError: String): ResultEnvelope<Nothing> = ResultEnvelope(
            success = false,
            errorCode = errorCode,
            messageKey = errorCodeToMessageKey(errorCode),
            userMessage = null,
            rawError = rawError
        )

        private fun errorCodeToMessageKey(errorCode: String): String = when (errorCode) {
            "IO_ERROR" -> "error.io"
            "JSON_ERROR" -> "error.json"
            "INVALID_WORKSPACE" -> "error.invalid_workspace"
            "PROJECT_NOT_FOUND" -> "error.project_not_found"
            "VOLUME_NOT_FOUND" -> "error.volume_not_found"
            "CHAPTER_NOT_FOUND" -> "error.chapter_not_found"
            "EMPTY_OVERWRITE_BLOCKED" -> "error.empty_overwrite_blocked"
            "NOT_IMPLEMENTED" -> "error.not_implemented"
            "REFUSE_DELETE_WORKSPACE_ROOT" -> "error.refuse_delete_workspace_root"
            "INVALID_DELETE_TARGET" -> "error.invalid_delete_target"
            "SYNC_CONFLICT" -> "error.sync_conflict"
            "SYNC_FAILED" -> "error.sync_failed"
            "NATIVE_ERROR" -> "error.native_error"
            "PARSE_ERROR" -> "error.parse_error"
            "NOT_IMPLEMENTED_BRIDGE" -> "error.not_implemented_bridge"
            "FLUSH_ERROR" -> "error.flush"
            "CLOSE_ERROR" -> "error.close"
            "FLUSH_ALL_ERROR" -> "error.flush_all"
            "CONVERSION_ERROR" -> "error.conversion"
            "SNAPSHOT_CACHE_NOT_INITIALIZED" -> "error.snapshot_cache_not_initialized"
            "STAR_MAP_CACHE_MISSING" -> "error.star_map_cache_missing"
            else -> "error.other"
        }
    }
}
