@file:OptIn(kotlinx.coroutines.ExperimentalCoroutinesApi::class)

package com.xiwei.sujian.feature.editor.window

import kotlinx.coroutines.async
import kotlinx.coroutines.delay
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #640 B：PresentationReadinessGate 回归测试 — 纯 Kotlin readiness/await state seam。
 *
 * 覆盖：
 * - 当前几何 ready 命中 → awaitPresentationReady 返回 true；
 * - 尺寸变化使旧几何失效（invalidateGeometry）→ ready=null，同 target await 继续等；
 * - target 替换/关闭（invalidateAndAdvance）→ await 快速返回 false，不永久挂住；
 * - width/height <= 0 不发布 ready；
 * - isReady 含几何检查。
 */
class PresentationReadinessGateTest {
    @Test
    fun isReady_false_whenNothingPublished() {
        val gate = PresentationReadinessGate()
        assertFalse(gate.isReady("a"))
        assertNull(gate.ready.value)
        assertEquals(0L, gate.generation.value)
    }

    @Test
    fun isReady_true_whenGeometryPublishedWithPositiveDimensions() {
        val gate = PresentationReadinessGate()
        gate.publishReady("a", 1080, 2000)
        assertTrue(gate.isReady("a"))
        val ready = gate.ready.value
        assertNotNull(ready)
        assertEquals("a", ready!!.targetId)
        assertEquals(1080, ready.widthPx)
        assertEquals(2000, ready.heightPx)
    }

    @Test
    fun isReady_false_forDifferentTargetId() {
        val gate = PresentationReadinessGate()
        gate.publishReady("a", 1080, 2000)
        assertFalse(gate.isReady("b"))
    }

    @Test
    fun publishReady_ignoredWhenWidthOrHeightNonPositive() {
        val gate = PresentationReadinessGate()
        gate.publishReady("a", 0, 2000)
        assertNull(gate.ready.value)
        gate.publishReady("a", 1080, 0)
        assertNull(gate.ready.value)
        gate.publishReady("a", -1, 2000)
        assertNull(gate.ready.value)
        assertFalse(gate.isReady("a"))
    }

    @Test
    fun awaitPresentationReady_returnsTrue_immediatelyWhenAlreadyReady() = runTest(UnconfinedTestDispatcher()) {
        val gate = PresentationReadinessGate()
        gate.publishReady("a", 1080, 2000)
        assertTrue(gate.awaitPresentationReady("a"))
    }

    @Test
    fun awaitPresentationReady_returnsTrue_whenGeometryPublishedLater() = runTest(UnconfinedTestDispatcher()) {
        val gate = PresentationReadinessGate()
        val deferred = async { gate.awaitPresentationReady("a") }
        delay(10)
        assertFalse(gate.isReady("a"))
        gate.publishReady("a", 1080, 2000)
        assertTrue(deferred.await())
    }

    @Test
    fun awaitPresentationReady_returnsFalse_whenGenerationAdvancesBeforeReady() = runTest(UnconfinedTestDispatcher()) {
        val gate = PresentationReadinessGate()
        val deferred = async { gate.awaitPresentationReady("a") }
        delay(10)
        // 模拟 target 被另一个 bind 替换 → invalidateAndAdvance
        gate.invalidateAndAdvance()
        assertFalse(deferred.await())
        assertEquals(1L, gate.generation.value)
    }

    @Test
    fun invalidateGeometry_clearsReadyButKeepsGeneration_soSameTargetAwaitContinues() = runTest(UnconfinedTestDispatcher()) {
        val gate = PresentationReadinessGate()
        gate.publishReady("a", 1080, 2000)
        assertTrue(gate.isReady("a"))
        val genBefore = gate.generation.value
        // 尺寸变化先使旧几何失效
        gate.invalidateGeometry()
        assertNull(gate.ready.value)
        assertFalse(gate.isReady("a"))
        // 代次不变 — 同一 target 仍在等待新几何
        assertEquals(genBefore, gate.generation.value)
        // 新几何发布后 await 命中
        gate.publishReady("a", 720, 1280)
        assertTrue(gate.awaitPresentationReady("a"))
        val ready = gate.ready.value
        assertNotNull(ready)
        assertEquals(720, ready!!.widthPx)
        assertEquals(1280, ready.heightPx)
    }

    @Test
    fun awaitPresentationReady_forReplacedTarget_returnsFalseWhileNewTargetSucceeds() = runTest(UnconfinedTestDispatcher()) {
        val gate = PresentationReadinessGate()
        // target A 开始等待
        val awaitA = async { gate.awaitPresentationReady("a") }
        delay(10)
        // 用户切到 target B — beginEdit 使 A 失效并推进代次
        gate.invalidateAndAdvance()
        // A 的 await 快速返回 false
        assertFalse(awaitA.await())
        // B 发布 ready 后命中
        gate.publishReady("b", 1080, 2000)
        assertTrue(gate.awaitPresentationReady("b"))
        assertFalse(gate.isReady("a"))
        assertTrue(gate.isReady("b"))
    }

    @Test
    fun invalidateAndAdvance_repeatedInvariantsKeepAwaitFromHanging() = runTest(UnconfinedTestDispatcher()) {
        val gate = PresentationReadinessGate()
        val awaitA = async { gate.awaitPresentationReady("a") }
        delay(10)
        gate.invalidateAndAdvance()
        gate.invalidateAndAdvance()
        gate.invalidateAndAdvance()
        assertFalse(awaitA.await())
        assertEquals(3L, gate.generation.value)
    }

    @Test
    fun awaitPresentationReady_doesNotReturnFalseOnTransientNullWhenGenerationStable() = runTest(UnconfinedTestDispatcher()) {
        val gate = PresentationReadinessGate()
        val awaitA = async { gate.awaitPresentationReady("a") }
        delay(10)
        // 几何失效但代次不变（尺寸变化中间态）— await 不应返回 false，应继续等
        gate.invalidateGeometry()
        delay(10)
        assertFalse(awaitA.isCompleted)
        // 新几何发布后才命中
        gate.publishReady("a", 1080, 2000)
        assertTrue(awaitA.await())
    }
}
