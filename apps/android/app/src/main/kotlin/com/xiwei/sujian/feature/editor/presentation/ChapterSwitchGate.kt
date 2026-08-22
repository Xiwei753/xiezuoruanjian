package com.xiwei.sujian.feature.editor.presentation

import kotlinx.coroutines.CompletableDeferred
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
 * - 事务执行期间通过 [isLatest] 回调在每个可见提交边界重新校验 — 过期请求必须回滚
 *   临时状态后返回 [Result.Stale]，不得提交任何可见状态；
 * - 事务返回后 [runLatest] 再次校验 — 即使事务代码漏检，过期事务的结果也不会
 *   以 Completed 形式被调用方消费（调用方不得导航）；
 * - 请求执行期间被取消时，锁由 [Mutex.withLock] 自动释放，事务由调用方负责
 *   恢复旧状态（见 EditorViewModel.switchChapterLocked）。
 *
 * #632 评论 5378239827 项4：恢复"同目标 join、不同目标 latest-wins"的入口合并。
 * [runChapterSwitch] 按 [ChapterSwitchKey] 合并：
 * - 同 key 已有 in-flight 请求 → 直接 await 现有 deferred，不再执行第二次事务；
 * - 不同 key → 仍走 [runLatest] counter（latest-wins）；
 * - 首个同 key 请求执行完后 complete deferred；
 * - CancellationException 用 deferred.cancel(e) 后重抛；
 * - 普通异常用 deferred.completeExceptionally(e) 后重抛；
 * - finally 里在 inFlightMutex.withLock 中删除当前 key，且只在 map 里仍是当前
 *   deferred 时删除，避免旧请求误删后来的同 key 请求。
 *
 * #632 评论 5378641437：修同目标 join 的真实并发竞态。旧实现把"查已有 inFlight"
 * 和"登记新 deferred"拆成两次 inFlightMutex.withLock，中间解锁 — 两个同 key 请求
 * 可同时在第一次加锁里读到 null，各自创建 deferred 互相覆盖，最后两个都继续执行
 * runLatest(request)，"同章节只做一次 Repository IO / session prepare"的目标仍可能
 * 失效。修复：把 get-or-put 合成一次临界区，明确 owner/joiner；joiner 拿到 shared
 * deferred 后立刻释放锁再 await，不在持有 inFlightMutex 时 await。
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

    // #632 评论 5378239827 项4：同目标 join 的入口合并。

    /** 章节切换请求的目标 key — 同 key 请求共享同一个 in-flight 事务。 */
    data class ChapterSwitchKey(
        val projectId: String,
        val volumeId: String,
        val chapterId: String,
    )

    private val inFlightMutex = Mutex()
    private val inFlight =
        mutableMapOf<ChapterSwitchKey, CompletableDeferred<Result<ChapterSwitchResult>>>()

    /**
     * 按 [ChapterSwitchKey] 合并章节切换请求：
     * - 同 key 已有 in-flight 请求 → 直接 await 现有 deferred；
     * - 不同 key → 走 [runLatest]（latest-wins）。
     *
     * #632 评论 5378641437：get-or-put 在一次 inFlightMutex 临界区内完成，明确
     * owner/joiner — 两个同 key 并发请求不可能各自创建 deferred 互相覆盖后都执行
     * 事务。joiner 拿到 shared deferred 后立刻释放锁再 await，不在持有 inFlightMutex
     * 时 await。
     *
     * 首个同 key 请求执行完后 complete deferred；CancellationException 用
     * deferred.cancel(e) 后重抛；普通异常用 deferred.completeExceptionally(e) 后重抛；
     * finally 里在 inFlightMutex.withLock 中删除当前 key，且只在 map 里仍是当前
     * deferred 时删除。
     */
    suspend fun runChapterSwitch(
        key: ChapterSwitchKey,
        request: suspend (isLatest: () -> Boolean) -> ChapterSwitchResult,
    ): Result<ChapterSwitchResult> {
        // #632 评论 5378641437：查已有 + 登记自己合成一次临界区，原子 get-or-put。
        // 旧实现两次 withLock 之间解锁，两个同 key 请求可都读到 null 后各自创建
        // deferred 互相覆盖，最后两个都执行 runLatest(request)。
        val candidate = CompletableDeferred<Result<ChapterSwitchResult>>()
        val (owner, shared) =
            inFlightMutex.withLock {
                val existing = inFlight[key]
                if (existing != null) {
                    false to existing
                } else {
                    inFlight[key] = candidate
                    true to candidate
                }
            }

        // joiner：拿到 shared deferred 后已释放 inFlightMutex，再 await。
        if (!owner) return shared.await()

        // owner：执行事务，完成后 complete candidate。
        try {
            val result = runLatest(request)
            candidate.complete(result)
            return result
        } catch (e: kotlinx.coroutines.CancellationException) {
            candidate.cancel(e)
            throw e
        } catch (e: Exception) {
            candidate.completeExceptionally(e)
            throw e
        } finally {
            inFlightMutex.withLock {
                // 只在 map 里仍是当前 deferred 时删除，避免旧请求误删后来的同 key 请求。
                if (inFlight[key] === candidate) {
                    inFlight.remove(key)
                }
            }
        }
    }
}
