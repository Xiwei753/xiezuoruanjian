package com.xiwei.sujian.runtime

import android.content.Context
import androidx.compose.runtime.compositionLocalOf
import com.xiwei.sujian.data.AppServiceBridge
import com.xiwei.sujian.data.BridgeProvider
import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.data.SyncCoordinator
import com.xiwei.sujian.data.SyncStatusRepository
import com.xiwei.sujian.data.WorkspaceRepository
import com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator

interface SujianAppDependencies {
    val workspaceRepository: WorkspaceRepository
    val settingsRepository: SettingsRepository
    val appServiceBridge: AppServiceBridge
    val coordinator: AnimatedTextEditorCoordinator
    val syncStatusRepository: SyncStatusRepository
    val syncCoordinator: SyncCoordinator

    fun release()

    companion object {
        @Volatile
        private var _testProvider: ((Context) -> SujianAppDependencies)? = null

        fun setTestProvider(provider: ((Context) -> SujianAppDependencies)?) {
            _testProvider = provider
        }

        fun getTestProvider(): ((Context) -> SujianAppDependencies)? = _testProvider
    }
}

val LocalSujianAppDependencies = compositionLocalOf<SujianAppDependencies> {
    error("No SujianAppDependencies provided. Wrap with CompositionLocalProvider.")
}

class DefaultSujianAppDependencies(context: Context) : SujianAppDependencies {
    private val appContext = context.applicationContext
    override val appServiceBridge: AppServiceBridge = BridgeProvider.getAppServiceBridge(appContext)
    override val workspaceRepository: WorkspaceRepository = WorkspaceRepository(appContext, appServiceBridge)
    override val settingsRepository: SettingsRepository = SettingsRepository(appContext, appServiceBridge)

    /**
     * 编辑器协调器是 UI 侧组件（构造即创建 Choreographer 帧时钟，必须运行在
     * 带 Looper 的主线程）。容器构造可能发生在 WorkManager 后台线程
     * （进程由 Worker 拉起时无任何 UI 组合），因此必须惰性创建：
     * 只有首次 UI 组合访问时才在 UI 线程构造。
     */
    override val coordinator: AnimatedTextEditorCoordinator by lazy {
        AnimatedTextEditorCoordinator(appContext, appServiceBridge)
    }
    override val syncStatusRepository: SyncStatusRepository = SyncStatusRepository(settingsRepository)
    override val syncCoordinator: SyncCoordinator = SyncCoordinator(settingsRepository, syncStatusRepository)

    override fun release() {
        coordinator.releaseHost()
    }
}
