package com.xiwei.sujian.feature.editor.diagnostics

import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.atomic.AtomicBoolean

object EditorEventRingBuffer {
    private const val MAX_EVENTS = 1000
    private val enabled = AtomicBoolean(false)
    private val events = ArrayBlockingQueue<Map<String, Any?>>(MAX_EVENTS)

    private val SENSITIVE_KEYS =
        setOf(
            "text", "content", "body", "chapter",
            "chapter_content", "chapterContent",
            "password", "passwd", "secret", "token",
            "access_token", "refresh_token", "authorization",
            "private_key", "ssh_private_key",
        )

    fun setEnabled(isEnabled: Boolean) {
        enabled.set(isEnabled)
        if (!isEnabled) events.clear()
    }

    fun isEnabled(): Boolean = enabled.get()

    fun record(event: Map<String, Any?>) {
        if (!enabled.get()) return
        val redacted = event.toMutableMap()
        for (key in SENSITIVE_KEYS) {
            redacted.remove(key)
        }
        if (!events.offer(redacted)) {
            events.poll()
            events.offer(redacted)
        }
    }

    fun getSnapshot(): List<Map<String, Any?>> {
        return events.toList()
    }

    fun clear() {
        events.clear()
    }
}
