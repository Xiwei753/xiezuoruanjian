package com.xiwei.sujian.support

import androidx.compose.ui.test.SemanticsNodeInteraction
import androidx.compose.ui.test.junit4.ComposeTestRule
import java.util.concurrent.TimeUnit

object ComposeWait {
    private const val DEFAULT_TIMEOUT_MS = 10_000L

    fun waitForTag(
        rule: ComposeTestRule,
        tag: String,
        timeoutMs: Long = DEFAULT_TIMEOUT_MS
    ): SemanticsNodeInteraction {
        val deadline = System.nanoTime() + TimeUnit.MILLISECONDS.toNanos(timeoutMs)
        var lastException: AssertionError? = null
        while (System.nanoTime() < deadline) {
            try {
                val node = rule.onNode(androidx.compose.ui.test.hasTestTag(tag))
                node.assertExists()
                return node
            } catch (e: AssertionError) {
                lastException = e
                Thread.sleep(100)
            }
        }
        throw lastException ?: AssertionError("Timed out waiting for tag: $tag")
    }

    fun waitUntil(
        rule: ComposeTestRule,
        condition: () -> Boolean,
        timeoutMs: Long = DEFAULT_TIMEOUT_MS
    ) {
        rule.waitUntil(timeoutMs, condition)
    }
}
