package com.xiwei.sujian.feature.settings.data
import com.xiwei.sujian.core.interop.common.ResultEnvelope
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.asSharedFlow

object CoreSettingsEvents {
    private const val SETTINGS_SAVED = "SettingsSaved"

    private val _settingsChanged = MutableSharedFlow<Unit>(extraBufferCapacity = 1)
    val settingsChanged: SharedFlow<Unit> = _settingsChanged.asSharedFlow()

    private val _editorSettingsChanged = MutableSharedFlow<Unit>(extraBufferCapacity = 1)
    val editorSettingsChanged: SharedFlow<Unit> = _editorSettingsChanged.asSharedFlow()

    fun record(envelope: ResultEnvelope<*>) {
        if (envelope.changedEntities.none { it.entityType == SETTINGS_SAVED }) return
        _settingsChanged.tryEmit(Unit)
        _editorSettingsChanged.tryEmit(Unit)
    }

    fun markEditorChanged() {
        _settingsChanged.tryEmit(Unit)
        _editorSettingsChanged.tryEmit(Unit)
    }
}
