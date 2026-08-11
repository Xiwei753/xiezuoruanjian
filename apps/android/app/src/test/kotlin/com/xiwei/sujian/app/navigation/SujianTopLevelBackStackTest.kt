package com.xiwei.sujian.app.navigation

import androidx.compose.runtime.mutableStateOf
import androidx.navigation3.runtime.NavBackStack
import androidx.navigation3.runtime.NavKey
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #614 评论二：SujianTopLevelBackStack 重构为 Navigation 3 多栈可恢复模型。
 *
 * 每个 top-level 持有独立 [NavBackStack]；[SujianTopLevelBackStack.backStack] 派生为
 * 当前 top-level 的栈引用。切 tab 只切引用，不 clear/rebuild，各栈独立保留。
 *
 * 用 3 个手工 [NavBackStack] + [mutableStateOf] 构造，验证真正交给 NavDisplay 的栈
 * 与各 top-level 独立栈保留。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SujianTopLevelBackStackTest {
    private fun makeStacks(
        initialTopLevel: SujianDestination = SujianDestination.Works,
        worksInitial: List<SujianRoute> = listOf(SujianRoute.Works),
    ): SujianTopLevelBackStack {
        val worksKeys: List<NavKey> = worksInitial
        val works = NavBackStack<NavKey>(*worksKeys.toTypedArray())
        val starMap = NavBackStack<NavKey>(SujianRoute.StarMap)
        val stats = NavBackStack<NavKey>(SujianRoute.Stats)
        return SujianTopLevelBackStack(works, starMap, stats, mutableStateOf(initialTopLevel))
    }

    @Test
    fun init_backStackRefersToWorksStackWithInitialContent() {
        val stack =
            makeStacks(
                initialTopLevel = SujianDestination.Works,
                worksInitial = listOf(SujianRoute.Works, SujianRoute.Settings),
            )
        assertSame("backStack 必须引用 worksStack", stack.worksStack, stack.backStack)
        assertEquals(
            listOf<SujianRoute>(SujianRoute.Works, SujianRoute.Settings),
            stack.backStack.toList(),
        )
    }

    @Test
    fun addTopLevel_switchesBackStackRefToTargetStackWithRootContent() {
        val stack = makeStacks(initialTopLevel = SujianDestination.Works)

        stack.addTopLevel(SujianDestination.StarMap)
        assertSame("切 StarMap 后 backStack 必须引用 starMapStack", stack.starMapStack, stack.backStack)
        assertEquals(listOf<SujianRoute>(SujianRoute.StarMap), stack.backStack.toList())

        stack.addTopLevel(SujianDestination.Stats)
        assertSame("切 Stats 后 backStack 必须引用 statsStack", stack.statsStack, stack.backStack)
        assertEquals(listOf<SujianRoute>(SujianRoute.Stats), stack.backStack.toList())
    }

    @Test
    fun addTopLevel_sameDestination_isNoOp_referenceAndContentUnchanged() {
        val stack = makeStacks(initialTopLevel = SujianDestination.Works)
        val refBefore = stack.backStack

        stack.addTopLevel(SujianDestination.Works)

        assertSame("backStack 引用必须不变", refBefore, stack.backStack)
        assertEquals(listOf<SujianRoute>(SujianRoute.Works), stack.backStack.toList())
    }

    @Test
    fun addTopLevel_preservesEachStackAcrossSwitch_pushSettingsThenSwitchAndBack() {
        val stack = makeStacks(initialTopLevel = SujianDestination.Works)
        stack.add(SujianRoute.Settings) // worksStack: [Works, Settings]

        stack.addTopLevel(SujianDestination.StarMap)
        stack.addTopLevel(SujianDestination.Works)

        assertSame("切回 Works 后 backStack 必须再次引用 worksStack", stack.worksStack, stack.backStack)
        assertEquals(
            "切走再切回必须恢复 [Works, Settings]，证明各 top-level 栈独立保留",
            listOf<SujianRoute>(SujianRoute.Works, SujianRoute.Settings),
            stack.backStack.toList(),
        )
    }

    @Test
    fun addTopLevel_switchesBackStackRefToTargetStack() {
        val stack = makeStacks(initialTopLevel = SujianDestination.Works)

        stack.addTopLevel(SujianDestination.StarMap)
        assertSame(stack.starMapStack, stack.backStack)

        stack.addTopLevel(SujianDestination.Works)
        assertSame(stack.worksStack, stack.backStack)

        stack.addTopLevel(SujianDestination.Stats)
        assertSame(stack.statsStack, stack.backStack)
    }

    @Test
    fun removeLastOrNull_popsWhenSizeGreaterThanOne_returnsFalseWhenSingle() {
        val stack = makeStacks(initialTopLevel = SujianDestination.Works)
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
        val stack = makeStacks(initialTopLevel = SujianDestination.Works)
        stack.add(SujianRoute.Settings) // [Works, Settings]

        stack.resetCurrentToRoot()

        assertEquals(listOf<SujianRoute>(SujianRoute.Works), stack.backStack.toList())
    }

    @Test
    fun stacks_areIndependent_pushOnWorksDoesNotAffectStarMapStack() {
        val stack = makeStacks(initialTopLevel = SujianDestination.Works)
        stack.add(SujianRoute.Settings) // worksStack: [Works, Settings]

        stack.addTopLevel(SujianDestination.StarMap)

        assertEquals(
            "切到 StarMap 后 starMapStack 内容仍为 [StarMap]，不受 Works 栈影响",
            listOf<SujianRoute>(SujianRoute.StarMap),
            stack.starMapStack.toList(),
        )
        assertEquals(
            "worksStack 内容仍保留 [Works, Settings]",
            listOf<SujianRoute>(SujianRoute.Works, SujianRoute.Settings),
            stack.worksStack.toList(),
        )
    }
}
