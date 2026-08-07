package com.xiwei.sujian.runtime

import android.content.Context
import androidx.compose.runtime.compositionLocalOf
import com.xiwei.sujian.data.AppServiceBridge
import com.xiwei.sujian.data.BridgeProvider
import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.data.SyncCoordinator
import com.xiwei.sujian.data.SyncStatusRepository
import com.xiwei.sujian.data.WorkspaceRepository

interface AppServiceContainer {
    val appServiceBridge: AppServiceBridge
    val workspaceRepository: WorkspaceRepository
    val settingsRepository: SettingsRepository
    val syncStatusRepository: SyncStatusRepository
    val syncCoordinator: SyncCoordinator
}

interface SujianAppDependencies : AppServiceContainer {
    companion object {
        @Volatile
        private var _testProvider: ((Context) -> SujianAppDependencies)? = null

        fun setTestProvider(provider: ((Context) -> SujianAppDependencies)?) {
            _testProvider = provider
        }

        fun getTestProvider(): ((Context) -> SujianAppDependencies)? = _testProvider
    }
}

val LocalSujianAppDependencies =
    compositionLocalOf<SujianAppDependencies> {
        error("No SujianAppDependencies provided. Wrap with CompositionLocalProvider.")
    }

class DefaultAppServiceContainer(context: Context) : AppServiceContainer {
    private val appContext = context.applicationContext
    override val appServiceBridge: AppServiceBridge = BridgeProvider.getAppServiceBridge(appContext)
    override val workspaceRepository: WorkspaceRepository = WorkspaceRepository(appContext, appServiceBridge)
    override val settingsRepository: SettingsRepository = SettingsRepository(appContext, appServiceBridge)
    override val syncStatusRepository: SyncStatusRepository = SyncStatusRepository(settingsRepository)
    override val syncCoordinator: SyncCoordinator = SyncCoordinator(settingsRepository, syncStatusRepository)
}

class DefaultSujianAppDependencies(
    private val appContainer: AppServiceContainer,
) : SujianAppDependencies, AppServiceContainer by appContainer
