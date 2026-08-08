package com.xiwei.sujian.app

import android.content.Context
import androidx.compose.runtime.compositionLocalOf
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.app.BridgeProvider
import com.xiwei.sujian.core.interop.project.ProjectRepository
import com.xiwei.sujian.core.interop.settings.SettingsRepository
import com.xiwei.sujian.core.interop.sync.AppSyncDataBarrier
import com.xiwei.sujian.core.interop.sync.SyncCoordinator
import com.xiwei.sujian.core.interop.sync.SyncStatusRepository

interface AppServiceContainer {
    val appServiceBridge: AppServiceBridge
    val projectRepository: ProjectRepository
    val settingsRepository: SettingsRepository
    val syncStatusRepository: SyncStatusRepository
    val syncCoordinator: SyncCoordinator
    val starmapRepository: com.xiwei.sujian.core.interop.starmap.StarMapRepository
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
    override val starmapRepository: com.xiwei.sujian.core.interop.starmap.StarMapRepository =
        BridgeProvider.getStarmapBridge(appContext).repository
    private val appSyncDataBarrier: AppSyncDataBarrier =
        AppSyncDataBarrier(
            starmapBridge = BridgeProvider.getStarmapBridge(appContext),
            reloadSettings = { settingsRepository.notifySyncableSettingsChangedExternally() },
            reloadThemes = { settingsRepository.notifyPaletteCatalogChangedExternally() },
            invalidateStarmapCache = { starmapRepository.invalidateCache() },
        )
    override val syncCoordinator: SyncCoordinator =
        SyncCoordinator(settingsRepository, syncStatusRepository, appSyncDataBarrier)
}

class DefaultSujianAppDependencies(
    private val appContainer: AppServiceContainer,
) : SujianAppDependencies, AppServiceContainer by appContainer
