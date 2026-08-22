package com.xiwei.sujian.feature.editor.presentation

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.async
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 一：章节切换串行门契约测试。
 *
 * 旧缺陷：switchChapter 没有事务锁或请求序号，用户连续点击多个章节时，
 * 旧任务可能在新任务之后完成并再次回滚状态。
 *
 * 修复：ChapterSwitchGate 串行执行 + requestId 校验 —
 * - 同一时间只有一个事务执行（Mutex 串行）；
 * - 进入锁时发现已有更新的请求（排队或执行中）→ 返回 Stale，不执行任何代码；
 * - 事务执行期间有更新请求排队 → 事务结果被抑制为 Stale（调用方不得提交
 *   导航）；每个可见提交边界由事务内部经 isLatest() 重新校验；
 * - 旧请求绝不可能在新请求之后提交（锁内校验 + 事务返回后再次校验）。
 */
class ChapterSwitchGateTest {
    @Test
    fun singleRequest_completesNormally() =
        runTest {
            val gate = ChapterSwitchGate()
            val result = gate.runLatest { 42 }
            assertEquals(ChapterSwitchGate.Result.Completed(42), result)
        }

    @Test
    fun supersededRequest_returnsStaleWithoutRunning() =
        runTest {
            val gate = ChapterSwitchGate()
            val release = CompletableDeferred<Unit>()
            var firstRan = false

            // 第一个请求拿到锁并在事务内挂起（模拟保存/加载 IO）
            val first =
                async {
                    gate.runLatest { isLatest ->
                        firstRan = true
                        release.await()
                        "first"
                    }
                }
            runCurrent()

            // 第二个请求排队（counter=2），第三个请求随后到达（counter=3）
            val second = async { gate.runLatest { isLatest -> "second" } }
            val third = async { gate.runLatest { isLatest -> "third" } }
            runCurrent()

            release.complete(Unit)

            assertEquals(
                "#595 一：事务执行期间已有更新请求排队 → 执行者的事务结果必须被抑制为 " +
                    "Stale（调用方不得提交导航，避免 B 闪现后又被 C 覆盖）",
                ChapterSwitchGate.Result.Stale,
                first.await(),
            )
            assertEquals(
                "排队期间被更新的请求取代 → 必须返回 Stale 且不执行事务",
                ChapterSwitchGate.Result.Stale,
                second.await(),
            )
            assertEquals(ChapterSwitchGate.Result.Completed("third"), third.await())
            assertTrue("第一个请求的事务必须执行过", firstRan)
        }

    @Test
    fun queuedRequestWithNoNewerRequest_stillRuns() =
        runTest {
            val gate = ChapterSwitchGate()
            val release = CompletableDeferred<Unit>()

            val first =
                async {
                    gate.runLatest { isLatest ->
                        release.await()
                        "first"
                    }
                }
            runCurrent()
            val second = async { gate.runLatest { isLatest -> "second" } }
            runCurrent()

            release.complete(Unit)

            assertEquals(
                "#595 一：执行期间已有新请求排队 → 执行者结果被抑制为 Stale，由最新请求提交",
                ChapterSwitchGate.Result.Stale,
                first.await(),
            )
            assertEquals(
                "没有更新的请求时，排队的第二个请求必须正常执行",
                ChapterSwitchGate.Result.Completed("second"),
                second.await(),
            )
        }

    @Test
    fun commitBoundaryCheck_suppressesSupersededCommit() =
        runTest {
            // #595 一：事务在提交边界必须重新校验 requestId —
            // B 执行期间 C 到达，B 即使完成了事务主体，提交也会被抑制（Stale）。
            val gate = ChapterSwitchGate()
            val release = CompletableDeferred<Unit>()
            var commitBoundaryIsLatest = false

            val first =
                async {
                    gate.runLatest { isLatest ->
                        release.await()
                        // 提交边界：此时 C 已排队 → isLatest() 为 false
                        commitBoundaryIsLatest = isLatest()
                        "first-commit"
                    }
                }
            runCurrent()
            val second = async { gate.runLatest { isLatest -> "second" } }
            runCurrent()

            release.complete(Unit)

            assertFalse(
                "#595 一：事务执行期间有更新请求排队时，提交边界 isLatest() 必须为 false",
                commitBoundaryIsLatest,
            )
            assertEquals(
                "事务主体完成后发现过期 → 结果必须被抑制为 Stale（调用方不得提交导航）",
                ChapterSwitchGate.Result.Stale,
                first.await(),
            )
            assertEquals(ChapterSwitchGate.Result.Completed("second"), second.await())
        }

