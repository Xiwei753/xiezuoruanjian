package com.xiwei.sujian.ui

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.async
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 一：章节切换串行门契约测试。
 *
 * 旧缺陷：switchChapter 没有事务锁或请求序号，用户连续点击多个章节时，
 * 旧任务可能在新任务之后完成并再次回滚状态。
 *
 * 修复：ChapterSwitchGate 串行执行 + requestId 校验 —
 * - 同一时间只有一个事务执行（Mutx 串行）；
 * - 进入锁时发现已有更新的请求（排队或执行中）→ 返回 Stale，不执行任何代码；
 * - 旧请求绝不可能在新请求之后提交（锁内校验 + FIFO 排队）。
 */
class ChapterSwitchGateTest {

    @Test
    fun singleRequest_completesNormally() = runTest {
        val gate = ChapterSwitchGate()
        val result = gate.runLatest { 42 }
        assertEquals(ChapterSwitchGate.Result.Completed(42), result)
    }

    @Test
    fun supersededRequest_returnsStaleWithoutRunning() = runTest {
        val gate = ChapterSwitchGate()
        val release = CompletableDeferred<Unit>()
        var firstRan = false

        // 第一个请求拿到锁并在事务内挂起（模拟保存/加载 IO）
        val first = async { gate.runLatest { firstRan = true; release.await(); "first" } }
        runCurrent()

        // 第二个请求排队（counter=2），第三个请求随后到达（counter=3）
        val second = async { gate.runLatest { "second" } }
        val third = async { gate.runLatest { "third" } }
        runCurrent()

        release.complete(Unit)

        assertEquals(ChapterSwitchGate.Result.Completed("first"), first.await())
        assertEquals(
            "排队期间被更新的请求取代 → 必须返回 Stale 且不执行事务",
            ChapterSwitchGate.Result.Stale,
            second.await(),
        )
        assertEquals(ChapterSwitchGate.Result.Completed("third"), third.await())
        assertTrue("第一个请求的事务必须执行过", firstRan)
    }

    @Test
    fun queuedRequestWithNoNewerRequest_stillRuns() = runTest {
        val gate = ChapterSwitchGate()
        val release = CompletableDeferred<Unit>()

        val first = async { gate.runLatest { release.await(); "first" } }
        runCurrent()
        val second = async { gate.runLatest { "second" } }
        runCurrent()

        release.complete(Unit)

        assertEquals(ChapterSwitchGate.Result.Completed("first"), first.await())
        assertEquals(
            "没有更新的请求时，排队的第二个请求必须正常执行",
            ChapterSwitchGate.Result.Completed("second"),
            second.await(),
        )
    }

    @Test
    fun cancellation_releasesLockAndAllowsNextRequest() = runTest {
        val gate = ChapterSwitchGate()
        val release = CompletableDeferred<Unit>()
        var cancelled = false

        val first = async {
            try {
                gate.runLatest { release.await() }
            } catch (e: kotlinx.coroutines.CancellationException) {
                cancelled = true
                throw e
            }
        }
        runCurrent()

        val second = async { gate.runLatest { "second" } }
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
