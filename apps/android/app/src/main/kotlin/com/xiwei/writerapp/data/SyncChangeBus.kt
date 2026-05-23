package com.xiwei.writerapp.data

import java.util.concurrent.atomic.AtomicBoolean

object SyncChangeBus {
    private val changed = AtomicBoolean(false)

    fun notifyChanged() {
        changed.set(true)
    }

    fun consumeChanged(): Boolean {
        return changed.getAndSet(false)
    }
}
