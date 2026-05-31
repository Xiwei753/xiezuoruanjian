package com.xiwei.writerapp.data

import java.util.concurrent.atomic.AtomicBoolean

/**
 * Stores Core-emitted settings events until resumed UI components consume them.
 */
object CoreSettingsEvents {
    private const val SETTINGS_SAVED = "SettingsSaved"

    private val changed = AtomicBoolean(false)
    private val editorChanged = AtomicBoolean(false)

    fun record(envelope: ResultEnvelope<*>) {
        if (envelope.changedEntities.none { it.entityType == SETTINGS_SAVED }) return
        changed.set(true)
        editorChanged.set(true)
    }

    fun consumeChanged(): Boolean {
        return changed.getAndSet(false)
    }

    fun consumeEditorChanged(): Boolean {
        return editorChanged.getAndSet(false)
    }
}
