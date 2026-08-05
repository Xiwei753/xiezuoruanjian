package com.xiwei.sujian.runtime

import com.xiwei.sujian.SujianApp
import org.junit.Assert.assertEquals
import org.junit.Assert.assertSame
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

/**
 * 进程级依赖容器唯一性契约（#592 三）：应用依赖、设置页、AutoSyncWorker
 * 等所有入口必须取得同一 [SujianAppDependencies] 实例，否则会出现两份
 * SyncStatusRepository StateFlow / SyncCoordinator 互相覆盖。
 *
 * 生产实现是 SujianApp 上的线程安全 lazy，本测试钉住该契约：
 * 重复访问与后台线程并发首次访问都必须只构造一个实例。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SujianAppDependenciesSingletonTest {

    private fun app(): SujianApp = RuntimeEnvironment.getApplication() as SujianApp

    @Test
    fun appContainer_repeatedAccess_returnsSameInstance() {
        val container = app().appContainer
        assertSame(container, app().appContainer)
    }

    @Test
    fun appContainer_syncStateFlowAndCoordinator_areUniquePerProcess() {
        val first = app().appContainer
        val second = app().appContainer
        assertSame(first.syncStatusRepository, second.syncStatusRepository)
        assertSame(first.syncCoordinator, second.syncCoordinator)
    }

    @Test
    fun appContainer_concurrentFirstAccess_createsSingleInstance() {
        val app = app()
        val seen = java.util.Collections.synchronizedList(mutableListOf<com.xiwei.sujian.runtime.AppServiceContainer>())
        val threads = (1..8).map {
            Thread { seen.add(app.appContainer) }
        }
        threads.forEach { it.start() }
        threads.forEach { it.join() }
        assertEquals(1, seen.distinct().size)
        assertSame(seen.first(), app.appContainer)
    }

    @Test
    fun dependencies_delegatesToAppContainer() {
        val app = app()
        val deps = app.dependencies
        val container = app.appContainer
        assertSame(container.appServiceBridge, deps.appServiceBridge)
        assertSame(container.syncStatusRepository, deps.syncStatusRepository)
        assertSame(container.syncCoordinator, deps.syncCoordinator)
    }
}
