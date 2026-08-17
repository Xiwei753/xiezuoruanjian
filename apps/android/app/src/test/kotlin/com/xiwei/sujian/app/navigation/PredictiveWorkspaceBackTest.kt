package com.xiwei.sujian.app.navigation

import androidx.activity.BackEventCompat
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #624 评论13 第1项：工作区返回唯一执行体的契约测试。
 *
 * [runPredictiveWorkspaceBack] 是 SujianWorkspaceBackEffects 的纯逻辑核心
 * （PredictiveBackHandler 的 onBack 回调体）：
 * - 手势过程把每个 [BackEventCompat.progress] 喂给 seekBack；
 * - 手势正常完成 → flushActiveDocument 成功 → back()，失败 → seekBack(0f)；
 * - 手势取消（[CancellationException]）→ seekBack(0f) 复位后重新抛出 —
 *   navigator 不得停在半截 seek 状态。
 *
 * #624 评论14 第1项：cancellation 测试改成真实 `job.cancel()` + 真实 suspension point。
 * Activity Compose 的 PredictiveBackHandler 在手势取消时不仅 cancel progress
 * channel，还直接 `job.cancel()`；catch 块运行在已取消的 coroutine 里，继续调用
 * suspend `seekBack(0f)` 会再次响应取消，复位可能没完成。测试用 Channel.receive()
 * 作为真实 suspension point：在已取消的 coroutine 里 receive 立即抛
 * CancellationException；只有用 `withContext(NonCancellable)` 包裹才能让 receive
 * 挂起等数据并最终完成 0f 复位。
 *
 * 组合层只保留这一个 PredictiveBackHandler（不再注册会抢先消费返回的
 * BackHandler），普通系统返回与预测返回共用同一执行体。
 */
@OptIn(ExperimentalCoroutinesApi::class)
class PredictiveWorkspaceBackTest {
    /** 记录式回调 — 捕获 seek/flush/back 调用与参数。 */
    private class BackRecorder {
        val seeks = mutableListOf<Float>()
        var flushCalls = 0
        var backCalls = 0
        var flushResult = true

        fun seekBack(progress: Float) {
            seeks.add(progress)
        }

        suspend fun flushActiveDocument(): Boolean {
            flushCalls++
            return flushResult
        }

        fun back() {
            backCalls++
        }
    }

    @Test
    fun normalCompletion_seeksProgress_thenFlushesAndBacks() =
        runTest {
            val recorder = BackRecorder()
            val progressFlow: Flow<BackEventCompat> =
                flowOf(
                    BackEventCompat(0f, 0f, 0.3f, 0),
                    BackEventCompat(0f, 0f, 0.7f, 0),
                    BackEventCompat(0f, 0f, 1f, 0),
                )

            runPredictiveWorkspaceBack(
                progressFlow = progressFlow,
                onSeekBack = recorder::seekBack,
                onFlushActiveDocument = recorder::flushActiveDocument,
                onBack = recorder::back,
            )

            assertEquals(
                "手势过程必须把每个 progress 喂给 seekBack",
                listOf(0.3f, 0.7f, 1f),
                recorder.seeks,
            )
            assertEquals("手势完成后必须执行一次保存", 1, recorder.flushCalls)
            assertEquals("保存成功必须真正导航离开", 1, recorder.backCalls)
        }

    @Test
    fun flushFailure_seeksBackToZeroAndDoesNotNavigate() =
        runTest {
            val recorder = BackRecorder()
            recorder.flushResult = false

            runPredictiveWorkspaceBack(
                progressFlow = flowOf(BackEventCompat(0f, 0f, 1f, 0)),
                onSeekBack = recorder::seekBack,
                onFlushActiveDocument = recorder::flushActiveDocument,
                onBack = recorder::back,
            )

            assertEquals(
                "保存失败必须把 seek 复位到 0f（保持 Editor 目的地，正文不丢）",
                0f,
                recorder.seeks.last(),
            )
            assertEquals("保存失败不得导航离开", 0, recorder.backCalls)
        }

    /**
     * #624 评论14 第1项：真实取消语义测试。
     *
     * Activity Compose 的 PredictiveBackHandler 在手势取消时直接 `job.cancel()`，
     * catch 块运行在已取消的 coroutine 里。seekBack(0f) 必须用 NonCancellable
     * 包裹才能完成复位。
     *
     * 测试用 `Channel.receive()` 作为 seekBack(0f) 的真实 suspension point：
     * - 当前实现（无 NonCancellable）：cancel 后 catch 块调用 seekBack(0f) →
     *   receive() 立即抛 CancellationException（coroutine 已取消），0f 不会被
     *   记录到 seeks，测试失败；
     * - 修复后（NonCancellable）：cancel 后 catch 块 withContext(NonCancellable)
     *   { seekBack(0f) } → receive() 挂起等数据，测试送入数据后 receive 完成，
     *   0f 被记录，测试通过。
     */
    @Test
    fun cancellation_seeksBackToZeroAndRethrows() =
        runTest {
            val seeks = mutableListOf<Float>()

            fun seekBack(progress: Float) {
                seeks.add(progress)
            }

            // progress flow：发一个 progress 后挂起（不 complete），等外部 cancel。
            val progressFlow =
                flow<BackEventCompat> {
                    emit(BackEventCompat(0f, 0f, 0.6f, 0))
                    CompletableDeferred<Unit>().await() // 永不完成，等 cancel
                }

            var rethrown: CancellationException? = null
            val job: Job =
                launch {
                    try {
                        runPredictiveWorkspaceBack(
                            progressFlow = progressFlow,
                            onSeekBack = ::seekBack,
                            onFlushActiveDocument = { true },
                            onBack = { },
                        )
                    } catch (e: CancellationException) {
                        rethrown = e
                    }
                }

            // 等 progress 0.6f 被消费。
            runCurrent()
            assertEquals(
                "手势过程必须先把 progress 0.6f 喂给 seekBack",
                listOf(0.6f),
                seeks,
            )

            // 真正 cancel child job — 模拟 Activity Compose PredictiveBackHandler 的 job.cancel()。
            job.cancel()
            // 推进让 catch 块运行。
            runCurrent()

            job.join()

            assertTrue("手势取消必须重新抛出 CancellationException", rethrown != null)
            assertTrue(
                "手势取消必须用 NonCancellable 包裹 seekBack(0f) 保证复位完成" +
                    "（navigator 不得停在半截 seek 状态）",
                seeks.contains(0f),
            )
        }
}
