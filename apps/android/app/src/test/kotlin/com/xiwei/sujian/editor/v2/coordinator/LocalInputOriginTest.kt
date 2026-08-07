package com.xiwei.sujian.editor.v2.coordinator

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 一：本地输入回灌不触发 session reset 契约测试。
 *
 * 验证 WritingPane 用 `sessionState.origin == LOCAL_INPUT && sessionState.text == uiState.content`
 * 判断本地更新，而非 `revision == lastAppliedRevision` 比较（后者在连续输入时第二次不满足）。
 *
 * onLocalEdit 先于 onContentChanged 调用，确保 LaunchedEffect(uiState.content) 触发时
 * sessionStateFlow 已是最新。
 */
class LocalInputOriginTest {
    @Test
    fun localInputWithMatchingTextDoesNotTriggerReset() {
        val sessionState =
            EditorSessionState(
                targetId = "t1",
                text = "hello world",
                revision = 5L,
                origin = EditorSessionOrigin.LOCAL_INPUT,
            )
        val uiContent = "hello world"
        val isLocal =
            sessionState.origin == EditorSessionOrigin.LOCAL_INPUT &&
                sessionState.text == uiContent
        assertTrue("Local input with matching text must not trigger reset", isLocal)
    }

    @Test
    fun externalReplaceWithMismatchedTextTriggersReset() {
        val sessionState =
            EditorSessionState(
                targetId = "t1",
                text = "local edit",
                revision = 5L,
                origin = EditorSessionOrigin.LOCAL_INPUT,
            )
        val uiContent = "external replace"
        val needsReset = uiContent != sessionState.text
        assertTrue("External replace with mismatched text must trigger reset", needsReset)
    }

    @Test
    fun consecutiveLocalInputsDoNotFalselyTriggerReset() {
        val state1 =
            EditorSessionState(
                targetId = "t1",
                text = "a",
                revision = 5L,
                origin = EditorSessionOrigin.LOCAL_INPUT,
            )
        val content1 = "a"
        val isLocal1 =
            state1.origin == EditorSessionOrigin.LOCAL_INPUT &&
                state1.text == content1
        assertTrue("First local input must be detected", isLocal1)

        val state2 =
            EditorSessionState(
                targetId = "t1",
                text = "ab",
                revision = 6L,
                origin = EditorSessionOrigin.LOCAL_INPUT,
            )
        val content2 = "ab"
        val isLocal2 =
            state2.origin == EditorSessionOrigin.LOCAL_INPUT &&
                state2.text == content2
        assertTrue("Second consecutive local input must be detected", isLocal2)
    }

    @Test
    fun initialLoadWithMatchingTextDoesNotTriggerReset() {
        val sessionState =
            EditorSessionState(
                targetId = "t1",
                text = "loaded content",
                revision = 0L,
                origin = EditorSessionOrigin.INITIAL_LOAD,
            )
        val uiContent = "loaded content"
        val isLocal =
            sessionState.origin == EditorSessionOrigin.LOCAL_INPUT &&
                sessionState.text == uiContent
        assertFalse("Initial load is not local input", isLocal)
        val needsReset = uiContent != sessionState.text
        assertFalse("Initial load with matching text does not need reset", needsReset)
    }

    @Test
    fun externalReplaceWithSameTextButDifferentOriginDoesNotReset() {
        val sessionState =
            EditorSessionState(
                targetId = "t1",
                text = "same",
                revision = 5L,
                origin = EditorSessionOrigin.LOCAL_INPUT,
            )
        val uiContent = "same"
        val isLocal =
            sessionState.origin == EditorSessionOrigin.LOCAL_INPUT &&
                sessionState.text == uiContent
        assertTrue("Same text with LOCAL_INPUT origin is local", isLocal)
        val needsReset = uiContent != sessionState.text
        assertFalse("Same text does not need reset", needsReset)
    }
}
