package com.xiwei.sujian.app.di

import android.content.Context
import androidx.compose.runtime.compositionLocalOf
import com.xiwei.sujian.app.presentation.screen.PresentationPolicyCatalog
import com.xiwei.sujian.app.theme.ThemeRepository
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.project.data.ChapterRepository
import com.xiwei.sujian.feature.project.data.ProjectRepository
import com.xiwei.sujian.feature.project.data.RecentEditsRepository
import com.xiwei.sujian.feature.settings.data.SettingsRepository
import com.xiwei.sujian.feature.stats.data.WritingStatsRepository
import com.xiwei.sujian.feature.sync.data.AppSyncDataBarrier
import com.xiwei.sujian.feature.sync.data.SyncCoordinator
import com.xiwei.sujian.feature.sync.data.SyncRepository
import com.xiwei.sujian.feature.sync.data.SyncStatusRepository

interface AppServiceContainer {
    val appServiceBridge: AppServiceBridge

    /** 静态页面契约目录（#618 一）：容器创建时一次性解析，Compose 热路径只查内存 Map。 */
    val presentationPolicyCatalog: PresentationPolicyCatalog
    val projectRepository: ProjectRepository
    val chapterRepository: ChapterRepository
    val recentEditsRepository: RecentEditsRepository
    val statsRepository: WritingStatsRepository
    val settingsRepository: SettingsRepository
    val themeRepository: ThemeRepository
    val syncRepository: SyncRepository
    val syncStatusRepository: SyncStatusRepository
    val syncCoordinator: SyncCoordinator
    val starmapRepository: com.xiwei.sujian.feature.starmap.data.StarMapRepository
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

    // #624 评论11 第3项：stats writer actor 的进程级 scope（SupervisorJob + Dispatchers.IO）—
    // 与 WritingStatsRepository 同生命周期；不放进 EditorViewModel，否则 Activity/ViewModel
    // 重建会切断统计写队列。
    private val statsWriterScope: kotlinx.coroutines.CoroutineScope =
        kotlinx.coroutines.CoroutineScope(kotlinx.coroutines.SupervisorJob() + kotlinx.coroutines.Dispatchers.IO)

    // #618 一：桥创建后立即解析静态页面契约（resolver 在 DI 层提供 Bridge 调用，
    // presentation 层不直接依赖 Bridge），之后 Compose 只查 Map，
    // 不再在页面组合过程中临时跨 UniFFI 取契约。
    override val presentationPolicyCatalog: PresentationPolicyCatalog =
        PresentationPolicyCatalog(
            resolver = { role ->
                when (val result = appServiceBridge.resolveScreenPolicy(role)) {
                    is BridgeResult.Success -> result.data
                    else -> null
                }
            },
        )
    override val projectRepository: ProjectRepository = ProjectRepository(appContext, appServiceBridge)
    override val chapterRepository: ChapterRepository = ChapterRepository(appContext, appServiceBridge)
    override val recentEditsRepository: RecentEditsRepository = RecentEditsRepository(appContext, appServiceBridge)
    override val statsRepository: WritingStatsRepository =
        WritingStatsRepository(appServiceBridge.statsBridge, statsWriterScope)
    override val settingsRepository: SettingsRepository = SettingsRepository(appContext, appServiceBridge)
    override val themeRepository: ThemeRepository = ThemeRepository(appContext, appServiceBridge)
    override val syncRepository: SyncRepository = SyncRepository(appContext, appServiceBridge)
    override val syncStatusRepository: SyncStatusRepository = SyncStatusRepository(syncRepository)
    override val starmapRepository: com.xiwei.sujian.feature.starmap.data.StarMapRepository =
        appServiceBridge.starMapBridge.repository
    private val appSyncDataBarrier: AppSyncDataBarrier =
        AppSyncDataBarrier(
            starmapBridge = appServiceBridge.starMapBridge,
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
