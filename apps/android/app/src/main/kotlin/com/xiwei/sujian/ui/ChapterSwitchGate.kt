package com.xiwei.sujian.ui

import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import java.util.concurrent.atomic.AtomicLong

/**
 * #595 一：章节切换串行门 — 真正 latest-wins。
 *
 * 旧缺陷：只在请求取得 Mutex 后检查一次 requestId；B 进入锁并开始保存/加载时
 * C 到达并排队，B 不再检查 requestId 仍提交 Success — 宿主先导航到 B，C 随后
 * 再导航到 C，B 短暂闪现并污染返回历史。
 *
 * 修复：
 * - [runLatest] 串行执行；请求进入时递增 [AtomicLong]，在锁内校验自己是否仍是最新；
 * - 事务执行期间通过 [isLatest] 回调在每个可见提交边界（保存后 / 加载后 /
 *   session 预准备后 / 最终提交前）重新校验 — 过期请求必须回滚临时状态后返回
 *   [Result.Stale]，不得提交任何可见状态；
 * - 事务返回后 [runLatest] 再次校验 — 即使事务代码漏检，过期事务的结果也不会
 *   以 Completed 形式被调用方消费（调用方不得导航）；
 * - 请求执行期间被取消时，锁由 [Mutex.withLock] 自动释放，事务由调用方负责
 *   恢复旧状态（见 EditorViewModel.switchChapterLocked）。
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
     *
     * [request] 收到 [isLatest] 回调，必须在每个可见提交边界调用：
     * 返回 false 表示已有更新的请求排队，本次事务必须回滚临时状态并返回
     * [ChapterSwitchResult.Stale]，不能提交。
     */
    suspend fun <T> runLatest(request: suspend (isLatest: () -> Boolean) -> T): Result<T> {
        val requestId = counter.incrementAndGet()
        return mutex.withLock {
            if (requestId != counter.get()) {
                return@withLock Result.Stale
            }
            val value = request { requestId == counter.get() }
            if (requestId != counter.get()) {
                // 事务执行期间有更新的请求到达：即使事务内部漏检，
                // 其结果也不得作为 Completed 交付 — 调用方不得提交导航。
                Result.Stale
            } else {
                Result.Completed(value)
            }
        }
    }
}
