package com.xiwei.sujian.support

import androidx.compose.ui.test.ComposeTimeoutException
import androidx.compose.ui.test.SemanticsNodeInteraction
import androidx.compose.ui.test.junit4.ComposeTestRule

object ComposeWait {
    private const val DEFAULT_TIMEOUT_MS = 10_000L

    fun waitForTag(
        rule: ComposeTestRule,
        tag: String,
        timeoutMs: Long = DEFAULT_TIMEOUT_MS
    ): SemanticsNodeInteraction {
        waitUntil(rule, {
            val nodes = rule.onAllNodes(androidx.compose.ui.test.hasTestTag(tag))
            nodes.fetchSemanticsNodes().isNotEmpty()
        }, timeoutMs) {
            "Timed out waiting for tag: $tag"
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
        var lastObserved: String? = null
        var lastConditionError: AssertionError? = null
        val wrappedCondition: () -> Boolean = {
            try {
                val result = condition()
                if (!result) {
                    lastObserved = "condition returned false"
                }
                result
            } catch (e: AssertionError) {
                lastConditionError = e
                lastObserved = e.message
                false
            }
        }
        try {
            rule.waitUntil(timeoutMs, wrappedCondition)
        } catch (e: androidx.compose.ui.test.ComposeTimeoutException) {
            val diag = message?.let { "${it()}. " }.orEmpty()
            val conditionDiag = lastConditionError?.let {
                "Condition threw: ${it.javaClass.simpleName}: ${it.message}"
            }.orEmpty()
            val observedDiag = lastObserved?.let { "Last observed: $it" }.orEmpty()
            val timeoutDiag = "Timeout after ${timeoutMs}ms"
            val combined = buildString {
                append(diag)
                if (conditionDiag.isNotEmpty()) append(conditionDiag + ". ")
                if (observedDiag.isNotEmpty() && conditionDiag.isEmpty()) append(observedDiag + ". ")
                append(timeoutDiag)
            }
            throw AssertionError(combined, e)
        }
    }
}
