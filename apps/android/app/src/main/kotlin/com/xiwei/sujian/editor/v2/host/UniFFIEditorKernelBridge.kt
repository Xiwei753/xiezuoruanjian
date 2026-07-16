package com.xiwei.sujian.editor.v2.host

import com.xiwei.sujian.data.AppServiceBridge
import com.xiwei.sujian.data.BridgeResult
import com.xiwei.sujian.editor.v2.mirror.EditResult
import uniffi.writer_core.EditorEditResultDto

class UniFFIEditorKernelBridge(
    private val appServiceBridge: AppServiceBridge
) : EditorKernelBridge {

    override fun apply(commandJson: String): EditorEditResultDto? {
        return when (val result = appServiceBridge.editorKernelApply(commandJson)) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    override fun loadText(text: String, cursorUtf8: Int): EditorEditResultDto? {
        return when (val result = appServiceBridge.editorKernelLoadText(text, cursorUtf8.toUInt())) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    override fun compositionCommit(
        compositionReplaceStart: Int,
        compositionReplaceEndExclusive: Int,
        committedText: String,
        originalText: String
    ): EditorEditResultDto? {
        return when (val result = appServiceBridge.editorKernelCompositionCommit(
            compositionReplaceStart.toUInt(),
            compositionReplaceEndExclusive.toUInt(),
            committedText,
            originalText
        )) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    override fun setAnimationEnabled(enabled: Boolean) {
        appServiceBridge.editorKernelSetAnimationEnabled(if (enabled) 1u else 0u)
    }

    override fun setAnimationDurationMs(durationMs: Long) {
        appServiceBridge.editorKernelSetAnimationDurationMs(durationMs.toULong())
    }

    override fun compositionUpdateVisualIntent(
        compositionReplaceStart: UInt,
        compositionReplaceEndExclusive: UInt,
        oldPreeditText: String,
        newPreeditText: String
    ): uniffi.writer_core.EditorVisualIntentDto? {
        return try {
            appServiceBridge.editorKernelCompositionUpdateVisualIntent(
                compositionReplaceStart,
                compositionReplaceEndExclusive,
                oldPreeditText,
                newPreeditText,
            )
        } catch (_: Exception) {
            null
        }
    }
}
