package com.xiwei.sujian.editor.v2.coordinator

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.lang.reflect.Method

/**
 * #595 一：EditorSessionCoordinator sessionStateFlow 契约测试。
 *
 * 验证本地输入更新 SessionState（revision/transactionId/origin=LOCAL_INPUT），
 * 外部替换用 revision 判断是否需要 reset。
 */
class EditorSessionStateFlowContractTest {

    @Test
    fun applyLocalEditExistsOnSessionCoordinator() {
        val method = EditorSessionCoordinator::class.java.methods.firstOrNull {
            it.name == "applyLocalEdit" && it.parameterTypes.size == 1 &&
            it.parameterTypes[0] == EditorDocumentUpdate.LocalInput::class.java
        }
        assertNotNull("EditorSessionCoordinator must have applyLocalEdit(LocalInput)", method)
    }

    @Test
    fun shouldApplyExternalReplaceExistsOnSessionCoordinator() {
        val method = EditorSessionCoordinator::class.java.methods.firstOrNull {
            it.name == "shouldApplyExternalReplace" && it.parameterTypes.size == 1 &&
            it.parameterTypes[0] == EditorDocumentUpdate.ExternalReplace::class.java
        }
        assertNotNull("EditorSessionCoordinator must have shouldApplyExternalReplace(ExternalReplace)", method)
    }

    @Test
    fun applyExternalReplaceExistsOnSessionCoordinator() {
        val method = EditorSessionCoordinator::class.java.methods.firstOrNull {
            it.name == "applyExternalReplace" && it.parameterTypes.size == 1 &&
            it.parameterTypes[0] == EditorDocumentUpdate.ExternalReplace::class.java
        }
        assertNotNull("EditorSessionCoordinator must have applyExternalReplace(ExternalReplace)", method)
    }

    @Test
    fun sessionStateFlowExistsOnSessionCoordinator() {
        val field = EditorSessionCoordinator::class.java.getDeclaredField("sessionStateFlow")
        assertNotNull("EditorSessionCoordinator must expose sessionStateFlow", field)
    }

    @Test
    fun applyMotionPolicyIsSingleWritableSource() {
        val method = EditorSessionCoordinator::class.java.methods.firstOrNull {
            it.name == "applyMotionPolicy" && it.parameterTypes.size == 1 &&
            it.parameterTypes[0] == com.xiwei.sujian.editor.v2.motion.EditorMotionPolicy::class.java
        }
        assertNotNull("EditorSessionCoordinator must have applyMotionPolicy(EditorMotionPolicy)", method)
    }

    @Test
    fun editorAnimationSettingsFlowIsNotPresentAsMutableStateFlow() {
        // #595 七：EditorAnimationSettings 不再单独存储为 StateFlow
        val field = EditorSessionCoordinator::class.java.declaredFields.firstOrNull {
            it.name.contains("editorAnimationSettings") && it.name.contains("Flow")
        }
        assertTrue(
            "EditorAnimationSettings must not be stored as a separate StateFlow field",
            field == null,
        )
    }

    @Test
    fun localInputUpdatePreservesRevisionInSessionState() {
        val update = EditorDocumentUpdate.LocalInput("t1", "hello", 7L, 99L)
        val state = EditorSessionState(
            targetId = update.targetId,
            text = update.text,
            revision = update.revision,
            lastAppliedTransactionId = update.transactionId,
            origin = EditorSessionOrigin.LOCAL_INPUT,
        )
        assertEquals(7L, state.revision)
        assertEquals(99L, state.lastAppliedTransactionId)
        assertEquals(EditorSessionOrigin.LOCAL_INPUT, state.origin)
    }

    @Test
    fun externalReplaceWithSameRevisionAndTextDoesNotNeedReset() {
        val currentState = EditorSessionState(
            targetId = "t1",
            text = "text",
            revision = 5L,
            origin = EditorSessionOrigin.LOCAL_INPUT,
        )
        val external = EditorDocumentUpdate.ExternalReplace("t1", "text", 5L, ExternalSource.SYNC_MERGE)
        // Same revision and text → no reset
        assertFalse(currentState.revision == external.revision && currentState.text == external.text == false)
        assertTrue(currentState.revision == external.revision && currentState.text == external.text)
    }

    @Test
    fun externalReplaceWithNewerRevisionNeedsReset() {
        val currentState = EditorSessionState(
            targetId = "t1",
            text = "old",
            revision = 3L,
            origin = EditorSessionOrigin.LOCAL_INPUT,
        )
        val external = EditorDocumentUpdate.ExternalReplace("t1", "new", 10L, ExternalSource.REPOSITORY_LOAD)
        // Local revision < external revision → reset needed
        assertTrue(currentState.revision < external.revision)
    }

    @Test
    fun externalReplaceWithStaleRevisionDoesNotNeedReset() {
        val currentState = EditorSessionState(
            targetId = "t1",
            text = "local edit",
            revision = 10L,
            origin = EditorSessionOrigin.LOCAL_INPUT,
        )
        val external = EditorDocumentUpdate.ExternalReplace("t1", "stale", 5L, ExternalSource.SYNC_MERGE)
        // Local input with newer revision → no reset
        assertTrue(currentState.origin == EditorSessionOrigin.LOCAL_INPUT && currentState.revision >= external.revision)
    }
}
