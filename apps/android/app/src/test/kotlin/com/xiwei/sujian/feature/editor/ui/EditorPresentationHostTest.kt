@file:OptIn(kotlinx.coroutines.ExperimentalCoroutinesApi::class)

package com.xiwei.sujian.feature.editor.ui

import android.app.Application
import androidx.test.core.app.ApplicationProvider
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.feature.editor.session.EditorSessionCoordinator
import com.xiwei.sujian.feature.editor.session.TextEditorProfile
import com.xiwei.sujian.feature.editor.visual.ChoreographerAnimationTimeSource
import com.xiwei.sujian.feature.editor.visual.TransactionIdSource
import com.xiwei.sujian.feature.editor.window.EditableTextTarget
import com.xiwei.sujian.feature.editor.window.EditorWindowHost
import kotlinx.coroutines.async
import kotlinx.coroutines.delay
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #640 评论 5441010318 项3：EditorPresentationHost / EditorWindowHost.awaitPresentationReady
 * 行为测试 — 验证 await 已改为 combine(presentationReady, sessionStateFlow)。
 *
 * 场景1（active 切换）：ready=null，active=A，开始 await(A)；只把 active 改成 B，
 * 不 publish 任何 ready；await(A) 必须立即返回 false。关键：await 现在订阅
 * sessionStateFlow，active 变化触发 combine 重新计算，不依赖 ready 流再发一次。
 *
 * 场景2（几何切换）：A 在 800x1200 ready；尺寸切到 800x600；旧 geometry 不得继续
 * 作为最终 ready；新 geometry publish 后才得到 800x600 ready。
 *
 * 测试通过 [EditorSessionCoordinator.mutateSession] 直接设置 activeTargetId，
 * 不依赖 native session 创建 — 聚焦验证 await 的 combine 逻辑而非 bind 路径。
 * presentationReady 几何由 [EditorWindowHost.publishPresentationReadyForTest] /
 * [EditorWindowHost.invalidatePresentationGeometryForTest] 驱动。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class EditorPresentationHostTest {
    private companion object {
        const val TARGET_A_ID = "chapter-body:p:v:c-a"
        const val TARGET_B_ID = "chapter-body:p:v:c-b"
    }

    private fun createHost(): EditorWindowHost {
        val context = ApplicationProvider.getApplicationContext<Application>()
        val bridge =
            AppServiceBridge(
                WriterAppServiceHolder(
                    "/home/xiwei/.cache/agent-tmp/sujian_test_640_ready",
                    "/home/xiwei/.cache/agent-tmp/sujian_test_640_ready",
                ),
            )
        val coordinator = EditorSessionCoordinator(bridge)
        return EditorWindowHost(
            context,
            coordinator,
            bridge,
            ChoreographerAnimationTimeSource(),
            TransactionIdSource(),
        )
    }

    /**
     * #640 评论 5441849412 问题1：直接通过 [EditorSessionCoordinator.mutateSession] 设置
     * 逻辑 session 身份 targetId — 不依赖 native session 创建。
     *
     * awaitPresentationReady 现在用 [EditorSessionState.targetId] 判断过期（而非 activeTargetId）。
     * 此 helper 只设置 targetId，保留 activeTargetId=null，模拟 commitPreparedSession 后的
     * Detached 预热态（targetId=B, bindingState=Detached(B), activeTargetId=null）。
     */
    private fun setSessionTargetId(
        host: EditorWindowHost,
        targetId: String?,
    ) {
        host.sessionCoordinator.mutateSession {
            sessionState = sessionState.copy(targetId = targetId)
        }
    }

    @Test
    fun awaitPresentationReady_returnsFalse_immediatelyWhenActiveTargetSwitched() =
        runTest(UnconfinedTestDispatcher()) {
            val host = createHost()
            val targetA =
                EditableTextTarget(
                    TARGET_A_ID,
                    TextEditorProfile.DocumentBody,
                    isPersistent = true,
                )
            host.registerTarget(targetA)
            // ready=null，session 逻辑身份 targetId=A（模拟 Detached 预热态：activeTargetId=null）
            setSessionTargetId(host, targetA.targetId)
            assertEquals(targetA.targetId, host.sessionStateFlow.value.targetId)
            assertEquals(null, host.sessionStateFlow.value.activeTargetId)
            assertEquals(null, host.presentationReady.value)
            // 开始 await(A)
            val awaitA = async { host.awaitPresentationReady(targetA.targetId) }
            delay(10)
            // ready=null, session.targetId=A → else -> null，仍在等
            assertFalse("ready=null 且 session.targetId=A 时 await(A) 不应完成", awaitA.isCompleted)
            // 把 session 逻辑身份 targetId 改成 B，不 publish 任何 ready
            // （#640 评论 5441849412 问题1：await 现在用 session.targetId 判断过期，而非 activeTargetId。
            // activeTargetId 在 Detached 预热态为 null，不能用于过期判断——否则会把"B 还没 attach"
            // 误判成"B 已经过期"。此处切换 targetId 模拟真正切章节。）
            setSessionTargetId(host, TARGET_B_ID)
            assertEquals(
                TARGET_B_ID,
                host.sessionStateFlow.value.targetId,
            )
            // await(A) 必须立即返回 false — session 逻辑身份已不等于 A
            assertFalse(
                "session.targetId 切到 B 后 await(A) 必须立即返回 false，不依赖 ready 流再发一次",
                awaitA.await(),
            )
        }

    @Test
    fun awaitPresentationReady_oldGeometryDoesNotContinue_newGeometryPublishHits() =
        runTest(UnconfinedTestDispatcher()) {
            val host = createHost()
            val targetA =
                EditableTextTarget(
                    TARGET_A_ID,
                    TextEditorProfile.DocumentBody,
                    isPersistent = true,
                )
            host.registerTarget(targetA)
            setSessionTargetId(host, targetA.targetId)
            // A 在 800x1200 ready
            host.publishPresentationReadyForTest(targetA.targetId, 800, 1200)
            assertTrue(host.isPresentationReady(targetA.targetId))
            val readyBefore = host.presentationReady.value
            assertNotNull(readyBefore)
            assertEquals(800, readyBefore!!.widthPx)
            assertEquals(1200, readyBefore.heightPx)
            // await(A) 命中 800x1200
            assertTrue(host.awaitPresentationReady(targetA.targetId))
            // 尺寸切到 800x600 — 旧 geometry 失效
            host.invalidatePresentationGeometryForTest()
            // 旧 geometry 不得继续作为最终 ready
            assertFalse(host.isPresentationReady(targetA.targetId))
            assertEquals(null, host.presentationReady.value)
            // 开始 await(A)，旧 geometry 不应让它立即返回
            val awaitA = async { host.awaitPresentationReady(targetA.targetId) }
            delay(10)
            assertFalse(
                "旧 geometry 不得继续作为最终 ready — await(A) 不应在 invalidateGeometry 后立即完成",
                awaitA.isCompleted,
            )
            // 新 geometry publish 后才得到 800x600 ready
            host.publishPresentationReadyForTest(targetA.targetId, 800, 600)
            assertTrue(
                "新 geometry publish 后 await(A) 应命中 800x600",
                awaitA.await(),
            )
            val readyAfter = host.presentationReady.value
            assertNotNull(readyAfter)
            assertEquals(800, readyAfter!!.widthPx)
            assertEquals(600, readyAfter.heightPx)
        }

    @Test
    fun awaitPresentationReady_returnsTrue_immediatelyWhenAlreadyReady() =
        runTest(UnconfinedTestDispatcher()) {
            val host = createHost()
            val targetA =
                EditableTextTarget(
                    TARGET_A_ID,
                    TextEditorProfile.DocumentBody,
                    isPersistent = true,
                )
            host.registerTarget(targetA)
            setSessionTargetId(host, targetA.targetId)
            host.publishPresentationReadyForTest(targetA.targetId, 1080, 2000)
            assertTrue(host.awaitPresentationReady(targetA.targetId))
        }

    @Test
    fun awaitPresentationReady_returnsTrue_whenGeometryPublishedLater() =
        runTest(UnconfinedTestDispatcher()) {
            val host = createHost()
            val targetA =
                EditableTextTarget(
                    TARGET_A_ID,
                    TextEditorProfile.DocumentBody,
                    isPersistent = true,
                )
            host.registerTarget(targetA)
            setSessionTargetId(host, targetA.targetId)
            val deferred = async { host.awaitPresentationReady(targetA.targetId) }
            delay(10)
            assertFalse(host.isPresentationReady(targetA.targetId))
            host.publishPresentationReadyForTest(targetA.targetId, 1080, 2000)
            assertTrue(deferred.await())
        }

    @Test
    fun awaitPresentationReady_doesNotReturnFalseOnTransientNullWhenActiveStable() =
        runTest(UnconfinedTestDispatcher()) {
            val host = createHost()
            val targetA =
                EditableTextTarget(
                    TARGET_A_ID,
                    TextEditorProfile.DocumentBody,
                    isPersistent = true,
                )
            host.registerTarget(targetA)
            setSessionTargetId(host, targetA.targetId)
            val awaitA = async { host.awaitPresentationReady(targetA.targetId) }
            delay(10)
            // 几何失效但 session 身份不变（尺寸变化中间态）— await 不应返回 false，应继续等
            host.invalidatePresentationGeometryForTest()
            delay(10)
            assertFalse(awaitA.isCompleted)
            // 新几何发布后才命中
            host.publishPresentationReadyForTest(targetA.targetId, 1080, 2000)
            assertTrue(awaitA.await())
        }

    @Test
    fun awaitPresentationReady_forReplacedTarget_returnsFalseWhileNewTargetSucceeds() =
        runTest(UnconfinedTestDispatcher()) {
            val host = createHost()
            val targetA =
                EditableTextTarget(
                    TARGET_A_ID,
                    TextEditorProfile.DocumentBody,
                    isPersistent = true,
                )
            host.registerTarget(targetA)
            // target A 开始等待（session.targetId=A，ready=null）
            setSessionTargetId(host, targetA.targetId)
            val awaitA = async { host.awaitPresentationReady(targetA.targetId) }
            delay(10)
            // 用户切到 target B — session 逻辑身份变为 B
            setSessionTargetId(host, TARGET_B_ID)
            // A 的 await 快速返回 false
            assertFalse(awaitA.await())
            // B 发布 ready 后命中
            setSessionTargetId(host, TARGET_B_ID)
            host.publishPresentationReadyForTest(TARGET_B_ID, 1080, 2000)
            assertTrue(host.awaitPresentationReady(TARGET_B_ID))
            assertFalse(host.isPresentationReady(targetA.targetId))
            assertTrue(host.isPresentationReady(TARGET_B_ID))
        }

    /**
     * #640 评论 5441849412 问题1：锁定核心场景 — commitPreparedSession 在章节 A 提交成功后
     * 故意把 A 放成 targetId=A, bindingState=Detached(A), activeTargetId=null，然后页面才提交
     * PreparedEditorTarget(A)，AndroidView 才开始 bind。真实打开流程里 awaitPresentationReady(A)
     * 第一次 combine 看到 ready=null, session.targetId=A, session.activeTargetId=null。
     *
     * 旧实现用 activeTargetId 判断过期会立刻返回 false（null != A），把"A 还没 attach"误判成
     * "A 已经过期"。改用 session.targetId 判断后，此场景 await(A) 不应提前返回 false，应继续等，
     * 直到 publish A ready 才返回 true。
     */
    @Test
    fun awaitPresentationReady_doesNotReturnFalseWhenDetachedWithNullActiveTarget() =
        runTest(UnconfinedTestDispatcher()) {
            val host = createHost()
            val targetA =
                EditableTextTarget(
                    TARGET_A_ID,
                    TextEditorProfile.DocumentBody,
                    isPersistent = true,
                )
            host.registerTarget(targetA)
            // 模拟真实预热态：targetId=A + activeTargetId=null（bindingState 默认 Idle，
            // 真实流程为 Detached(A)；await 只看 session.targetId，不依赖 bindingState）。
            setSessionTargetId(host, targetA.targetId)
            assertEquals(targetA.targetId, host.sessionStateFlow.value.targetId)
            assertEquals(null, host.sessionStateFlow.value.activeTargetId)
            assertEquals(null, host.presentationReady.value)
            // 开始 await(A)
            val awaitA = async { host.awaitPresentationReady(targetA.targetId) }
            delay(10)
            // ready=null, session.targetId=A, session.activeTargetId=null →
            // 旧实现（用 activeTargetId）会立即 false；新实现（用 targetId）应继续等。
            assertFalse(
                "Detached 预热态（targetId=A, activeTargetId=null, ready=null）下 await(A) 不应提前完成",
                awaitA.isCompleted,
            )
            // publish A ready 后命中 true
            host.publishPresentationReadyForTest(targetA.targetId, 1080, 2000)
            assertTrue(
                "publish A ready 后 await(A) 应命中 true，而非被误判过期",
                awaitA.await(),
            )
        }

    /**
     * #640 评论 5441849412 问题1：A→B 替换 — targetId=A 预热 await(A)，然后 session 逻辑身份
     * 切到 B（模拟切章节），await(A) 返回 false。与 [awaitPresentationReady_returnsFalse_immediatelyWhenActiveTargetSwitched]
     * 互补：前者聚焦"切换 targetId 立即 false"，此测试聚焦"预热 A 等待 → 真正切 B → A 过期 false"
     * 的完整切章节流程。
     */
    @Test
    fun awaitPresentationReady_returnsFalse_whenSessionTargetReplacedAtoB() =
        runTest(UnconfinedTestDispatcher()) {
            val host = createHost()
            val targetA =
                EditableTextTarget(
                    TARGET_A_ID,
                    TextEditorProfile.DocumentBody,
                    isPersistent = true,
                )
            host.registerTarget(targetA)
            // A 预热：targetId=A, activeTargetId=null
            setSessionTargetId(host, targetA.targetId)
            val awaitA = async { host.awaitPresentationReady(targetA.targetId) }
            delay(10)
            // 预热态下 await(A) 不提前结束
            assertFalse(
                "预热态 targetId=A 时 await(A) 不应提前完成",
                awaitA.isCompleted,
            )
            // 切到章节 B — session 逻辑身份变为 B
            setSessionTargetId(host, TARGET_B_ID)
            // 旧 A 等待返回 false
            assertFalse(
                "session.targetId 切到 B 后 await(A) 应返回 false",
                awaitA.await(),
            )
        }
}
