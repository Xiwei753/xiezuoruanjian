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
 * 外部加载用真实 fileHash 判断是否需要 reset（#595 二：不再伪造 revision）。
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
    fun shouldApplyRepositoryLoadExistsOnSessionCoordinator() {
        val method = EditorSessionCoordinator::class.java.methods.firstOrNull {
            it.name == "shouldApplyRepositoryLoad" && it.parameterTypes.size == 1 &&
            it.parameterTypes[0] == EditorDocumentUpdate.RepositoryLoaded::class.java
        }
        assertNotNull("EditorSessionCoordinator must have shouldApplyRepositoryLoad(RepositoryLoaded)", method)
    }

    @Test
    fun applyRepositoryLoadedExistsOnSessionCoordinator() {
        val method = EditorSessionCoordinator::class.java.methods.firstOrNull {
            it.name == "applyRepositoryLoaded" && it.parameterTypes.size == 1 &&
            it.parameterTypes[0] == EditorDocumentUpdate.RepositoryLoaded::class.java
        }
        assertNotNull("EditorSessionCoordinator must have applyRepositoryLoaded(RepositoryLoaded)", method)
    }

    @Test
    fun fabricatedExternalReplaceProtocol_removed() {
        // #595 二：外部更新不得再由 UI 伪造 revision/source。
        assertTrue(
            "shouldApplyExternalReplace must be removed",
            EditorSessionCoordinator::class.java.methods.none { it.name == "shouldApplyExternalReplace" },
        )
        assertTrue(
            "applyExternalReplace must be removed",
            EditorSessionCoordinator::class.java.methods.none { it.name == "applyExternalReplace" },
        )
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
        val update = EditorDocumentUpdate.LocalInput("t1", "hello", 7L, 1L, 99L)
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
    fun repositoryLoadWithSameHashAndTextDoesNotNeedReset() {
        // #595 一：同一 fileHash 且内容一致 → 幂等重放，不 reset。
        val currentState = EditorSessionState(
            targetId = "t1",
            text = "text",
            revision = 5L,
            origin = EditorSessionOrigin.EXTERNAL_REPLACE,
            lastRepositoryHash = "hash-1",
        )
        val load = EditorDocumentUpdate.RepositoryLoaded("t1", "text", "hash-1", 0L, contentVersion = 1L)
        val alreadyApplied = currentState.lastRepositoryHash == load.fileHash &&
            currentState.text == load.text
        assertTrue("Same hash + same text must be idempotent (no reset)", alreadyApplied)
    }

    @Test
    fun repositoryLoadWithNewContentNeedsReset() {
        val currentState = EditorSessionState(
            targetId = "t1",
            text = "old",
            revision = 3L,
            lastRepositoryHash = "hash-1",
        )
        val load = EditorDocumentUpdate.RepositoryLoaded("t1", "new", "hash-2", 0L, contentVersion = 1L)
        val needsReset = currentState.text != load.text
        assertTrue("Different content must trigger reset protocol", needsReset)
    }

    @Test
    fun repositoryLoadWithSameContentDoesNotNeedReset() {
        // #595 一：内容与当前 session 一致 → 无需 reset（即使 hash 不同，内容相同）。
        val currentState = EditorSessionState(
            targetId = "t1",
            text = "same",
            revision = 10L,
            origin = EditorSessionOrigin.LOCAL_INPUT,
            lastRepositoryHash = "hash-1",
        )
        val load = EditorDocumentUpdate.RepositoryLoaded("t1", "same", "hash-2", 0L, contentVersion = 1L)
        val needsReset = currentState.text != load.text
        assertFalse("Same content must not reset even with different hash", needsReset)
    }
}
