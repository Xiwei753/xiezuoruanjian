@file:Suppress("StringLiteralDuplication") // 测试固件字符串天然重复

package com.xiwei.sujian.feature.editor.session

import com.xiwei.sujian.feature.editor.projection.SessionCloseReason
import com.xiwei.sujian.feature.editor.window.EditableTextTarget
import com.xiwei.sujian.feature.editor.window.EditingState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * #595 四：EditorSessionState 唯一状态源契约测试。
 *
 * 旧缺陷：会话层同时保留 EditorSessionState / activeTargetIdFlow /
 * editingStateFlow / windowBindingStateFlow / targetTexts / persistentSessionIds，
 * 且状态更新不完整：
 * - detachWindowBinding 不改 EditorSessionState.bindingState；
 * - closeTarget 不清 SessionState；
 * - applyLocalEdit 沿用旧 selection（不携带真实选区）；
 * - commitActiveSession 清除 active target 后 SessionState 仍保留旧 target/binding。
 *
 * 修复：EditorSessionState 是唯一正文/选区/revision/binding 事实源，
 * 所有生命周期事件同步更新它；targetTexts 并行缓存已删除。
 */
class EditorSessionStateSingleSourceTest {
    private fun createCoordinator(): EditorSessionCoordinator {
        return EditorSessionCoordinator(
            com.xiwei.sujian.core.interop.app.AppServiceBridge(
                com.xiwei.sujian.core.interop.app.WriterAppServiceHolder(
                    "/tmp/sujian_test_workspace_595",
                    "/tmp/sujian_test_workspace_595",
                ),
            ),
        )
    }

