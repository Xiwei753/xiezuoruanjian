package com.xiwei.sujian.editor.v2.coordinator

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 一/二：EditorSessionCoordinator sessionStateFlow 契约测试。
 *
 * 验证本地输入更新 SessionState（revision/transactionId/origin=LOCAL_INPUT），
 * 外部文档事实用 DocumentVersion 判断是否需要 reset（#595 二：不再伪造 revision、
 * 不再使用进程内 contentVersion）。
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
    fun shouldApplyExternalContentExistsOnSessionCoordinator() {
        val method = EditorSessionCoordinator::class.java.methods.firstOrNull {
            it.name == "shouldApplyExternalContent" && it.parameterTypes.size == 1 &&
            it.parameterTypes[0] == TargetDocumentFact::class.java
        }
        assertNotNull("EditorSessionCoordinator must have shouldApplyExternalContent(TargetDocumentFact)", method)
    }

    @Test
    fun applyExternalContentFactExistsOnSessionCoordinator() {
        val method = EditorSessionCoordinator::class.java.methods.firstOrNull {
            it.name == "applyExternalContentFact" && it.parameterTypes.size == 1 &&
            it.parameterTypes[0] == TargetDocumentFact::class.java
        }
        assertNotNull("EditorSessionCoordinator must have applyExternalContentFact(TargetDocumentFact)", method)
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
        assertTrue(
            "shouldApplyRepositoryLoad (old event API) must be removed",
            EditorSessionCoordinator::class.java.methods.none { it.name == "shouldApplyRepositoryLoad" },
        )
        assertTrue(
            "shouldApplyExternalUpdate (old event API) must be removed",
            EditorSessionCoordinator::class.java.methods.none { it.name == "shouldApplyExternalUpdate" },
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
    fun documentFactWithSameVersionAndText_doesNotNeedReset() {
        // #595 二：同 sourceVersion 且正文一致 → 幂等重放，不 reset。
        val fact = TargetDocumentFact(
            "t1", "text",
            DocumentVersion(contentHash = "hash-1"),
            DocumentVersion(),
            DocumentFactOrigin.REPOSITORY_LOAD,
        )
        val coordinator = EditorSessionCoordinator(com.xiwei.sujian.data.AppServiceBridge(
            com.xiwei.sujian.data.WriterAppServiceHolder("/tmp/sujian_test_workspace_595_stateflow")
        ))
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))
        coordinator.applyExternalContentFact(fact)
        assertEquals(
            "同版本重放必须 IgnoreReplay",
            ExternalContentDecision.IgnoreReplay,
            coordinator.shouldApplyExternalContent(fact),
        )
    }

    @Test
    fun documentFactWithNewContentNeedsReset() {
        // #595 二：不同版本 + 正文不同 + 非 dirty → Apply。
        val coordinator = EditorSessionCoordinator(com.xiwei.sujian.data.AppServiceBridge(
            com.xiwei.sujian.data.WriterAppServiceHolder("/tmp/sujian_test_workspace_595_stateflow")
        ))
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))
        coordinator.applyExternalContentFact(
            TargetDocumentFact("t1", "old", DocumentVersion(contentHash = "hash-1"), DocumentVersion(), DocumentFactOrigin.REPOSITORY_LOAD)
        )
        val load = TargetDocumentFact("t1", "new", DocumentVersion(contentHash = "hash-2"), DocumentVersion(contentHash = "hash-1"), DocumentFactOrigin.REPOSITORY_LOAD)
        assertEquals("Different content must trigger reset protocol", ExternalContentDecision.Apply, coordinator.shouldApplyExternalContent(load))
    }
}
