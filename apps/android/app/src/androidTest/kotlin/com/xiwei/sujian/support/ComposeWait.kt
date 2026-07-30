package com.xiwei.sujian.support

import androidx.compose.ui.test.SemanticsNodeInteraction
import androidx.compose.ui.test.junit4.ComposeTestRule

object ComposeWait {
    private const val DEFAULT_TIMEOUT_MS = 10_000L

    fun waitForTag(
        rule: ComposeTestRule,
        tag: String,
        timeoutMs: Long = DEFAULT_TIMEOUT_MS
    ): SemanticsNodeInteraction {
        var lastNodeCount = 0
        waitUntil(rule, {
            val nodes = rule.onAllNodes(androidx.compose.ui.test.hasTestTag(tag))
            val fetched = nodes.fetchSemanticsNodes()
            lastNodeCount = fetched.size
            fetched.isNotEmpty()
        }, timeoutMs) {
            "Timed out waiting for tag '$tag': last seen $lastNodeCount nodes with that tag"
        }
        return rule.onNode(androidx.compose.ui.test.hasTestTag(tag))
    }

    fun waitForSaveStatus(
        rule: ComposeTestRule,
        expectedState: String,
        timeoutMs: Long = 15_000L
    ) {
        var lastObservedState: String? = null
        var lastNodeCount = 0
        waitUntil(rule, {
            val nodes = rule.onAllNodes(androidx.compose.ui.test.hasTestTag(
                com.xiwei.sujian.designsystem.testing.SujianSemanticIds.EditorSaveStatus
            ))
            val fetched = nodes.fetchSemanticsNodes()
            lastNodeCount = fetched.size
            if (fetched.isEmpty()) {
                lastObservedState = null
                false
            } else {
                val stateDesc = fetched.first().config.getOrElse(
                    androidx.compose.ui.semantics.SemanticsProperties.StateDescription
                ) { "" }
                lastObservedState = stateDesc
                stateDesc == expectedState
            }
        }, timeoutMs) {
            "Expected save status '$expectedState' but last observed was '$lastObservedState' (nodes found: $lastNodeCount)"
        }
    }

    fun waitForEspressoViewCondition(
        rule: ComposeTestRule,
        viewAssertion: androidx.test.espresso.ViewAssertion,
        timeoutMs: Long = 15_000L,
        message: () -> String
    ) {
        var lastDiagnostic = "no result yet"
        waitUntil(rule, {
            try {
                androidx.test.espresso.Espresso.onView(
                    androidx.test.espresso.matcher.ViewMatchers.withId(
                        com.xiwei.sujian.R.id.editor_content
                    )
                ).check(viewAssertion)
                true
            } catch (e: AssertionError) {
                lastDiagnostic = e.message ?: "assertion failed"
                false
            } catch (e: Exception) {
                if (e is InterruptedException || e is java.util.concurrent.CancellationException) {
                    throw e
                }
                lastDiagnostic = "${e.javaClass.simpleName}: ${e.message}"
                false
            }
        }, timeoutMs) {
            "${message()}. Last diagnostic: $lastDiagnostic"
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
        } catch (e: RuntimeException) {
            if (e is InterruptedException || e is java.util.concurrent.CancellationException) {
                throw e
            }
            val detail = if (message != null) "${message()}. " else ""
            throw RuntimeException("${detail}Original: ${e.message}", e)
        }
    }
}
