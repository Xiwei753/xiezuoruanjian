package com.xiwei.sujian.app.navigation

import androidx.navigation3.runtime.NavKey
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertSame
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #611 一：SujianTopLevelBackStack 多栈管理正反测试。
 *
 * 每个 top-level destination（Works/StarMap/Stats）持有独立 back stack；
 * 切 tab 只切 currentTopLevel，不做整页 push/pop 横移动画。
 *
 * 正测试验证多栈隔离与恢复；反测试验证切相同 tab 与切 tab 不破坏栈内容。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SujianTopLevelBackStackTest {
    @Test
    fun init_eachDestinationHasOwnBackStack() {
        val backStack = SujianTopLevelBackStack(initialTopLevel = SujianDestination.Works)

        // 初始 currentBackStack 返回 Works 根 route
        assertEquals(listOf<SujianRoute>(SujianRoute.Works), backStack.currentBackStack())

        // 切到 StarMap 后返回 StarMap 根 route（独立栈，不是 Works 栈）
        backStack.addTopLevel(SujianDestination.StarMap)
        assertEquals(listOf<SujianRoute>(SujianRoute.StarMap), backStack.currentBackStack())

        // 切到 Stats 后返回 Stats 根 route
        backStack.addTopLevel(SujianDestination.Stats)
        assertEquals(listOf<SujianRoute>(SujianRoute.Stats), backStack.currentBackStack())
    }

    @Test
    fun addTopLevel_switchesCurrentDestination() {
        val backStack = SujianTopLevelBackStack(initialTopLevel = SujianDestination.Works)

        backStack.addTopLevel(SujianDestination.StarMap)

        assertEquals(SujianDestination.StarMap, backStack.currentTopLevel)
    }

    @Test
    fun addTopLevel_sameDestination_isNoOp() {
        val backStack = SujianTopLevelBackStack(initialTopLevel = SujianDestination.Works)
        // 给 Works 栈写入非根内容，验证切相同 tab 既不改 currentTopLevel 也不清栈
        val worksStack: List<NavKey> = listOf(SujianRoute.Works, SujianRoute.Settings)
        backStack.saveCurrent(worksStack)

        backStack.addTopLevel(SujianDestination.Works)

        assertEquals(SujianDestination.Works, backStack.currentTopLevel)
        assertEquals(worksStack, backStack.currentBackStack())
    }

    @Test
    fun saveCurrent_andRestore_preservesStack() {
        val backStack = SujianTopLevelBackStack(initialTopLevel = SujianDestination.Works)
        val worksStack: List<NavKey> = listOf(SujianRoute.Works, SujianRoute.Settings)
        backStack.saveCurrent(worksStack)

        // 切到其他 tab 再切回，Works 栈必须原样恢复
        backStack.addTopLevel(SujianDestination.StarMap)
        backStack.addTopLevel(SujianDestination.Works)

        assertEquals(worksStack, backStack.currentBackStack())
    }

    @Test
    fun saveCurrent_isolatedBetweenTabs() {
        val backStack = SujianTopLevelBackStack(initialTopLevel = SujianDestination.Works)
        val worksStack: List<NavKey> = listOf(SujianRoute.Works, SujianRoute.Settings)
        backStack.saveCurrent(worksStack)

        backStack.addTopLevel(SujianDestination.StarMap)
        val starMapStack: List<NavKey> = listOf(SujianRoute.StarMap)
        backStack.saveCurrent(starMapStack)

        backStack.addTopLevel(SujianDestination.Stats)
        val statsStack: List<NavKey> = listOf(SujianRoute.Stats)
        backStack.saveCurrent(statsStack)

        // 三个 tab 栈互不影响
        backStack.addTopLevel(SujianDestination.Works)
        assertEquals(worksStack, backStack.currentBackStack())
        backStack.addTopLevel(SujianDestination.StarMap)
        assertEquals(starMapStack, backStack.currentBackStack())
        backStack.addTopLevel(SujianDestination.Stats)
        assertEquals(statsStack, backStack.currentBackStack())
    }

    @Test
    fun resetCurrentToRoot_resetsToSingleRoute() {
        val backStack = SujianTopLevelBackStack(initialTopLevel = SujianDestination.Works)
        backStack.saveCurrent(listOf(SujianRoute.Works, SujianRoute.Settings))

        backStack.resetCurrentToRoot()

        assertEquals(listOf<SujianRoute>(SujianRoute.Works), backStack.currentBackStack())
    }

    @Test
    fun addTopLevel_doesNotDoPagePushPopAnimation() {
        val backStack = SujianTopLevelBackStack(initialTopLevel = SujianDestination.Works)
        // Works 栈有 2 个 entry；切 tab 不应扁平成 1 个 entry（即不做 push/pop）
        val worksStack: List<NavKey> = listOf(SujianRoute.Works, SujianRoute.Settings)
        backStack.saveCurrent(worksStack)
        val sizeBeforeSwitch = backStack.currentBackStack().size

        backStack.addTopLevel(SujianDestination.StarMap)
        backStack.addTopLevel(SujianDestination.Works)

        val restored = backStack.currentBackStack()
        assertEquals("栈内容必须原样保留，不被 push/pop 扁平切换", worksStack, restored)
        assertEquals("栈大小必须保留，不得被 while/remove/add 改写", sizeBeforeSwitch, restored.size)
        assertSame("根 route 引用必须保持稳定", SujianRoute.Works, restored.first())
        // 反向断言：栈确实是非平凡多元素，否则本测试无法证明“未扁平化”
        assertNotEquals(1, restored.size)
    }
}
