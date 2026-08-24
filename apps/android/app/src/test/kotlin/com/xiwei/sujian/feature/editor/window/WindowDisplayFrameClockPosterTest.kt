package com.xiwei.sujian.feature.editor.window

import android.view.Choreographer
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeNoException
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #637 评论 5386301277 项2：[WindowDisplayFrameClock.VsyncChoreographerPoster] 和
 * [WindowDisplayFrameClock.LegacyChoreographerPoster] 的 callback 保存/移除语义。
 *
 * - `VsyncChoreographerPoster`：`postVsyncCallback(callback)` 返回 Unit，公开 API 没有
 *   `VsyncCallbackToken`；`removeVsyncCallback()` 接收原来的 [Choreographer.VsyncCallback]。
 *   本测试验证 callback 本身被保存到内部 map，`removeFramePulse` 能取出并移除。
 * - `LegacyChoreographerPoster`：API 30–32 兼容路径，`postFrameCallback`/
 *   `removeFrameCallback` 由 Robolectric `ShadowLegacyChoreographer` 拦截，可完整验证
 *   callback 保存/移除。
 * - 时间源语义（`preferredFrameTimeline.expectedPresentationTimeNanos` 透传）在
 *   [com.xiwei.sujian.feature.editor.session.SingleFrameDriveTest] 里通过
 *   `FakeFrameCallbackPoster` 验证；本文件覆盖 poster 与真实 Choreographer 的交互。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class WindowDisplayFrameClockPosterTest {
    private val pulseCallback = WindowDisplayFrameClock.FramePulseCallback { /* no-op */ }

    /**
     * VsyncChoreographerPoster.removeFramePulse 对未注册 callback 必须不抛异常
     * （`callbacks.remove(callback) ?: return` 的空 map 分支）。
     */
    @Test
    fun vsyncPoster_removeFramePulse_onUnregisteredCallbackDoesNotThrow() {
        val poster = WindowDisplayFrameClock.VsyncChoreographerPoster(Choreographer.getInstance())
        poster.removeFramePulse(pulseCallback)
    }

    /**
     * VsyncChoreographerPoster.postFramePulse 保存 callback 到内部 map；
     * removeFramePulse 取出并清空。若 Robolectric 不支持 postVsyncCallback 的
     * native 调度，则跳过 postFramePulse 真实调用，仅验证 removeFramePulse 空安全。
     */
    @Test
    fun vsyncPoster_postAndRemove_savesAndRemovesCallback() {
        val poster = WindowDisplayFrameClock.VsyncChoreographerPoster(Choreographer.getInstance())
        val callbacksField =
            WindowDisplayFrameClock.VsyncChoreographerPoster::class.java
                .getDeclaredField("callbacks")
        callbacksField.isAccessible = true

        @Suppress("UNCHECKED_CAST")
        val callbacksMap =
            callbacksField.get(poster) as MutableMap<WindowDisplayFrameClock.FramePulseCallback, Any>

        assertEquals("初始 map 必须为空", 0, callbacksMap.size)

        // 尝试 postFramePulse；Robolectric 的 ShadowChoreographer 不拦截 postVsyncCallback，
        // 若 instrumented 实现需要 native 调度会抛 UnsatisfiedLinkError，此时跳过本测试。
        try {
            poster.postFramePulse(pulseCallback)
        } catch (e: Throwable) {
            assumeNoException("Robolectric 不支持 postVsyncCallback，跳过 post 路径", e)
        }

        assertEquals("postFramePulse 必须保存 callback 到 map", 1, callbacksMap.size)
        val saved = callbacksMap[pulseCallback]
        assertNotNull("保存的 VsyncCallback 不得为 null", saved)

        poster.removeFramePulse(pulseCallback)
        assertEquals("removeFramePulse 必须从 map 移除 callback", 0, callbacksMap.size)
    }

    /**
     * LegacyChoreographerPoster.postFramePulse 保存 FrameCallback；
     * removeFramePulse 取出并移除。Robolectric 拦截 postFrameCallback，可完整验证。
     */
    @Test
    fun legacyPoster_postAndRemove_savesAndRemovesCallback() {
        val poster = WindowDisplayFrameClock.LegacyChoreographerPoster(Choreographer.getInstance())
        val frameCallbacksField =
            WindowDisplayFrameClock.LegacyChoreographerPoster::class.java
                .getDeclaredField("frameCallbacks")
        frameCallbacksField.isAccessible = true

        @Suppress("UNCHECKED_CAST")
        val frameCallbacksMap =
            frameCallbacksField.get(poster) as MutableMap<WindowDisplayFrameClock.FramePulseCallback, Any>

        assertEquals("初始 map 必须为空", 0, frameCallbacksMap.size)

        poster.postFramePulse(pulseCallback)
        assertEquals("postFramePulse 必须保存 FrameCallback 到 map", 1, frameCallbacksMap.size)
        val saved = frameCallbacksMap[pulseCallback]
        assertNotNull("保存的 FrameCallback 不得为 null", saved)

        poster.removeFramePulse(pulseCallback)
        assertEquals("removeFramePulse 必须从 map 移除 FrameCallback", 0, frameCallbacksMap.size)
    }

    /**
     * LegacyChoreographerPoster.removeFramePulse 对未注册 callback 不抛异常。
     */
    @Test
    fun legacyPoster_removeFramePulse_onUnregisteredCallbackDoesNotThrow() {
        val poster = WindowDisplayFrameClock.LegacyChoreographerPoster(Choreographer.getInstance())
        poster.removeFramePulse(pulseCallback)
    }

    /**
     * LegacyChoreographerPoster.doFrame 透传 frameTimeNanos 给 FramePulseCallback
     * （API 30–32 兼容路径没有 preferredFrameTimeline，直接用 doFrame 的时间）。
     */
    @Test
    fun legacyPoster_doFrame_propagatesFrameTimeToPulseCallback() {
        val poster = WindowDisplayFrameClock.LegacyChoreographerPoster(Choreographer.getInstance())
        val frameCallbacksField =
            WindowDisplayFrameClock.LegacyChoreographerPoster::class.java
                .getDeclaredField("frameCallbacks")
        frameCallbacksField.isAccessible = true

        var receivedNanos = -1L
        val cb = WindowDisplayFrameClock.FramePulseCallback { receivedNanos = it }

        poster.postFramePulse(cb)
        @Suppress("UNCHECKED_CAST")
        val frameCallbacksMap = frameCallbacksField.get(poster) as Map<*, *>
        val saved = (frameCallbacksMap[cb] as Choreographer.FrameCallback?)!!

        saved.doFrame(123_456_789L)
        assertEquals(
            "LegacyChoreographerPoster 必须把 doFrame 的 frameTimeNanos 透传给 FramePulseCallback",
            123_456_789L,
            receivedNanos,
        )

        poster.removeFramePulse(cb)
    }

    /**
     * VsyncChoreographerPoster 的 VsyncCallback.onVsync 透传
     * preferredFrameTimeline.expectedPresentationTimeNanos 给 FramePulseCallback，
     * 并在触发后从 map 移除。用反射构造 FrameData 不现实，因此这里通过验证
     * saved callback 的类型是 Choreographer.VsyncCallback 来保证时间源路径正确；
     * 完整的 FrameData 透传由 SingleFrameDriveTest 的 onFramePulse 时间透传断言覆盖。
     */
    @Test
    fun vsyncPoster_savedCallback_isVsyncCallbackType() {
        val poster = WindowDisplayFrameClock.VsyncChoreographerPoster(Choreographer.getInstance())
        val callbacksField =
            WindowDisplayFrameClock.VsyncChoreographerPoster::class.java
                .getDeclaredField("callbacks")
        callbacksField.isAccessible = true

        try {
            poster.postFramePulse(pulseCallback)
        } catch (e: Throwable) {
            assumeNoException("Robolectric 不支持 postVsyncCallback，跳过", e)
        }

        @Suppress("UNCHECKED_CAST")
        val callbacksMap =
            callbacksField.get(poster) as MutableMap<WindowDisplayFrameClock.FramePulseCallback, Any>
        val saved = callbacksMap[pulseCallback]
        assertNotNull(saved)
        assertTrue(
            "保存的 callback 必须是 Choreographer.VsyncCallback 类型",
            saved is Choreographer.VsyncCallback,
        )

        poster.removeFramePulse(pulseCallback)
    }
}
