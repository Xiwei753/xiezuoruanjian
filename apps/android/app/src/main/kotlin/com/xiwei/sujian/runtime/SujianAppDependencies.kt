package com.xiwei.sujian.runtime

import android.content.Context
import androidx.compose.runtime.compositionLocalOf
import com.xiwei.sujian.data.AppServiceBridge
import com.xiwei.sujian.data.BridgeProvider
import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.data.WorkspaceRepository
import com.xiwei.sujian.data.WorkspaceUseCase
import com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator

interface SujianAppDependencies {
    val workspaceRepository: WorkspaceRepository
    val settingsRepository: SettingsRepository
    val appServiceBridge: AppServiceBridge
    val coordinator: AnimatedTextEditorCoordinator

    fun release()

    companion object {
        @Volatile
        private var _testProvider: ((Context) -> SujianAppDependencies)? = null

        fun setTestProvider(provider: ((Context) -> SujianAppDependencies)?) {
            _testProvider = provider
            if (provider == null) {
                BridgeProvider.clearTestMode()
            }
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
    override val coordinator: AnimatedTextEditorCoordinator = AnimatedTextEditorCoordinator(appContext, appServiceBridge)

    override fun release() {
        coordinator.releaseHost()
    }
}
