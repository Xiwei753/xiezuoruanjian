package com.xiwei.sujian.editor.v2.coordinator

import androidx.lifecycle.ViewModel
import com.xiwei.sujian.data.AppServiceBridge

/**
 * #592 一：ViewModel 只持有会话层协调器 [EditorSessionCoordinator]，
 * 不再持有整个 AnimatedTextEditorCoordinator（窗口级对象）。
 * 会话层跨配置变化存活；窗口宿主由 Compose 层按窗口生命周期创建和释放。
 */
class EditorSessionViewModel : ViewModel() {
    var sessionCoordinator: EditorSessionCoordinator? = null
        private set

    fun getOrCreateSessionCoordinator(
        appServiceBridge: AppServiceBridge,
        animationTimeSource: com.xiwei.sujian.editor.v2.visual.AnimationTimeSource,
        transactionIdSource: com.xiwei.sujian.editor.v2.visual.TransactionIdSource,
    ): EditorSessionCoordinator {
        sessionCoordinator?.let { return it }
        val c = EditorSessionCoordinator(
            appServiceBridge,
            animationTimeSource,
            transactionIdSource,
        )
        sessionCoordinator = c
        return c
    }

    override fun onCleared() {
        sessionCoordinator?.releaseHost()
        sessionCoordinator = null
    }
}
