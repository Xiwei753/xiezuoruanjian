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
                rule.mainClock.advanceTimeByFrame()
            }
        }
        throw lastException ?: AssertionError("Timed out waiting for tag: $tag")
    }

    fun waitForSaveStatus(
        rule: ComposeTestRule,
        expectedState: String,
        timeoutMs: Long = 15_000L
    ) {
        var lastObservedState: String? = null
        var lastError: String? = null
        waitUntil(rule, {
            try {
                val node = rule.onNode(androidx.compose.ui.test.hasTestTag(
                    com.xiwei.sujian.designsystem.testing.SujianSemanticIds.EditorSaveStatus
                ))
                node.assertExists()
                val stateDesc = node.fetchSemanticsNode().config[
                    androidx.compose.ui.semantics.SemanticsProperties.StateDescription
                ]
                lastObservedState = stateDesc
                stateDesc == expectedState
            } catch (e: Exception) {
                lastError = "Exception while checking save status: ${e.message}"
                false
            }
        }, timeoutMs) {
            "Expected save status '$expectedState' but last observed was '$lastObservedState'. Last error: $lastError"
        }
    }

    fun waitUntil(
        rule: ComposeTestRule,
        condition: () -> Boolean,
        timeoutMs: Long = DEFAULT_TIMEOUT_MS,
        message: (() -> String)? = null
    ) {
        try {
            rule.waitUntil(timeoutMs, condition)
        } catch (e: AssertionError) {
            if (message != null) {
                throw AssertionError("${message()}. Original: ${e.message}", e)
            }
            throw e
        }
    }
}
