package com.xiwei.sujian.app.navigation

import androidx.activity.BackEventCompat
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.flowOf
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
 * 组合层只保留这一个 PredictiveBackHandler（不再注册会抢先消费返回的
 * BackHandler），普通系统返回与预测返回共用同一执行体。
 */
class PredictiveWorkspaceBackTest {
    /** 记录式回调 — 捕获 seek/flush/back 调用与参数。 */
    private class BackRecorder {
        val seeks = mutableListOf<Float>()
        var flushCalls = 0
        var backCalls = 0
        var flushResult = true

        suspend fun seekBack(progress: Float) {
            seeks.add(progress)
        }

        suspend fun flushActiveDocument(): Boolean {
            flushCalls++
            return flushResult
        }

        suspend fun back() {
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

    @Test
    fun cancellation_seeksBackToZeroAndRethrows() =
        runTest {
            val recorder = BackRecorder()
            val progressFlow =
                flow<BackEventCompat> {
                    emit(BackEventCompat(0f, 0f, 0.6f, 0))
                    throw CancellationException("gesture cancelled")
                }

            var rethrown: CancellationException? = null
            try {
                runPredictiveWorkspaceBack(
                    progressFlow = progressFlow,
                    onSeekBack = recorder::seekBack,
                    onFlushActiveDocument = recorder::flushActiveDocument,
                    onBack = recorder::back,
                )
            } catch (e: CancellationException) {
                rethrown = e
            }

            assertTrue("手势取消必须重新抛出 CancellationException", rethrown != null)
            assertEquals(
                "手势取消必须把 seek 复位到 0f（navigator 不得停在半截 seek 状态）",
                0f,
                recorder.seeks.last(),
            )
            assertEquals("手势取消不得导航离开", 0, recorder.backCalls)
            assertEquals("手势取消不得执行保存", 0, recorder.flushCalls)
        }
}
