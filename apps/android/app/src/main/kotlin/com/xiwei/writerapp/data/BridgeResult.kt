package com.xiwei.writerapp.data

import com.google.gson.Gson
import com.google.gson.reflect.TypeToken
import com.xiwei.writerapp.model.BridgeError
import com.xiwei.writerapp.model.BridgeErrorCode

sealed class BridgeResult<out T> {
    data class Success<out T>(val data: T) : BridgeResult<T>()
    data class Error(val error: BridgeError) : BridgeResult<Nothing>() {
        val message: String get() = error.message
        val code: BridgeErrorCode get() = error.code
    }
    object NotLoaded : BridgeResult<Nothing>()
}

internal fun <T> NativeResult<T>.toBridgeResult(): BridgeResult<T> {
    return when (this) {
        is NativeResult.Success -> BridgeResult.Success(data)
        is NativeResult.Error -> BridgeResult.Error(bridgeError)
        NativeResult.NotLoaded -> BridgeResult.NotLoaded
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
                BridgeError(
                    BridgeErrorCode.JsonError,
                    "解析 $label JSON 失败: ${e.message ?: e.javaClass.simpleName}"
                )
            )
        }
        is BridgeResult.Error -> this
        BridgeResult.NotLoaded -> BridgeResult.NotLoaded
    }
}
