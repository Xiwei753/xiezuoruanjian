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
 * 工作区返回执行体的契约测试。
 *
 * [runPredictiveWorkspaceBack] 是 SujianWorkspaceBackEffects 的纯逻辑核心
 * （PredictiveBackHandler 的 onBack 回调体）：
 * - 手势完成后先 flushActiveDocument 保存活动正文 — 成功才 back() 导航；
 * - 手势取消（[CancellationException]）直接向上传播。
 */
@OptIn(ExperimentalCoroutinesApi::class)
class PredictiveWorkspaceBackTest {
    /** 记录式回调 — 捕获 flush/back 调用与参数。 */
    private class BackRecorder {
        var flushCalls = 0
        var backCalls = 0
        var flushResult = true

        fun flushActiveDocument(): Boolean {
            flushCalls++
            return flushResult
        }

        fun back() {
            backCalls++
        }
    }

    @Test
    fun normalCompletion_flushesAndBacks() =
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
                onFlushActiveDocument = recorder::flushActiveDocument,
                onBack = recorder::back,
            )

            assertEquals("手势完成后必须执行一次保存", 1, recorder.flushCalls)
            assertEquals("保存成功必须真正导航离开", 1, recorder.backCalls)
        }

    @Test
    fun flushFailure_doesNotNavigate() =
        runTest {
            val recorder = BackRecorder()
            recorder.flushResult = false

            runPredictiveWorkspaceBack(
                progressFlow = flowOf(BackEventCompat(0f, 0f, 1f, 0)),
                onFlushActiveDocument = recorder::flushActiveDocument,
                onBack = recorder::back,
            )

            assertEquals("保存失败不得导航离开", 0, recorder.backCalls)
        }

    @Test
    fun cancellation_rethrows() =
        runTest {
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
                            onFlushActiveDocument = { true },
                            onBack = { },
                        )
                    } catch (e: CancellationException) {
                        rethrown = e
                    }
                }

            // 真正 cancel child job — 模拟 Activity Compose PredictiveBackHandler 的 job.cancel()。
            job.cancel()
            // 推进让 collect 响应取消。
            runCurrent()

            job.join()

            assertTrue("手势取消必须重新抛出 CancellationException", rethrown != null)
        }
}
