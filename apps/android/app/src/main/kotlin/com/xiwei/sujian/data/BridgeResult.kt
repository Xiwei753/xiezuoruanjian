package com.xiwei.sujian.data


import com.google.gson.Gson
import com.google.gson.reflect.TypeToken

sealed class BridgeResult<out T> {
    data class Success<out T>(val data: T, val envelope: ResultEnvelope<T> = ResultEnvelope.success(data)) : BridgeResult<T>()
    data class Error(
        val envelope: ResultEnvelope<Nothing>
    ) : BridgeResult<Nothing>() {
        val message: String get() = envelope.messageKey ?: envelope.errorCode ?: ""
        val code: String? get() = envelope.errorCode
    }
    object NotLoaded : BridgeResult<Nothing>() {
        @Suppress("DEPRECATION")
        val envelope: ResultEnvelope<Nothing> = ResultEnvelope(
            success = false,
            errorCode = "NATIVE_NOT_LOADED",
            userMessage = null
        )
    }
}

data class ChangedEntity(
    val entityType: String,
    val entityId: String? = null
)

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
            else -> "error.other"
        }
    }
}

internal inline fun <reified T> BridgeResult<String>.parseJsonResult(
    gson: Gson,
    label: String
): BridgeResult<T> {
    return when (this) {
        is BridgeResult.Success -> try {
            val type = object : TypeToken<T>() {}.type
            BridgeResult.Success(gson.fromJson<T>(data, type))
        } catch (e: Exception) {
            BridgeResult.Error(
                ResultEnvelope.error("JSON_ERROR", "JSON parse error for $label: ${e.message ?: e.javaClass.simpleName}")
            )
        }
        is BridgeResult.Error -> this
        BridgeResult.NotLoaded -> BridgeResult.NotLoaded
    }
}
