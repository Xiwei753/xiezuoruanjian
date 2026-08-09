package com.xiwei.sujian.feature.sync.data

import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger

sealed class ExclusiveResult<out T> {
    object Busy : ExclusiveResult<Nothing>()

    data class Success<out T>(val taskId: Int, val value: T) : ExclusiveResult<T>()
}

object SyncSession {
    private val lock = AtomicBoolean(false)
    private val currentTaskId = AtomicInteger(0)

    suspend fun <T> runExclusive(block: suspend (taskId: Int) -> T): ExclusiveResult<T> {
        if (!lock.compareAndSet(false, true)) return ExclusiveResult.Busy
        val taskId = currentTaskId.incrementAndGet()
        return try {
            ExclusiveResult.Success(taskId, block(taskId))
        } finally {
            lock.set(false)
        }
    }
}
