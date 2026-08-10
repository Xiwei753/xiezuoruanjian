package com.xiwei.sujian.app.navigation

import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.navigation3.runtime.NavKey

/**
 * 一级导航 TopLevelBackStack — 每个 top-level destination 有自己的 back stack。
 *
 * Works / StarMap / Stats 一级切换时只切 tab，不做整页 push/pop 横移动画。
 * Settings 仍然是 Works 下的层级页面，不变成第四个底栏 tab。
 *
 * 切换 tab 时由调用方把当前 [NavBackStack] 内容存入 [saveCurrent]，
 * 再用 [addTopLevel] 切换，最后用 [currentBackStack] 取出目标 tab 的栈恢复。
 *
 * 参考：https://developer.android.com/guide/navigation/navigation-3/recipes/multiple-backstacks
 */
@Stable
class SujianTopLevelBackStack(
    initialTopLevel: SujianDestination = SujianDestination.Works,
) {
    private val backStacks: MutableMap<SujianDestination, MutableList<NavKey>> = mutableMapOf()

    var currentTopLevel: SujianDestination by mutableStateOf(initialTopLevel)
        private set

    init {
        SujianDestination.entries.forEach { dest ->
            backStacks[dest] = mutableListOf(dest.toRoute())
        }
    }

    /** 把当前 tab 的导航栈快照保存下来，供切回时恢复。 */
    fun saveCurrent(stack: List<NavKey>) {
        backStacks[currentTopLevel] = stack.toMutableList()
    }

    /** 当前 tab 的导航栈快照。 */
    fun currentBackStack(): List<NavKey> = backStacks[currentTopLevel]?.toList() ?: emptyList()

    /** 切换一级 tab；相同 tab 时无操作。 */
    fun addTopLevel(destination: SujianDestination) {
        if (destination == currentTopLevel) return
        currentTopLevel = destination
    }

    /** 把当前 tab 的栈重置为根 route。 */
    fun resetCurrentToRoot() {
        backStacks[currentTopLevel] = mutableListOf(currentTopLevel.toRoute())
    }
}

@Composable
fun rememberSujianTopLevelBackStack(
    initialTopLevel: SujianDestination = SujianDestination.Works,
): SujianTopLevelBackStack = remember { SujianTopLevelBackStack(initialTopLevel) }
