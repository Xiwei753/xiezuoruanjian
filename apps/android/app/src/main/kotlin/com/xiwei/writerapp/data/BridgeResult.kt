package com.xiwei.writerapp.data

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
