package com.xiwei.sujian.runtime

import com.xiwei.sujian.SujianApp
import org.junit.Assert.assertSame
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

/**
 * #592 一：Compose UI 入口必须使用 Application 进程级容器，
 * 不得 DefaultAppServiceContainer(context) 创建第二份容器。
 *
 * 后台 Worker 也从同一容器取依赖，保证 SyncStatusRepository StateFlow
 * 和 SyncCoordinator 全进程唯一。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeDepsSingletonTest {

    private fun app(): SujianApp = RuntimeEnvironment.getApplication() as SujianApp

    @Test
    fun composeEntry_usesAppContainer_notCreatesSecond() {
        // Compose 入口通过 requireNotNull(app).dependencies 取得依赖，
        // 等价于 app.dependencies，后者委托给 appContainer。
        val app = app()
        val depsFromApp = app.dependencies
        val containerFromApp = app.appContainer
        // 依赖实例必须与容器中的实例相同
        assertSame(containerFromApp.syncStatusRepository, depsFromApp.syncStatusRepository)
        assertSame(containerFromApp.syncCoordinator, depsFromApp.syncCoordinator)
        assertSame(containerFromApp.settingsRepository, depsFromApp.settingsRepository)
        assertSame(containerFromApp.workspaceRepository, depsFromApp.workspaceRepository)
    }

    @Test
    fun noSecondContainer_createdForSameApp() {
        // 多次访问 app.dependencies 必须返回同一容器中的同一实例
        val app = app()
        val first = app.dependencies
        val second = app.dependencies
        assertSame(first.syncStatusRepository, second.syncStatusRepository)
        assertSame(first.syncCoordinator, second.syncCoordinator)
    }
}
