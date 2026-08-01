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
        var lastObserved: String? = null
        waitUntil(rule, {
            val nodes = rule.onAllNodes(androidx.compose.ui.test.hasTestTag(tag))
            val fetched = nodes.fetchSemanticsNodes()
            lastObserved = if (fetched.isEmpty()) "no nodes" else "${fetched.size} node(s)"
            fetched.isNotEmpty()
        }, timeoutMs) {
            "Timed out waiting for tag: $tag (last observed: $lastObserved)"
        }
        return rule.onNode(androidx.compose.ui.test.hasTestTag(tag))
    }

    fun waitForSaveStatus(
        rule: ComposeTestRule,
        expectedState: String,
        timeoutMs: Long = 15_000L
    ) {
        var lastObservedState: String? = null
        waitUntil(rule, {
            val nodes = rule.onAllNodes(androidx.compose.ui.test.hasTestTag(
                com.xiwei.sujian.designsystem.testing.SujianSemanticIds.EditorSaveStatus
            ))
            val fetched = nodes.fetchSemanticsNodes()
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
            "Expected save status '$expectedState' but last observed was '$lastObservedState'"
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
        var lastObservedState: String? = null
        try {
            rule.waitUntil(timeoutMs) {
                rule.waitForIdle()
                val result = condition()
                if (!result) {
                    lastObservedState = try {
                        val nodes = rule.onAllNodes(
                            androidx.compose.ui.test.hasTestTag(
                                com.xiwei.sujian.designsystem.testing.SujianSemanticIds.EditorSaveStatus
                            )
                        )
                        val fetched = nodes.fetchSemanticsNodes()
                        if (fetched.isEmpty()) "no save-status node" else fetched.first().config.getOrElse(
                            androidx.compose.ui.semantics.SemanticsProperties.StateDescription
                        ) { "no state desc" }.toString()
                    } catch (_: Exception) { "unavailable" }
                }
                result
            }
        } catch (e: androidx.compose.ui.test.ComposeTimeoutException) {
            val diag = if (message != null) message() else "Condition not satisfied"
            throw AssertionError(
                "$diag. Timeout: ${timeoutMs}ms. Last observed state: $lastObservedState. Original: ${e.message}",
                e
            )
        } catch (e: AssertionError) {
            if (message != null) {
                throw AssertionError("${message()}. Last observed state: $lastObservedState. Original: ${e.message}", e)
            }
            throw e
        }
    }
}
