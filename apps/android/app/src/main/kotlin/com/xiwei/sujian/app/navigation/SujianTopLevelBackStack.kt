package com.xiwei.sujian.app.navigation

import androidx.compose.runtime.Composable
import androidx.compose.runtime.MutableState
import androidx.compose.runtime.Stable
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.saveable.rememberSaveableStateHolder
import androidx.lifecycle.viewmodel.navigation3.rememberViewModelStoreNavEntryDecorator
import androidx.navigation3.runtime.NavBackStack
import androidx.navigation3.runtime.NavEntry
import androidx.navigation3.runtime.NavEntryDecorator
import androidx.navigation3.runtime.NavKey
import androidx.navigation3.runtime.rememberDecoratedNavEntries
import androidx.navigation3.runtime.rememberNavBackStack
import androidx.navigation3.runtime.rememberSaveableStateHolderNavEntryDecorator

/**
 * 一级导航 TopLevelBackStack — Navigation 3 多栈可恢复模型。
 *
 * 每个 top-level destination（Works/StarMap/Stats）持有自己的 [NavBackStack]，
 * 由 [rememberNavBackStack] 跨配置变化/进程恢复保存导航状态。
 * [currentTopLevel] 由 [rememberSaveable] 保存。
 * [backStack] 派生为当前 top-level 的栈引用；切换 tab 只切引用，不 clear/rebuild。
 * Settings 仍是 Works 子栈，通过 [add] 进入。
 *
 * 参考：https://developer.android.com/guide/navigation/navigation-3/recipes/multiple-backstacks
 */
@Stable
class SujianTopLevelBackStack(
    val worksStack: NavBackStack<NavKey>,
    val starMapStack: NavBackStack<NavKey>,
    val statsStack: NavBackStack<NavKey>,
    private val currentTopLevelState: MutableState<SujianDestination>,
) {
    var currentTopLevel: SujianDestination
        get() = currentTopLevelState.value
        private set(value) {
            currentTopLevelState.value = value
        }

    private val stacks: Map<SujianDestination, NavBackStack<NavKey>> =
        mapOf(
            SujianDestination.Works to worksStack,
            SujianDestination.StarMap to starMapStack,
            SujianDestination.Stats to statsStack,
        )

    /** 交给 NavDisplay 的当前活跃栈。切 tab 只切引用，不 clear/rebuild。 */
    val backStack: NavBackStack<NavKey> get() = stacks.getValue(currentTopLevel)

    /** 切换一级 tab；相同 tab 时无操作。 */
    fun addTopLevel(destination: SujianDestination) {
        if (destination == currentTopLevel) return
        currentTopLevel = destination
    }

    /** 向当前 tab 栈 push（如进入 Settings，只应在 Works 时调用）。 */
    fun add(key: NavKey) {
        backStack.add(key)
    }

    /** 弹出当前栈末尾；返回是否弹出。 */
    fun removeLastOrNull(): Boolean {
        val s = backStack
        if (s.size <= 1) return false
        s.removeAt(s.size - 1)
        return true
    }

    /** 把当前 tab 栈重置为根 route。 */
    fun resetCurrentToRoot() {
        val s = backStack
        s.clear()
        s.add(currentTopLevel.toRoute())
    }

    /**
     * #614 评论三：每个 top-level 栈绑定自己的 decorated entries。
     *
     * 三个 [rememberDecoratedNavEntries] 始终处于组合中（无条件调用），
     * 每个 top-level 有独立的 [rememberSaveableStateHolderNavEntryDecorator] +
     * [rememberViewModelStoreNavEntryDecorator]。
     * inactive tab 的 decorator 和状态仍活着；真正 pop 该 tab 内的 route 时才清掉。
     * 切 tab 不会把 inactive tab 的 saveable/viewmodel 状态误清。
     */
    @Composable
    fun decoratedEntries(entryProvider: (NavKey) -> NavEntry<NavKey>): List<NavEntry<NavKey>> {
        val worksEntries =
            rememberDecoratedNavEntries(
                backStack = worksStack,
                entryDecorators = rememberPerStackDecorators(),
                entryProvider = entryProvider,
            )
        val starMapEntries =
            rememberDecoratedNavEntries(
                backStack = starMapStack,
                entryDecorators = rememberPerStackDecorators(),
                entryProvider = entryProvider,
            )
        val statsEntries =
            rememberDecoratedNavEntries(
                backStack = statsStack,
                entryDecorators = rememberPerStackDecorators(),
                entryProvider = entryProvider,
            )
        return when (currentTopLevel) {
            SujianDestination.Works -> worksEntries
            SujianDestination.StarMap -> starMapEntries
            SujianDestination.Stats -> statsEntries
        }
    }
}

/**
 * #614 评论三：每个 top-level 栈独立的 decorators。
 * SaveableStateHolder 在前，ViewModelStore 在后。
 * 三个调用点各自有独立的 remember slot，互不影响。
 */
@Composable
private fun rememberPerStackDecorators(): List<NavEntryDecorator<NavKey>> {
    val saveableStateHolder = rememberSaveableStateHolder()
    val saveableDecorator = rememberSaveableStateHolderNavEntryDecorator<NavKey>(saveableStateHolder)
    val viewModelDecorator = rememberViewModelStoreNavEntryDecorator<NavKey>()
    return listOf(saveableDecorator, viewModelDecorator)
}

/**
 * 构造可跨配置变化/进程恢复的 [SujianTopLevelBackStack]。
 *
 * - 每个 top-level 栈用 [rememberNavBackStack] 单独保存；
 * - [currentTopLevel] 用 [rememberSaveable] 保存（enum 默认 Serializable，走 autoSaver）；
 * - [initialStack] 只在对应 top-level 上生效，其余 top-level 用各自根 route 初始化。
 */
@Composable
fun rememberSujianTopLevelBackStack(
    initialTopLevel: SujianDestination = SujianDestination.Works,
    initialStack: List<SujianRoute> = listOf(SujianDestination.Works.toRoute()),
): SujianTopLevelBackStack {
    val worksInitial: List<SujianRoute> =
        if (initialTopLevel == SujianDestination.Works) initialStack else listOf(SujianRoute.Works)
    val starMapInitial: List<SujianRoute> =
        if (initialTopLevel == SujianDestination.StarMap) initialStack else listOf(SujianRoute.StarMap)
    val statsInitial: List<SujianRoute> =
        if (initialTopLevel == SujianDestination.Stats) initialStack else listOf(SujianRoute.Stats)
    val worksStack = rememberNavBackStack(*worksInitial.toTypedArray())
    val starMapStack = rememberNavBackStack(*starMapInitial.toTypedArray())
    val statsStack = rememberNavBackStack(*statsInitial.toTypedArray())
    val currentTopLevel = rememberSaveable(initialTopLevel) { mutableStateOf(initialTopLevel) }
    return remember(worksStack, starMapStack, statsStack, currentTopLevel) {
        SujianTopLevelBackStack(worksStack, starMapStack, statsStack, currentTopLevel)
    }
}
