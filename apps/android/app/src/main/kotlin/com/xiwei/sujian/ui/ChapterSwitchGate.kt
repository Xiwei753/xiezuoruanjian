package com.xiwei.sujian.ui

import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import java.util.concurrent.atomic.AtomicLong

/**
 * #595 一：章节切换串行门 — 同一时间只允许一个切换事务执行，并为每次请求
 * 分配 requestId，保证旧请求不得在新请求之后提交成功或报告失败。
 *
 * 语义：
 * - [runLatest] 串行执行；请求进入时递增 [AtomicLong]，在锁内校验自己是否
 *   仍是最新请求；
 * - 过期请求（进入时已有更新的请求排队/执行）直接返回 [Result.Stale]，
 *   不执行任何事务代码、不改变任何状态；
 * - 请求执行期间被取消时，锁由 [Mutex.withLock] 自动释放，事务由调用方
 *   负责恢复旧状态（见 EditorViewModel.switchChapterLocked）。
 *
 * 线程安全由 Mutex + AtomicLong 保证，无 unsafe、无并行状态机。
 */
class ChapterSwitchGate {
    private val counter = AtomicLong(0L)
    private val mutex = Mutex()

    sealed interface Result<out T> {
        data class Completed<T>(val value: T) : Result<T>
        data object Stale : Result<Nothing>
    }

    /**
     * 只有最新请求可以执行 [request]；过期请求返回 [Result.Stale]。
     * 锁在整个事务期间持有 — 后续请求严格按到达顺序排队，
     * 任何旧请求都不可能在新请求之后提交。
     */
    suspend fun <T> runLatest(request: suspend () -> T): Result<T> {
        val requestId = counter.incrementAndGet()
        return mutex.withLock {
            if (requestId != counter.get()) {
                return@withLock Result.Stale
            }
            Result.Completed(request())
        }
    }
}
