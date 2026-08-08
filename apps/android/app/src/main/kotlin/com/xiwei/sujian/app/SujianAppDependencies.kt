package com.xiwei.sujian.app

import android.content.Context
import androidx.compose.runtime.compositionLocalOf
import com.xiwei.sujian.app.di.AppServiceProvider
import com.xiwei.sujian.app.theme.ThemeRepository
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.project.ProjectRepository
import com.xiwei.sujian.core.interop.settings.SettingsRepository
import com.xiwei.sujian.core.interop.stats.StatsRepository
import com.xiwei.sujian.core.interop.sync.AppSyncDataBarrier
import com.xiwei.sujian.core.interop.sync.SyncCoordinator
import com.xiwei.sujian.core.interop.sync.SyncStatusRepository
import com.xiwei.sujian.feature.project.data.ChapterRepository
import com.xiwei.sujian.feature.project.data.RecentEditsRepository
import com.xiwei.sujian.feature.sync.data.SyncRepository

interface AppServiceContainer {
    val appServiceBridge: AppServiceBridge
    val projectRepository: ProjectRepository
    val chapterRepository: ChapterRepository
    val recentEditsRepository: RecentEditsRepository
    val statsRepository: StatsRepository
    val settingsRepository: SettingsRepository
    val themeRepository: ThemeRepository
    val syncRepository: SyncRepository
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
    override val appServiceBridge: AppServiceBridge = AppServiceProvider.getAppServiceBridge(appContext)
    override val projectRepository: ProjectRepository = ProjectRepository(appContext, appServiceBridge)
    override val chapterRepository: ChapterRepository = ChapterRepository(appContext, appServiceBridge)
    override val recentEditsRepository: RecentEditsRepository = RecentEditsRepository(appContext, appServiceBridge)
    override val statsRepository: StatsRepository = StatsRepository(appServiceBridge.statsBridge)
    override val settingsRepository: SettingsRepository = SettingsRepository(appContext, appServiceBridge)
    override val themeRepository: ThemeRepository = ThemeRepository(appContext, appServiceBridge)
    override val syncRepository: SyncRepository = SyncRepository(appContext, appServiceBridge)
    override val syncStatusRepository: SyncStatusRepository = SyncStatusRepository(syncRepository)
    override val starmapRepository: com.xiwei.sujian.core.interop.starmap.StarMapRepository =
        AppServiceProvider.getStarmapBridge(appContext).repository
    private val appSyncDataBarrier: AppSyncDataBarrier =
        AppSyncDataBarrier(
            starmapBridge = AppServiceProvider.getStarmapBridge(appContext),
            reloadSettings = { settingsRepository.notifySyncableSettingsChangedExternally() },
            reloadThemes = { settingsRepository.notifyPaletteCatalogChangedExternally() },
            invalidateStarmapCache = { starmapRepository.invalidateCache() },
        )
    override val syncCoordinator: SyncCoordinator =
        SyncCoordinator(syncRepository, syncStatusRepository, appSyncDataBarrier)
}

class DefaultSujianAppDependencies(
    private val appContainer: AppServiceContainer,
) : SujianAppDependencies, AppServiceContainer by appContainer
