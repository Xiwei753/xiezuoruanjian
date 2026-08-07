package com.xiwei.sujian.diagnostics

import java.util.concurrent.ConcurrentLinkedQueue
import java.util.concurrent.atomic.AtomicBoolean

object EditorEventRingBuffer {
    private const val MAX_EVENTS = 1000
    private val enabled = AtomicBoolean(false)
    private val events = ConcurrentLinkedQueue<Map<String, Any?>>()

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
