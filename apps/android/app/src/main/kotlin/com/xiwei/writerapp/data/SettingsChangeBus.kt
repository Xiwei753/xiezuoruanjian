package com.xiwei.writerapp.data

import java.util.concurrent.atomic.AtomicBoolean

object SettingsChangeBus {
    private val changed = AtomicBoolean(false)
    private val editorChanged = AtomicBoolean(false)

    fun notifyChanged() {
        changed.set(true)
        editorChanged.set(true)
    }

    fun markChanged() {
        notifyChanged()
    }

    fun consumeChanged(): Boolean {
        return changed.getAndSet(false)
    }

    fun consumeEditorChanged(): Boolean {
        return editorChanged.getAndSet(false)
    }
}