    @Test
    fun applyLocalEdit_carriesRealSelectionIntoSessionState() {
        val coordinator = createCoordinator()
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                targetId = "t1",
                operationKind = EditorOperationKind.INSERT,
                revision = 3L,
                transactionId = 7L,
                contentChanged = true,
                contentDelta = EditorContentDelta(insertedChars = "你好世界".length),
                selectionAnchorUtf8 = 3,
                selectionHeadUtf8 = 9,
            ),
        )
        val state = coordinator.sessionState
        assertEquals("t1", state.targetId)
        // #624 评论9：SessionState 无 text 镜像 — 正文变化由 contentChanged/contentDelta 表达。
        assertEquals(true, state.localDirty)
        assertEquals(3L, state.revision)
        assertEquals(7L, state.lastAppliedTransactionId)
        assertEquals(3, state.selectionAnchorUtf8)
        assertEquals(9, state.selectionHeadUtf8)
        assertEquals(EditorSessionOrigin.LOCAL_INPUT, state.origin)
    }

    @Test
    fun detachWindowBinding_syncsSessionStateBindingState() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = false))
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                "t1",
                1L,
                1L,
                operationKind = EditorOperationKind.INSERT,
                contentChanged = true,
                contentDelta = EditorContentDelta(insertedChars = "text".length),
                selectionAnchorUtf8 = 2,
                selectionHeadUtf8 = 4,
            ),
        )
        coordinator.detachWindowBinding("w1", "t1")
        assertEquals(
            "detachWindowBinding must sync EditorSessionState.bindingState",
            WindowBindingState.Idle,
            coordinator.sessionState.bindingState,
        )
        assertNull(coordinator.sessionState.targetId)
    }

    @Test
    fun closeTarget_resetsSessionStateToIdle() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                "t1",
                1L,
                1L,
                operationKind = EditorOperationKind.INSERT,
                contentChanged = true,
                contentDelta = EditorContentDelta(insertedChars = "text".length),
                selectionAnchorUtf8 = 2,
                selectionHeadUtf8 = 4,
            ),
        )
        coordinator.closeTarget("t1", SessionCloseReason.WORKSPACE_NAVIGATION)
        val state = coordinator.sessionState
        assertNull("closeTarget must clear session target", state.targetId)
        assertNull("closeTarget must clear session id", state.sessionId)
        // #624 评论9：SessionState 无 text 镜像。
        assertEquals(null, state.targetId)
        assertEquals(0L, state.revision)
        assertEquals(WindowBindingState.Idle, state.bindingState)
        assertEquals(EditorSessionOrigin.NONE, state.origin)
    }

    @Test
    fun documentFact_recordsRealVersionAndIsIdempotent() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))

        val first =
            TargetDocumentFact(
                "t1",
                "repo text v1",
                DocumentVersion(contentHash = "hash-1"),
                DocumentVersion(),
                DocumentFactOrigin.REPOSITORY_LOAD,
            )
        assertEquals(
            "New repository version must apply",
            ExternalContentDecision.Apply,
            coordinator.shouldApplyExternalContent(first),
        )
        coordinator.applyExternalContentFact(first)

        var state = coordinator.sessionState
        assertEquals("hash-1", state.committedVersion.contentHash)
        assertEquals(EditorSessionOrigin.EXTERNAL_REPLACE, state.origin)

        // 同一 sourceVersion 重放：幂等，不 reset。
        assertEquals(
            "Same version replay must be idempotent",
            ExternalContentDecision.IgnoreReplay,
            coordinator.shouldApplyExternalContent(first),
        )

        // 新版本 + 新内容：应用（#595 五：加载事实携带 parentVersion=上次已知版本，
        // 与 committed 构成因果链 → 可比较）。
        val second =
            TargetDocumentFact(
                "t1",
                "repo text v2",
                DocumentVersion(contentHash = "hash-2", parentVersion = DocumentVersion(contentHash = "hash-1")),
                DocumentVersion(contentHash = "hash-1"),
                DocumentFactOrigin.REPOSITORY_LOAD,
            )
        assertEquals(
            "New repository version must apply",
            ExternalContentDecision.Apply,
            coordinator.shouldApplyExternalContent(second),
        )
        coordinator.applyExternalContentFact(second)
        state = coordinator.sessionState
        assertEquals("hash-2", state.committedVersion.contentHash)
        assertEquals(EditorSessionOrigin.EXTERNAL_REPLACE, state.origin)
    }

    @Test
    fun documentFact_emptyVersionIsRejected() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))
        val noHash =
            TargetDocumentFact(
                "t1",
                "x",
                DocumentVersion(),
                DocumentVersion(),
                DocumentFactOrigin.REPOSITORY_LOAD,
            )
        assertEquals(
            "Empty version must be rejected",
            ExternalContentDecision.IgnoreEmptyVersion,
            coordinator.shouldApplyExternalContent(noHash),
        )
    }

    @Test
    fun sessionState_hasNoParallelTargetTextsCache() {
        // targetTexts 并行正文缓存必须已删除：会话层正文唯一来源是 sessionState。
        val field =
            EditorSessionCoordinator::class.java.declaredFields.firstOrNull {
                it.name == "targetTexts"
            }
        assertNull(
            "targetTexts parallel text cache must be removed (#595 四)",
            field,
        )
    }

    @Test
    fun activeTargetIdFlow_isNotIndependentMutableStateFlow() {
        // #595 三：_activeTargetIdFlow 必须已删除 — activeTargetIdFlow 从 sessionStateFlow 派生。
        val field =
            EditorSessionCoordinator::class.java.declaredFields.firstOrNull {
                it.name == "_activeTargetIdFlow"
            }
        assertNull(
            "_activeTargetIdFlow must be removed — activeTargetIdFlow derives from sessionStateFlow (#595 三)",
            field,
        )
    }

    @Test
    fun editingStateFlow_isNotIndependentMutableStateFlow() {
        // #595 三：_editingStateFlow 必须已删除 — editingStateFlow 从 sessionStateFlow 派生。
        val field =
            EditorSessionCoordinator::class.java.declaredFields.firstOrNull {
                it.name == "_editingStateFlow"
            }
        assertNull(
            "_editingStateFlow must be removed — editingStateFlow derives from sessionStateFlow (#595 三)",
            field,
        )
    }

    @Test
    fun windowBindingStateFlow_isNotIndependentMutableStateFlow() {
        // #595 三：_windowBindingStateFlow 必须已删除 — windowBindingStateFlow 从 sessionStateFlow 派生。
        val field =
            EditorSessionCoordinator::class.java.declaredFields.firstOrNull {
                it.name == "_windowBindingStateFlow"
            }
        assertNull(
            "_windowBindingStateFlow must be removed — windowBindingStateFlow derives from sessionStateFlow (#595 三)",
            field,
        )
    }

    @Test
    fun sessionStateFlow_isOnlyWritableMutableStateFlow() {
        // #595 三：_sessionStateFlow 是会话层唯一可写 MutableStateFlow<EditorSessionState>。
        val field =
            EditorSessionCoordinator::class.java.declaredFields.firstOrNull {
                it.name == "_sessionStateFlow"
            }
        assertNotNull(
            "_sessionStateFlow must exist as the single writable MutableStateFlow<EditorSessionState>",
            field,
        )
    }

    @Test
    fun editorSessionState_containsEditingStateAndActiveTargetId() {
        // #595 三：EditorSessionState 必须包含 editingState 和 activeTargetId 字段，
        // 使 activeTargetIdFlow/editingStateFlow 能从 sessionStateFlow 派生。
        val editingStateField =
            EditorSessionState::class.java.declaredFields.firstOrNull {
                it.name == "editingState"
            }
        assertNotNull(
            "EditorSessionState must contain editingState field (#595 三)",
            editingStateField,
        )
        val activeTargetIdField =
            EditorSessionState::class.java.declaredFields.firstOrNull {
                it.name == "activeTargetId"
            }
        assertNotNull(
            "EditorSessionState must contain activeTargetId field (#595 三)",
            activeTargetIdField,
        )
    }

    @Test
    fun mutateSession_isSingleMutationGateEntry() {
        // #624 评论17 问题1/3：mutateSession 是 session state/store/epoch 写入的唯一入口。
        // updateSessionState 已删除（#624 评论17 问题3）。
        // internal inline 方法在 JVM 字节码中会被 Kotlin 名称修饰（mutateSession$module），
        // 用 startsWith 匹配避免绑定修饰后缀。
        val method =
            EditorSessionCoordinator::class.java.declaredMethods.firstOrNull {
                it.name.startsWith("mutateSession")
            }
        assertNotNull(
            "mutateSession must exist as the single mutation gate entry point (#624 评论17 问题1)",
            method,
        )
        val updateSessionStateMethod =
            EditorSessionCoordinator::class.java.declaredMethods.firstOrNull {
                it.name.startsWith("updateSessionState")
            }
        assertNull(
            "updateSessionState must be removed (#624 评论17 问题3)",
            updateSessionStateMethod,
        )
    }

    @Test
    fun activeTargetId_derivedFromSessionState() {
        // #595 三：activeTargetId getter 从 sessionStateFlow.value.activeTargetId 派生。
        val coordinator = createCoordinator()
        // 初始状态：activeTargetId 为 null
        assertNull(coordinator.activeTargetId)
        assertEquals(coordinator.sessionState.activeTargetId, coordinator.activeTargetId)
    }

    @Test
    fun editingState_derivedFromSessionState() {
        // #595 三：editingState getter 从 sessionStateFlow.value.editingState 派生。
        val coordinator = createCoordinator()
        assertEquals(EditingState.IDLE, coordinator.editingState)
        assertEquals(coordinator.sessionState.editingState, coordinator.editingState)
    }

    @Test
    fun windowBindingState_derivedFromSessionState() {
        // #595 三：windowBindingState getter 从 sessionStateFlow.value.bindingState 派生。
        val coordinator = createCoordinator()
        assertEquals(WindowBindingState.Idle, coordinator.windowBindingState)
        assertEquals(coordinator.sessionState.bindingState, coordinator.windowBindingState)
    }
}
