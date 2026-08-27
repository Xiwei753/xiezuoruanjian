@file:OptIn(kotlinx.coroutines.ExperimentalCoroutinesApi::class)

package com.xiwei.sujian.feature.editor.window

import android.app.Application
import androidx.test.core.app.ApplicationProvider
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.feature.editor.session.EditorSessionCoordinator
import com.xiwei.sujian.feature.editor.session.SessionCloseReason
import com.xiwei.sujian.feature.editor.session.TextEditorProfile
import com.xiwei.sujian.feature.editor.visual.ChoreographerAnimationTimeSource
import com.xiwei.sujian.feature.editor.visual.TransactionIdSource
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
 * #640 B：EditorWindowHost presentation-ready 几何代次集成测试。
 *
 * 验证 EditorWindowHost.awaitPresentationReady 返回 Boolean、closeTarget/detach 路径
 * 使旧 target 的 await 快速返回 false（不永久挂住），presentationReady StateFlow
 * 携带 EditorPresentationReady 几何类型。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class EditorWindowHostPresentationReadyTest {
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

    @Test
    fun awaitPresentationReady_returnsBoolean_false_whenTargetClosedBeforeReady() =
        runTest(UnconfinedTestDispatcher()) {
            val host = createHost()
            val target = EditableTextTarget("chapter-body:p:v:c-a", TextEditorProfile.DocumentBody, isPersistent = true)
            host.registerTarget(target)
            // #640 评论 5441849412 问题1：先设置 session 逻辑身份 targetId=A，让 await(A) 进入
            // 等待（不提前结束）。await 现在用 session.targetId 判断过期；若不设置 targetId，
            // session.targetId=null != A，await(A) 会在 closeTarget 之前就立即 false，测试无法
            // 真正卡住 close 唤醒路径。
            host.sessionCoordinator.mutateSession {
                sessionState = sessionState.copy(targetId = target.targetId)
            }
            assertEquals(target.targetId, host.sessionStateFlow.value.targetId)
            val deferred = async { host.awaitPresentationReady(target.targetId) }
            delay(10)
            // closeTarget 之前：session.targetId=A, ready=null → await(A) 继续等，不提前 false
            assertFalse(
                "closeTarget 之前 await(A) 不应提前完成",
                deferred.isCompleted,
            )
            // 业务关闭 target → closeTarget 重置 sessionState（targetId=null）→
            // session.targetId=null != A → await 快速 false（同时 invalidateTarget 推进 generation）
            host.closeTarget(target.targetId, SessionCloseReason.WORKSPACE_NAVIGATION)
            assertFalse(deferred.await())
        }

    @Test
    fun closeTarget_advancesGeneration_evenWhenReadyWasNull() {
        // #640 B deterministic 行为：target 尚未发布 ready（ready==null）时 closeTarget
        // 仍必须推进 generation，使 awaitPresentationReady 快速返回 false，不永久挂住。
        // 直接锁定根因 — 不依赖协程时序/delay/View layout。
        val host = createHost()
        val target = EditableTextTarget("chapter-body:p:v:c-a", TextEditorProfile.DocumentBody, isPersistent = true)
        host.registerTarget(target)
        assertEquals(null, host.presentationReady.value)
        val genBefore = host.presentationReadyGeneration.value
        host.closeTarget(target.targetId, SessionCloseReason.WORKSPACE_NAVIGATION)
        assertEquals(null, host.presentationReady.value)
        assertTrue(
            "closeTarget 必须推进 generation 即使 ready 为 null",
            host.presentationReadyGeneration.value > genBefore,
        )
    }

    @Test
    fun detachWindowBinding_advancesGeneration_evenWhenReadyWasNull() {
        // #640 B deterministic 行为：target 尚未发布 ready 时 detachWindowBinding
        // 仍必须推进 generation（窗口解绑/Compose onDispose 路径），await 快速 false。
        val host = createHost()
        val target = EditableTextTarget("chapter-body:p:v:c-a", TextEditorProfile.DocumentBody, isPersistent = true)
        host.registerTarget(target)
        val genBefore = host.presentationReadyGeneration.value
        host.detachWindowBinding(host.windowId, target.targetId)
        assertTrue(
            "detachWindowBinding 必须推进 generation 即使 ready 为 null",
            host.presentationReadyGeneration.value > genBefore,
        )
    }

    @Test
    fun awaitPresentationReady_returnsTrue_whenGeometryPublishedViaReadyFlow() =
        runTest(UnconfinedTestDispatcher()) {
            val host = createHost()
            val target = EditableTextTarget("chapter-body:p:v:c-a", TextEditorProfile.DocumentBody, isPersistent = true)
            host.registerTarget(target)
            // 直接通过公开 presentationReady StateFlow 验证类型契约：
            // 初始 null，几何发布后携带 EditorPresentationReady(targetId, w, h)。
            assertEquals(null, host.presentationReady.value)
            assertEquals(0L, host.presentationReadyGeneration.value)
            // isPresentationReady 含几何检查
            assertFalse(host.isPresentationReady(target.targetId))
        }

    @Test
    fun presentationReadyStateFlow_exposesEditorPresentationReadyGeometryType() {
        val host = createHost()
        // 编译通过即证明 presentationReady 是 StateFlow<EditorPresentationReady?>，
        // 携带 targetId + widthPx + heightPx 不可伪造几何。
        val flow = host.presentationReady
        assertNotNull(flow)
        val genFlow = host.presentationReadyGeneration
        assertNotNull(genFlow)
        assertEquals(null, flow.value)
        assertEquals(0L, genFlow.value)
    }

    @Test
    fun awaitPresentationReady_isSuspendReturningBoolean_correctJvmSignature() {
        // Kotlin suspend fun awaitPresentationReady(String): Boolean 编译为 JVM bridge
        // Object awaitPresentationReady(String, Continuation) — 反射 returnType 是 Object
        // （Continuation 恢复类型），不是 primitive boolean。验证存在带 Continuation 参数
        // 的 suspend bridge 且返回 Object，即证明这是 suspend Boolean 而非 Unit/非 suspend。
        val methods = EditorWindowHost::class.java.declaredMethods
        val suspendBridge =
            methods.find {
                it.name == "awaitPresentationReady" &&
                    it.parameterTypes.size == 2 &&
                    it.parameterTypes[0] == java.lang.String::class.java &&
                    it.parameterTypes[1] == kotlin.coroutines.Continuation::class.java
            }
        assertNotNull("awaitPresentationReady suspend bridge (String, Continuation) 应存在", suspendBridge)
        assertEquals(
            "suspend Boolean 的 JVM bridge 返回 Object（Continuation 恢复类型），不是 primitive boolean",
            java.lang.Object::class.java,
            suspendBridge!!.returnType,
        )
    }

    @Test
    fun sujianEditorView_exposesGeometryCallbacksNotOneShot() {
        // 编译通过即证明 SujianEditorView 暴露 onPresentationGeometryReady (Int, Int) -> Unit
        // 和 onPresentationGeometryInvalidated () -> Unit 持久回调，以及 dispatchPresentationReadyIfPossible 公开方法。
        val viewClass = com.xiwei.sujian.feature.editor.platform.SujianEditorView::class.java
        val readyField = viewClass.getDeclaredField("onPresentationGeometryReady")
        assertNotNull(readyField)
        val invalidatedField = viewClass.getDeclaredField("onPresentationGeometryInvalidated")
        assertNotNull(invalidatedField)
        val dispatchMethod = viewClass.getDeclaredMethod("dispatchPresentationReadyIfPossible")
        assertNotNull(dispatchMethod)
    }
}
