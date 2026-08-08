package com.xiwei.sujian.feature.editor.session

import androidx.lifecycle.ViewModel
import com.xiwei.sujian.core.interop.app.AppServiceBridge

/**
 * #592 一：ViewModel 只持有会话层协调器 [EditorSessionCoordinator]，
 * 不再持有整个 AnimatedTextEditorCoordinator（窗口级对象）。
 * 会话层跨配置变化存活；窗口宿主由 Compose 层按窗口生命周期创建和释放。
 */
class EditorSessionViewModel : ViewModel() {
    var sessionCoordinator: EditorSessionCoordinator? = null
        private set

    fun getOrCreateSessionCoordinator(appServiceBridge: AppServiceBridge): EditorSessionCoordinator {
        sessionCoordinator?.let { return it }
        val c =
            EditorSessionCoordinator(
                appServiceBridge,
            )
        sessionCoordinator = c
        return c
    }

    override fun onCleared() {
        sessionCoordinator?.releaseHost()
        sessionCoordinator = null
    }
}
