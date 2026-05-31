package com.xiwei.writerapp.data

import com.google.gson.Gson
import com.google.gson.reflect.TypeToken

sealed class BridgeResult<out T> {
    data class Success<out T>(val data: T, val envelope: ResultEnvelope<T> = ResultEnvelope.success(data)) : BridgeResult<T>()
    data class Error(
        val envelope: ResultEnvelope<Nothing>
    ) : BridgeResult<Nothing>() {
        val message: String get() = envelope.userMessage ?: ""
        val code: String? get() = envelope.errorCode
    }
    object NotLoaded : BridgeResult<Nothing>() {
        val envelope: ResultEnvelope<Nothing> = ResultEnvelope(
            success = false,
            errorCode = "NATIVE_NOT_LOADED",
            userMessage = "Native bridge not loaded"
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
    val userMessage: String? = null,
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

        fun error(errorCode: String, userMessage: String): ResultEnvelope<Nothing> = ResultEnvelope(
            success = false,
            errorCode = errorCode,
            userMessage = userMessage,
            rawError = userMessage
        )
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
                ResultEnvelope.error("JSON_ERROR", "解析 $label JSON 失败: ${e.message ?: e.javaClass.simpleName}")
            )
        }
        is BridgeResult.Error -> this
        BridgeResult.NotLoaded -> BridgeResult.NotLoaded
    }
}
