package com.xiwei.sujian.app.navigation

import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.navigation3.runtime.NavBackStack
import androidx.navigation3.runtime.NavKey

/**
 * 一级导航 TopLevelBackStack — 持有并暴露唯一 [NavBackStack] 供 NavDisplay 观察。
 *
 * 每个 top-level destination（Works/StarMap/Stats）保留独立栈；底栏点击只调
 * [addTopLevel]，由本类在同一份 [backStack] 上做内容切换，调用方不再 clear/rebuild。
 * Settings 仍是 Works 子栈，通过 [add] 进入。
 *
 * 参考：https://developer.android.com/guide/navigation/navigation-3/recipes/common-ui
 */
@Stable
class SujianTopLevelBackStack(
    initialTopLevel: SujianDestination = SujianDestination.Works,
    initialStack: List<NavKey> = listOf(initialTopLevel.toRoute()),
) {
    private val stacks: MutableMap<SujianDestination, MutableList<NavKey>> = mutableMapOf()

    var currentTopLevel: SujianDestination by mutableStateOf(initialTopLevel)
        private set

    /** 交给 NavDisplay 的唯一可观察栈。切换 tab 时在此对象上做内容切换。 */
    val backStack: NavBackStack<NavKey> = NavBackStack(*initialStack.toTypedArray())

    init {
        stacks[initialTopLevel] = initialStack.toMutableList()
        SujianDestination.entries.forEach { dest ->
            if (dest != initialTopLevel) stacks[dest] = mutableListOf(dest.toRoute())
        }
    }

    /** 切换一级 tab；相同 tab 时无操作（最外层早退）。 */
    fun addTopLevel(destination: SujianDestination) {
        if (destination == currentTopLevel) return
        // 保存当前 tab 栈快照
        stacks[currentTopLevel] = backStack.toList().toMutableList()
        currentTopLevel = destination
        // 在同一份 backStack 上切换内容
        backStack.clear()
        backStack.addAll(stacks[destination] ?: mutableListOf(destination.toRoute()))
    }

    /** 向当前 tab 栈 push（如进入 Settings）。 */
    fun add(key: NavKey) {
        backStack.add(key)
    }

    /** 弹出当前栈末尾；返回是否弹出。 */
    fun removeLastOrNull(): Boolean {
        if (backStack.size <= 1) return false
        backStack.removeAt(backStack.size - 1)
        return true
    }

    /** 把当前 tab 栈重置为根 route。 */
    fun resetCurrentToRoot() {
        stacks[currentTopLevel] = mutableListOf(currentTopLevel.toRoute())
        backStack.clear()
        backStack.add(currentTopLevel.toRoute())
    }
}

@Composable
fun rememberSujianTopLevelBackStack(
    initialTopLevel: SujianDestination = SujianDestination.Works,
    initialStack: List<NavKey> = listOf(SujianDestination.Works.toRoute()),
): SujianTopLevelBackStack = remember { SujianTopLevelBackStack(initialTopLevel, initialStack) }
