package com.xiwei.sujian.diagnostics

import java.util.concurrent.ConcurrentLinkedQueue
import java.util.concurrent.atomic.AtomicBoolean

object EditorEventRingBuffer {

    private const val MAX_EVENTS = 200
    private val enabled = AtomicBoolean(false)
    private val events = ConcurrentLinkedQueue<Map<String, Any?>>()

    fun setEnabled(isEnabled: Boolean) {
        enabled.set(isEnabled)
        if (!isEnabled) events.clear()
    }

    fun isEnabled(): Boolean = enabled.get()

    fun record(event: Map<String, Any?>) {
        if (!enabled.get()) return
        val redacted = event.toMutableMap()
        for (key in listOf("text", "content", "body", "chapter")) {
            redacted.remove(key)
        }
        events.add(redacted)
        while (events.size > MAX_EVENTS) {
            events.poll()
        }
    }

    fun getSnapshot(): List<Map<String, Any?>> {
        return events.toList()
    }

    fun clear() {
        events.clear()
    }
}