    @Test
    fun cancellation_releasesLockAndAllowsNextRequest() =
        runTest {
            val gate = ChapterSwitchGate()
            val release = CompletableDeferred<Unit>()
            var cancelled = false

            val first =
                async {
                    try {
                        gate.runLatest { isLatest -> release.await() }
                    } catch (e: kotlinx.coroutines.CancellationException) {
                        cancelled = true
                        throw e
                    }
                }
            runCurrent()

            val second = async { gate.runLatest { isLatest -> "second" } }
            runCurrent()

            first.cancel()
            // 让取消的 continuation 在调度器上落地（CancellationException 传播）。
            runCurrent()
            release.complete(Unit)

            assertTrue("第一个请求取消必须向上传播 CancellationException", cancelled)
            assertEquals(
                "第一个请求取消释放锁后，第二个请求必须能执行",
                ChapterSwitchGate.Result.Completed("second"),
                second.await(),
            )
        }

    // #632 评论 5378641437：直接覆盖 runChapterSwitch() 的同目标 join 并发竞态。

    private fun session(
        id: String,
        c: String,
    ) = EditorSession(id, "p", "v", c)

    private fun success(
        c: String,
        sid: String = "s1",
    ) = ChapterSwitchResult.Success(session(sid, c))

    /**
     * 并发发两个完全相同的 ChapterSwitchKey：首个 request 用 CompletableDeferred 挂住，
     * 第二个进入后确认 request body 执行次数始终为 1（join 而非重跑）；释放首个后
     * 两个调用拿到同一个结果。
     */
    @Test
    fun sameKey_concurrentRequests_joinSingleTransaction() =
        runTest {
            val gate = ChapterSwitchGate()
            val key = ChapterSwitchGate.ChapterSwitchKey("p", "v", "c")
            val release = CompletableDeferred<Unit>()
            var requestExecutions = 0
            val expected = success("c")

            val first =
                async {
                    gate.runChapterSwitch(key) { isLatest ->
                        requestExecutions++
                        release.await()
                        expected
                    }
                }
            runCurrent()

            val second =
                async {
                    gate.runChapterSwitch(key) { isLatest ->
                        requestExecutions++
                        expected
                    }
                }
            runCurrent()

            assertEquals(
                "#632 评论 5378641437：同 key 并发请求必须 join 同一事务，" +
                    "request body 只执行一次（旧实现两次 withLock 之间解锁可导致都执行）",
                1,
                requestExecutions,
            )

            release.complete(Unit)
            runCurrent()

            val firstResult = first.await()
            val secondResult = second.await()
            assertEquals(
                "首个请求完成后必须以 Completed 交付结果",
                ChapterSwitchGate.Result.Completed(expected),
                firstResult,
            )
            assertEquals(
                "joiner 必须拿到与 owner 完全相同的结果",
                firstResult,
                secondResult,
            )
        }

    /**
     * A key 执行中再发 B key：A 最终为 Stale，B 正常完成 —
     * 保证 get-or-put 没破坏不同目标的 latest-wins。
     */
    @Test
    fun differentKeys_remainLatestWins() =
        runTest {
            val gate = ChapterSwitchGate()
            val keyA = ChapterSwitchGate.ChapterSwitchKey("p", "v", "a")
            val keyB = ChapterSwitchGate.ChapterSwitchKey("p", "v", "b")
            val release = CompletableDeferred<Unit>()

            val first =
                async {
                    gate.runChapterSwitch(keyA) { isLatest ->
                        release.await()
                        success("a")
                    }
                }
            runCurrent()

            val second =
                async {
                    gate.runChapterSwitch(keyB) { isLatest -> success("b") }
                }
            runCurrent()

            release.complete(Unit)
            runCurrent()

            assertEquals(
                "#632：不同 key 仍走 latest-wins — A 被 B 取代后必须返回 Stale",
                ChapterSwitchGate.Result.Stale,
                first.await(),
            )
            assertEquals(
                "不同 key 的最新请求必须正常完成",
                ChapterSwitchGate.Result.Completed(success("b")),
                second.await(),
            )
        }

