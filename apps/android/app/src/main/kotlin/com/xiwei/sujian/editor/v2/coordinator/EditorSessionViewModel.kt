package com.xiwei.sujian.editor.v2.coordinator

import android.content.Context
import androidx.lifecycle.ViewModel
import com.xiwei.sujian.data.AppServiceBridge

class EditorSessionViewModel : ViewModel() {
    var coordinator: AnimatedTextEditorCoordinator? = null
        private set

    fun getOrCreateCoordinator(
        context: Context,
        appServiceBridge: AppServiceBridge,
        animationTimeSource: com.xiwei.sujian.editor.v2.visual.AnimationTimeSource,
        transactionIdSource: com.xiwei.sujian.editor.v2.visual.TransactionIdSource,
    ): AnimatedTextEditorCoordinator {
        coordinator?.let { return it }
        val c = AnimatedTextEditorCoordinator(
            context.applicationContext,
            appServiceBridge,
            animationTimeSource,
            transactionIdSource,
        )
        coordinator = c
        return c
    }

    override fun onCleared() {
        coordinator?.releaseHost()
        coordinator = null
    }
}
