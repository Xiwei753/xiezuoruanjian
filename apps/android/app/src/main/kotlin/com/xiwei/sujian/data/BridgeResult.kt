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
    data class Success<out T>(
        val data: T,
        val envelope: ResultEnvelope<T> = ResultEnvelope.success(data),
    ) : BridgeResult<T>()

    data class Error(
        val envelope: ResultEnvelope<Nothing>,
        /**
         * #592 七：Core/Bridge 边界的类型化同步失败。由 WriterException 变体直接推导，
         * 不再维护 Android 字符串错误码表；null 表示非同步错误或未知类型，默认 Fatal。
         */
        val syncFailureKind: SyncFailureKind? = null,
    ) : BridgeResult<Nothing>() {
        val message: String get() = envelope.messageKey ?: envelope.errorCode ?: ""
        val code: String? get() = envelope.errorCode
        val fullEnvelope: String
            get() = "[${envelope.errorCode ?: "UNKNOWN"}] ${envelope.messageKey ?: ""} | ${envelope.rawError ?: ""}"
    }

    object NotLoaded : BridgeResult<Nothing>() {
        val envelope: ResultEnvelope<Nothing> = ResultEnvelope.errorOf("NATIVE_NOT_LOADED", "Native library not loaded")
    }
}

data class ChangedEntity(
    val entityType: String,
    val entityId: String? = null,
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
    val rawError: String? = null,
    val warnings: List<String> = emptyList(),
    val changedPaths: List<String> = emptyList(),
    val changedEntities: List<ChangedEntity> = emptyList(),
) {
    companion object {
        fun <T> success(data: T): ResultEnvelope<T> =
            ResultEnvelope(
                success = true,
                data = data,
            )

        fun errorOf(
            errorCode: String,
            rawError: String,
        ): ResultEnvelope<Nothing> =
            ResultEnvelope(
                success = false,
                errorCode = errorCode,
                messageKey = errorCodeToMessageKey(errorCode),
                rawError = rawError,
            )

        private fun errorCodeToMessageKey(errorCode: String): String =
            when (errorCode) {
                "IO_ERROR" -> "error.io"
                "JSON_ERROR" -> "error.json"
                "PROJECT_NOT_FOUND" -> "error.project_not_found"
                "VOLUME_NOT_FOUND" -> "error.volume_not_found"
                "CHAPTER_NOT_FOUND" -> "error.chapter_not_found"
                "EMPTY_OVERWRITE_BLOCKED" -> "error.empty_overwrite_blocked"
                "NOT_IMPLEMENTED" -> "error.not_implemented"
                "REFUSE_DELETE_ROOT" -> "error.refuse_delete_root"
                "INVALID_DELETE_TARGET" -> "error.invalid_delete_target"
                "SYNC_CONFLICT" -> "error.sync_conflict"
                "SYNC_FAILED" -> "error.sync_failed"
                "SYNC_AUTH_FAILED" -> "error.sync_auth_failed"
                "SYNC_NETWORK_UNAVAILABLE" -> "error.sync_network_unavailable"
                "SYNC_RATE_LIMITED" -> "error.sync_rate_limited"
                "SYNC_DOCUMENT_CONFLICT" -> "error.sync_document_conflict"
                "SYNC_INCOMPLETE_TRANSACTION" -> "error.sync_incomplete_transaction"
                "SYNC_CHECKOUT_CONFLICT" -> "error.sync_checkout_conflict"
                "SYNC_SETTINGS_CONFLICT" -> "error.sync_settings_conflict"
                "SYNC_CONFLICT_DETECTED" -> "error.sync_conflict_detected"
                "SYNC_NON_FAST_FORWARD" -> "error.sync_non_fast_forward"
                "SYNC_UNRELATED_HISTORIES" -> "error.sync_unrelated_histories"
                "SYNC_REMOTE_BRANCH_NOT_FOUND" -> "error.sync_remote_branch_not_found"
                "SYNC_GITHUB_API_ERROR" -> "error.sync_github_api_error"
                "DISK_FULL" -> "error.disk_full"
                "STORAGE_TRANSACTION_INCOMPLETE" -> "error.storage_transaction_incomplete"
                "SAVE_QUEUE_FLUSH_INCOMPLETE" -> "error.save_queue_flush_incomplete"
                "NATIVE_ERROR" -> "error.native_error"
                "PARSE_ERROR" -> "error.parse_error"
                "NOT_IMPLEMENTED_BRIDGE" -> "error.not_implemented_bridge"
                "FLUSH_ERROR" -> "error.flush"
                "CLOSE_ERROR" -> "error.close"
                "FLUSH_ALL_ERROR" -> "error.flush_all"
                "CONVERSION_ERROR" -> "error.conversion"
                "SNAPSHOT_CACHE_NOT_INITIALIZED" -> "error.snapshot_cache_not_initialized"
                "STAR_MAP_CACHE_MISSING" -> "error.star_map_cache_missing"
                "NATIVE_NOT_LOADED" -> "error.native_not_loaded"
                "UNKNOWN" -> "error.unknown"
                "OTHER" -> "error.other"
                else -> "error.other"
            }
    }
}
