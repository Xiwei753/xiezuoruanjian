package com.xiwei.sujian.support

import androidx.compose.ui.test.SemanticsNodeInteraction
import androidx.compose.ui.test.junit4.ComposeTestRule

object ComposeWait {
    private const val DEFAULT_TIMEOUT_MS = 10_000L

    fun waitForTag(
        rule: ComposeTestRule,
        tag: String,
        timeoutMs: Long = DEFAULT_TIMEOUT_MS,
    ): SemanticsNodeInteraction {
        var lastObserved: String? = null
        waitUntil(rule, {
            val nodes = rule.onAllNodes(androidx.compose.ui.test.hasTestTag(tag))
            val fetched = nodes.fetchSemanticsNodes()
            lastObserved = if (fetched.isEmpty()) "no nodes" else "${fetched.size} node(s)"
            fetched.isNotEmpty()
        }, timeoutMs, diagnostic = {
            "Timed out waiting for tag: $tag (last observed: $lastObserved)"
        })
        return rule.onNode(androidx.compose.ui.test.hasTestTag(tag))
    }

    fun waitForSaveStatus(
        rule: ComposeTestRule,
        expectedState: String,
        timeoutMs: Long = 15_000L,
    ) {
        var lastObservedState: String? = null
        waitUntil(rule, {
            val nodes =
                rule.onAllNodes(
                    androidx.compose.ui.test.hasTestTag(
                        com.xiwei.sujian.core.designsystem.testing.SujianSemanticIds.EditorSaveStatus,
                    ),
                )
            val fetched = nodes.fetchSemanticsNodes()
            if (fetched.isEmpty()) {
                lastObservedState = null
                false
            } else {
                val stateDesc =
                    fetched.first().config.getOrElse(
                        androidx.compose.ui.semantics.SemanticsProperties.StateDescription,
                    ) { "" }
                lastObservedState = stateDesc
                stateDesc == expectedState
            }
        }, timeoutMs, diagnostic = {
            "Expected save status '$expectedState' but last observed was '$lastObservedState'"
        })
    }

    fun waitForEspressoViewCondition(
        rule: ComposeTestRule,
        viewAssertion: androidx.test.espresso.ViewAssertion,
        timeoutMs: Long = 15_000L,
        message: () -> String,
    ) {
        var lastDiagnostic = "no result yet"
        waitUntil(rule, {
            try {
                androidx.test.espresso.Espresso.onView(
                    androidx.test.espresso.matcher.ViewMatchers.withId(
                        com.xiwei.sujian.R.id.editor_content,
                    ),
                ).check(viewAssertion)
                true
            } catch (e: AssertionError) {
                lastDiagnostic = e.message ?: "assertion failed"
                false
            } catch (e: Exception) {
                lastDiagnostic = "${e.javaClass.simpleName}: ${e.message}"
                false
            }
        }, timeoutMs, diagnostic = {
            "${message()}. Last diagnostic: $lastDiagnostic"
        })
    }

    fun waitUntil(
        rule: ComposeTestRule,
        condition: () -> Boolean,
        timeoutMs: Long = DEFAULT_TIMEOUT_MS,
        message: (() -> String)? = null,
        diagnostic: (() -> String)? = null,
    ) {
        var lastDiagnosticValue: String? = null
        try {
            val conditionDescription = if (message != null) message() else "Condition not satisfied"
            rule.waitUntil(conditionDescription, timeoutMs) {
                val result = condition()
                if (!result && diagnostic != null) {
                    lastDiagnosticValue =
                        try {
                            diagnostic()
                        } catch (_: Exception) {
                            "unavailable"
                        }
                }
                result
            }
        } catch (e: androidx.compose.ui.test.ComposeTimeoutException) {
            val diag = if (message != null) message() else "Condition not satisfied"
            val detail = if (lastDiagnosticValue != null) ". Diagnostic: $lastDiagnosticValue" else ""
            throw AssertionError(
                "$diag. Timeout: ${timeoutMs}ms$detail. Original: ${e.message}",
                e,
            )
        } catch (e: AssertionError) {
            if (message != null) {
                val detail = if (lastDiagnosticValue != null) ". Diagnostic: $lastDiagnosticValue" else ""
                throw AssertionError("${message()}$detail. Original: ${e.message}", e)
            }
            throw e
        }
    }
}
