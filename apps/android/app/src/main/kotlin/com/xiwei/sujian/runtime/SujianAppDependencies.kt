package com.xiwei.sujian.runtime

import android.content.Context
import androidx.compose.runtime.compositionLocalOf
import com.xiwei.sujian.data.AppServiceBridge
import com.xiwei.sujian.data.AppSyncDataBarrier
import com.xiwei.sujian.data.BridgeProvider
import com.xiwei.sujian.data.ProjectRepository
import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.data.SyncCoordinator
import com.xiwei.sujian.data.SyncStatusRepository

interface AppServiceContainer {
    val appServiceBridge: AppServiceBridge
    val projectRepository: ProjectRepository
    val settingsRepository: SettingsRepository
    val syncStatusRepository: SyncStatusRepository
    val syncCoordinator: SyncCoordinator
    val starmapRepository: com.xiwei.sujian.data.starmap.StarMapRepository
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
    override val projectRepository: ProjectRepository = ProjectRepository(appContext, appServiceBridge)
    override val settingsRepository: SettingsRepository = SettingsRepository(appContext, appServiceBridge)
    override val syncStatusRepository: SyncStatusRepository = SyncStatusRepository(settingsRepository)
    override val starmapRepository: com.xiwei.sujian.data.starmap.StarMapRepository =
        BridgeProvider.getStarmapBridge(appContext).repository
    private val appSyncDataBarrier: AppSyncDataBarrier =
        AppSyncDataBarrier(
            starmapBridge = BridgeProvider.getStarmapBridge(appContext),
            reloadSettings = { },
            reloadThemes = { },
            invalidateStarmapCache = { starmapRepository.invalidateCache() },
        )
    override val syncCoordinator: SyncCoordinator =
        SyncCoordinator(settingsRepository, syncStatusRepository, appSyncDataBarrier)
}

class DefaultSujianAppDependencies(
    private val appContainer: AppServiceContainer,
) : SujianAppDependencies, AppServiceContainer by appContainer