    /**
     * owner 抛普通异常 → joiner 收到同一个异常；随后同 key 新请求可以重新执行，
     * 证明 inFlight 没泄漏。
     */
    @Test
    fun sameKey_exception_isSharedAndEntryRemoved() =
        runTest {
            val gate = ChapterSwitchGate()
            val key = ChapterSwitchGate.ChapterSwitchKey("p", "v", "c")
            val sharedException = IllegalStateException("boom")
            val release = CompletableDeferred<Unit>()

            // async 抛普通异常会传播到父 scope，故在协程内捕获存变量、不重抛。
            var firstError: Throwable? = null
            val first =
                async {
                    try {
                        gate.runChapterSwitch(key) { isLatest ->
                            release.await()
                            throw sharedException
                        }
                    } catch (e: Throwable) {
                        firstError = e
                    }
                }
            runCurrent()

            var secondError: Throwable? = null
            val second =
                async {
                    try {
                        gate.runChapterSwitch(key) { isLatest -> success("c") }
                    } catch (e: Throwable) {
                        secondError = e
                    }
                }
            runCurrent()

            release.complete(Unit)
            runCurrent()

            first.await()
            second.await()

            // owner 直接 catch 到自己抛出的同一个异常实例。
            assertTrue("owner 抛普通异常必须向上传播", firstError is IllegalStateException)
            assertSame("owner 抛出的必须是同一个异常实例", sharedException, firstError)

            // joiner 通过 CompletableDeferred.await() 恢复，kotlinx.coroutines 在 slow-path
            // 可能包装异常（原始异常作为 cause）。语义上 joiner 仍观察到 owner 的异常：
            // 同类型 + cause 链含原始异常实例。
            assertTrue(
                "joiner 必须收到同类型异常 (IllegalStateException)",
                secondError is IllegalStateException,
            )
            val joinerSeesOwnerException =
                secondError === sharedException || secondError?.cause === sharedException
            assertTrue(
                "joiner 必须观察到 owner 抛出的异常（直接或作为 cause）",
                joinerSeesOwnerException,
            )

            var reran = false
            val third =
                gate.runChapterSwitch(key) { isLatest ->
                    reran = true
                    success("c", "s2")
                }
            assertTrue(
                "owner 异常后 inFlight 必须被清理，同 key 新请求可重新执行",
                reran,
            )
            assertEquals(
                ChapterSwitchGate.Result.Completed(success("c", "s2")),
                third,
            )
        }

    /**
     * owner 取消后 joiner 不挂死（收到取消）；随后同 key 新请求可重新进入，
     * 证明 inFlight 没泄漏。
     */
    @Test
    fun sameKey_cancellation_isSharedAndEntryRemoved() =
        runTest {
            val gate = ChapterSwitchGate()
            val key = ChapterSwitchGate.ChapterSwitchKey("p", "v", "c")
            val release = CompletableDeferred<Unit>()

            val first =
                async {
                    gate.runChapterSwitch(key) { isLatest ->
                        release.await()
                        success("c")
                    }
                }
            runCurrent()

            var secondCancelled = false
            val second =
                async {
                    try {
                        gate.runChapterSwitch(key) { isLatest -> success("c") }
                    } catch (e: kotlinx.coroutines.CancellationException) {
                        secondCancelled = true
                    }
                }
            runCurrent()

            first.cancel()
            runCurrent()
            release.complete(Unit)
            runCurrent()

            assertTrue(
                "owner 取消后 joiner 必须收到取消，不能挂死",
                secondCancelled,
            )
            second.await()

            var reran = false
            val third =
                gate.runChapterSwitch(key) { isLatest ->
                    reran = true
                    success("c", "s2")
                }
            assertTrue(
                "owner 取消后 inFlight 必须被清理，同 key 新请求可重新进入",
                reran,
            )
            assertEquals(
                ChapterSwitchGate.Result.Completed(success("c", "s2")),
                third,
            )
        }
}
