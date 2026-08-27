package com.xiwei.sujian.feature.editor.window

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #640 B / #640 评论 5441010318 项2：PresentationReadinessGate 状态管理回归测试。
 *
 * #640 评论 5441010318 项2：await 逻辑已上移到 EditorWindowHost.awaitPresentationReady
 * （combine(presentationReady, sessionStateFlow)），本类只负责 ready 状态管理。
 * 本测试只覆盖 publish/invalidate/isReady/generation 的纯状态行为；await 行为
 * 由 EditorPresentationHostTest（feature/editor/ui）针对 EditorWindowHost 覆盖。
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
    fun invalidateGeometry_clearsReadyButKeepsGeneration() {
        // #640 评论 5441010318 项2：尺寸变化先使旧几何失效，代次不变（同一 target 仍在等待新几何）。
        // await 行为由 EditorPresentationHostTest 覆盖；此处只锁定 gate 状态语义。
        val gate = PresentationReadinessGate()
        gate.publishReady("a", 1080, 2000)
        assertTrue(gate.isReady("a"))
        val genBefore = gate.generation.value
        gate.invalidateGeometry()
        assertNull(gate.ready.value)
        assertFalse(gate.isReady("a"))
        assertEquals(genBefore, gate.generation.value)
        // 新几何发布后 ready 命中
        gate.publishReady("a", 720, 1280)
        assertTrue(gate.isReady("a"))
        val ready = gate.ready.value
        assertNotNull(ready)
        assertEquals(720, ready!!.widthPx)
        assertEquals(1280, ready.heightPx)
    }

    @Test
    fun invalidateAndAdvance_clearsReadyAndAdvancesGeneration() {
        val gate = PresentationReadinessGate()
        gate.publishReady("a", 1080, 2000)
        val genBefore = gate.generation.value
        gate.invalidateAndAdvance()
        assertNull(gate.ready.value)
        assertFalse(gate.isReady("a"))
        assertEquals(genBefore + 1L, gate.generation.value)
    }

    @Test
    fun invalidateTarget_onlyClearsReadyForMatchingTarget_andAdvancesGeneration() {
        val gate = PresentationReadinessGate()
        gate.publishReady("a", 1080, 2000)
        val genBefore = gate.generation.value
        // 关闭 target B — 不应清掉 A 的 ready，但代次仍推进
        gate.invalidateTarget("b")
        assertNotNull(gate.ready.value)
        assertTrue(gate.isReady("a"))
        assertEquals(genBefore + 1L, gate.generation.value)
        // 关闭 target A — 清掉 A 的 ready 并推进代次
        gate.invalidateTarget("a")
        assertNull(gate.ready.value)
        assertFalse(gate.isReady("a"))
        assertEquals(genBefore + 2L, gate.generation.value)
    }
}
