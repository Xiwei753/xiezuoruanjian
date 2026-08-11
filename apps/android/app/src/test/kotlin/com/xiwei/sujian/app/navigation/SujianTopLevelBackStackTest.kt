package com.xiwei.sujian.app.navigation

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #614 评论一：SujianTopLevelBackStack 持有并暴露唯一 [androidx.navigation3.runtime.NavBackStack]。
 *
 * 验证真正交给 NavDisplay 的栈（[SujianTopLevelBackStack.backStack]），
 * 不再验证旁边 MutableMap。关键：底栏点击只调 addTopLevel，由本类在同一份
 * backStack 上做内容切换，调用方不再 clear/rebuild。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SujianTopLevelBackStackTest {
    @Test
    fun init_backStackContentEqualsInitialStack() {
        val stack =
            SujianTopLevelBackStack(
                initialTopLevel = SujianDestination.Works,
                initialStack = listOf(SujianRoute.Works, SujianRoute.Settings),
            )
        assertEquals(
            listOf<SujianRoute>(SujianRoute.Works, SujianRoute.Settings),
            stack.backStack.toList(),
        )
    }

    @Test
    fun addTopLevel_switchesBackStackToTargetRootRoute() {
        val stack = SujianTopLevelBackStack(initialTopLevel = SujianDestination.Works)

        stack.addTopLevel(SujianDestination.StarMap)
        assertEquals(listOf<SujianRoute>(SujianRoute.StarMap), stack.backStack.toList())

        stack.addTopLevel(SujianDestination.Stats)
        assertEquals(listOf<SujianRoute>(SujianRoute.Stats), stack.backStack.toList())
    }

    @Test
    fun addTopLevel_sameDestination_isNoOp_referenceAndContentUnchanged() {
        val stack = SujianTopLevelBackStack(initialTopLevel = SujianDestination.Works)
        val refBefore = stack.backStack

        stack.addTopLevel(SujianDestination.Works)

        assertSame("backStack 引用必须不变", refBefore, stack.backStack)
        assertEquals(listOf<SujianRoute>(SujianRoute.Works), stack.backStack.toList())
    }

    @Test
    fun addTopLevel_preservesStackAcrossSwitch_pushSettingsThenSwitchAndBack() {
        val stack = SujianTopLevelBackStack(initialTopLevel = SujianDestination.Works)
        stack.add(SujianRoute.Settings) // [Works, Settings]

        stack.addTopLevel(SujianDestination.StarMap)
        stack.addTopLevel(SujianDestination.Works)

        assertEquals(
            "切走再切回必须恢复 [Works, Settings]",
            listOf<SujianRoute>(SujianRoute.Works, SujianRoute.Settings),
            stack.backStack.toList(),
        )
    }

    @Test
    fun addTopLevel_doesNotFlattenStack_sizeAndFirstElementStable() {
        val stack = SujianTopLevelBackStack(initialTopLevel = SujianDestination.Works)
        stack.add(SujianRoute.Settings) // [Works, Settings]
        val sizeBeforeSwitch = stack.backStack.size

        stack.addTopLevel(SujianDestination.StarMap)
        stack.addTopLevel(SujianDestination.Works)

        val restored = stack.backStack.toList()
        assertEquals("栈大小必须保留，不得被 push/pop 扁平化", sizeBeforeSwitch, restored.size)
        assertEquals(2, restored.size)
        assertSame("根 route 引用必须保持稳定", SujianRoute.Works, restored.first())
    }

    @Test
    fun backStack_referenceStableAcrossTopLevelSwitches() {
        val stack = SujianTopLevelBackStack(initialTopLevel = SujianDestination.Works)
        val ref = stack.backStack

        stack.addTopLevel(SujianDestination.StarMap)
        assertSame(ref, stack.backStack)
        stack.addTopLevel(SujianDestination.Works)
        assertSame(ref, stack.backStack)
        stack.addTopLevel(SujianDestination.Stats)
        assertSame(ref, stack.backStack)
    }

    @Test
    fun removeLastOrNull_popsWhenSizeGreaterThanOne_returnsFalseWhenSingle() {
        val stack = SujianTopLevelBackStack(initialTopLevel = SujianDestination.Works)
        stack.add(SujianRoute.Settings) // [Works, Settings]

        val popped = stack.removeLastOrNull()
        assertTrue(popped)
        assertEquals(listOf<SujianRoute>(SujianRoute.Works), stack.backStack.toList())

        val poppedAgain = stack.removeLastOrNull()
        assertFalse("size==1 时不再弹出", poppedAgain)
        assertEquals(listOf<SujianRoute>(SujianRoute.Works), stack.backStack.toList())
    }

    @Test
    fun resetCurrentToRoot_resetsBackStackToSingleRoot() {
        val stack = SujianTopLevelBackStack(initialTopLevel = SujianDestination.Works)
        stack.add(SujianRoute.Settings) // [Works, Settings]

        stack.resetCurrentToRoot()

        assertEquals(listOf<SujianRoute>(SujianRoute.Works), stack.backStack.toList())
    }
}
