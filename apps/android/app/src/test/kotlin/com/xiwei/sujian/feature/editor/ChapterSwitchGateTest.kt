package com.xiwei.sujian.feature.editor

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.async
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
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
}
