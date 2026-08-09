@file:Suppress("StringLiteralDuplication") // 测试固件字符串天然重复

package com.xiwei.sujian.feature.editor.session

import com.xiwei.sujian.feature.editor.platform.EditorEditSource
import com.xiwei.sujian.feature.editor.platform.SujianEditorView
import com.xiwei.sujian.feature.editor.window.EditableTextTarget
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 二/四：EditorDocumentUpdate 类型化事件与文档版本比较契约测试。
 *
 * 验证：
 * - 直接回调事件（LocalInput/UndoRestored/ProgrammaticReplace）不携带
 *   contentVersion（进程内事件序号已删除）；
 * - TargetDocumentFact 携带 DocumentVersion 锚点（contentHash + manifest）；
 * - shouldApplyExternalContent 按版本锚点 + localDirty 判断；
 * - applySyncMerged/applyUndoRestored/applyProgrammaticReplace 设置正确 origin；
 * - PipelineOutput 天然携带 EditorEditSource（无可变侧信道）。
 */
class EditorDocumentUpdateTypedEventsTest {
    private fun createCoordinator(): EditorSessionCoordinator {
        return EditorSessionCoordinator(
            com.xiwei.sujian.core.interop.app.AppServiceBridge(
                com.xiwei.sujian.core.interop.app.WriterAppServiceHolder(
                    "/tmp/sujian_test_workspace_595_typed",
                    "/tmp/sujian_test_workspace_595_typed",
                ),
            ),
        )
    }

    @Test
    fun directCallbackEventsCarryTransactionIds() {
        val localInput = EditorDocumentUpdate.LocalInput("t", "text", 1L, 1L)
        val undoRestored = EditorDocumentUpdate.UndoRestored("t", "text", 1L, 0L, 1L)
        val programmaticReplace = EditorDocumentUpdate.ProgrammaticReplace("t", "text", 1L, 0L, 1L)

        assertEquals(1L, localInput.transactionId)
        assertEquals(1L, undoRestored.transactionId)
        assertEquals(1L, programmaticReplace.transactionId)
        // #595 二：直接回调事件不再携带 contentVersion。
        assertFalse(
            EditorDocumentUpdate.LocalInput::class.java.declaredFields.any { it.name == "contentVersion" },
        )
    }

    @Test
    fun syncMergedFact_carriesManifestAnchorAndFileHash() {
        val fact =
            TargetDocumentFact(
                targetId = "t1",
                text = "merged text",
                sourceVersion = DocumentVersion(contentHash = "sync-hash-1", syncCommitId = "commit-42"),
                baseVersion = DocumentVersion(contentHash = "base-hash"),
                origin = DocumentFactOrigin.SYNC_MERGED,
            )
        assertEquals("t1", fact.targetId)
        assertEquals("merged text", fact.text)
        assertEquals("sync-hash-1", fact.sourceVersion.contentHash)
        assertEquals("commit-42", fact.sourceVersion.syncCommitId)
        assertEquals("base-hash", fact.baseVersion.contentHash)
        assertEquals(DocumentFactOrigin.SYNC_MERGED, fact.origin)
    }

    @Test
    fun undoRestored_carriesSnapshotId() {
        val update =
            EditorDocumentUpdate.UndoRestored(
                targetId = "t1",
                text = "restored",
                snapshotId = 99L,
                revision = 5L,
                transactionId = 7L,
            )
        assertEquals(99L, update.snapshotId)
        assertEquals(5L, update.revision)
        assertEquals(7L, update.transactionId)
    }

    @Test
    fun programmaticReplace_carriesCommandId() {
        val update =
            EditorDocumentUpdate.ProgrammaticReplace(
                targetId = "t1",
                text = "replaced",
                commandId = 55L,
                revision = 3L,
                transactionId = 8L,
            )
        assertEquals(55L, update.commandId)
        assertEquals(3L, update.revision)
        assertEquals(8L, update.transactionId)
    }

    @Test
    fun sameSourceVersionReplay_isRejected() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))
        val fact =
            TargetDocumentFact(
                "t1",
                "text v1",
                DocumentVersion(contentHash = "hash-1"),
                DocumentVersion(),
                DocumentFactOrigin.SYNC_MERGED,
            )
        // 首次 Apply（同正文时经 applyExternalContentFact 记录版本）。
        coordinator.applyExternalContentFact(fact)
        assertEquals("hash-1", coordinator.sessionState.committedVersion.contentHash)

        // 同 sourceVersion 重放 → IgnoreReplay。
        assertEquals(
            ExternalContentDecision.IgnoreReplay,
            coordinator.shouldApplyExternalContent(fact),
        )
    }

    @Test
    fun olderRepositoryRevision_isRejected() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))
        val newer =
            TargetDocumentFact(
                "t1",
                "text v2",
                DocumentVersion(contentHash = "hash-2", repositoryRevision = 200L),
                DocumentVersion(),
                DocumentFactOrigin.SYNC_MERGED,
            )
        coordinator.applyExternalContentFact(newer)

        // revision 更旧（100 < 200）→ IgnoreOlder。
        val older =
            TargetDocumentFact(
                "t1",
                "text v3",
                DocumentVersion(contentHash = "hash-3", repositoryRevision = 100L),
                DocumentVersion(),
                DocumentFactOrigin.SYNC_MERGED,
            )
        assertEquals(
            ExternalContentDecision.IgnoreOlder,
            coordinator.shouldApplyExternalContent(older),
        )
    }

    @Test
    fun localDirty_blocksExternalReset() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))
        // 本地输入 → localDirty=true
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("t1", "本地未保存输入", 3L, 7L, selectionAnchorUtf8 = 2, selectionHeadUtf8 = 4),
        )
        assertTrue(coordinator.sessionState.localDirty)

        // 外部同步下载 → 冲突，禁止直接 reset。
        val fact =
            TargetDocumentFact(
                "t1",
                "远端合并正文",
                DocumentVersion(contentHash = "hash-new", syncCommitId = "commit-500"),
                DocumentVersion(contentHash = "hash-old"),
                DocumentFactOrigin.SYNC_MERGED,
            )
        assertEquals(
            ExternalContentDecision.IgnoreDirtyConflict,
            coordinator.shouldApplyExternalContent(fact),
        )
    }

    @Test
    fun notDirty_differentVersion_applies() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))
        val first =
            TargetDocumentFact(
                "t1",
                "repo v1",
                DocumentVersion(contentHash = "hash-1"),
                DocumentVersion(),
                DocumentFactOrigin.REPOSITORY_LOAD,
            )
        coordinator.applyExternalContentFact(first)
        assertFalse(coordinator.sessionState.localDirty)

        val second =
            TargetDocumentFact(
                "t1",
                "repo v2",
                DocumentVersion(contentHash = "hash-2", parentVersion = DocumentVersion(contentHash = "hash-1")),
                DocumentVersion(contentHash = "hash-1"),
                DocumentFactOrigin.REPOSITORY_LOAD,
            )
        assertEquals(ExternalContentDecision.Apply, coordinator.shouldApplyExternalContent(second))
    }

    @Test
    fun emptyVersion_isRejected() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))
        val noVersion =
            TargetDocumentFact(
                "t1",
                "x",
                DocumentVersion(),
                DocumentVersion(),
                DocumentFactOrigin.SYNC_MERGED,
            )
        assertEquals(ExternalContentDecision.IgnoreEmptyVersion, coordinator.shouldApplyExternalContent(noVersion))
    }

    @Test
    fun applySyncMergedFact_setsSyncMergedOrigin() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))
        val fact =
            TargetDocumentFact(
                "t1",
                "synced text",
                DocumentVersion(contentHash = "hash-1"),
                DocumentVersion(),
                DocumentFactOrigin.SYNC_MERGED,
            )
        coordinator.applyExternalContentFact(fact)
        assertEquals(EditorSessionOrigin.SYNC_MERGED, coordinator.sessionState.origin)
        assertEquals("hash-1", coordinator.sessionState.committedVersion.contentHash)
        assertFalse(coordinator.sessionState.localDirty)
    }

    @Test
    fun applyUndoRestored_setsUndoRestoredOrigin() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))
        val update =
            EditorDocumentUpdate.UndoRestored(
                "t1",
                "undo text",
                1L,
                5L,
                10L,
                selectionAnchorUtf8 = 2,
                selectionHeadUtf8 = 4,
            )
        coordinator.applyUndoRestored(update)
        assertEquals(EditorSessionOrigin.UNDO_RESTORED, coordinator.sessionState.origin)
        assertEquals("undo text", coordinator.sessionState.text)
        assertEquals(5L, coordinator.sessionState.revision)
        assertEquals(10L, coordinator.sessionState.lastAppliedTransactionId)
        assertEquals(2, coordinator.sessionState.selectionAnchorUtf8)
        assertEquals(4, coordinator.sessionState.selectionHeadUtf8)
    }

    @Test
    fun applyProgrammaticReplace_setsProgrammaticReplaceOrigin() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))
        val update =
            EditorDocumentUpdate.ProgrammaticReplace(
                "t1",
                "replaced text",
                1L,
                3L,
                8L,
                selectionAnchorUtf8 = 0,
                selectionHeadUtf8 = 5,
            )
        coordinator.applyProgrammaticReplace(update)
        assertEquals(EditorSessionOrigin.PROGRAMMATIC_REPLACE, coordinator.sessionState.origin)
        assertEquals("replaced text", coordinator.sessionState.text)
        assertEquals(3L, coordinator.sessionState.revision)
        assertEquals(8L, coordinator.sessionState.lastAppliedTransactionId)
    }

    @Test
    fun markSaved_clearsLocalDirtyAndRecordsVersion() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))
        coordinator.applyLocalEdit(EditorDocumentUpdate.LocalInput("t1", "typed", 3L, 7L))
        assertTrue(coordinator.sessionState.localDirty)

        coordinator.markSaved("t1", DocumentVersion(contentHash = "saved-hash"))
        assertFalse("保存成功后 localDirty 必须清除", coordinator.sessionState.localDirty)

        // 保存后外部新版本（基于已保存内容）→ 可应用（不冲突）。
        val fact =
            TargetDocumentFact(
                "t1",
                "merged",
                DocumentVersion(
                    contentHash = "merged-hash",
                    syncCommitId = "commit-3",
                    parentVersion = DocumentVersion(contentHash = "saved-hash"),
                ),
                DocumentVersion(contentHash = "saved-hash"),
                DocumentFactOrigin.SYNC_MERGED,
            )
        assertEquals(ExternalContentDecision.Apply, coordinator.shouldApplyExternalContent(fact))
    }

    @Test
    fun editorEditSource_hasAllFourValues() {
        assertEquals(4, com.xiwei.sujian.feature.editor.platform.EditorEditSource.values().size)
        assertTrue(
            com.xiwei.sujian.feature.editor.platform.EditorEditSource.values().contains(
                com.xiwei.sujian.feature.editor.platform.EditorEditSource.NORMAL,
            ),
        )
        assertTrue(
            com.xiwei.sujian.feature.editor.platform.EditorEditSource.values().contains(
                com.xiwei.sujian.feature.editor.platform.EditorEditSource.UNDO,
            ),
        )
        assertTrue(
            com.xiwei.sujian.feature.editor.platform.EditorEditSource.values().contains(
                com.xiwei.sujian.feature.editor.platform.EditorEditSource.REDO,
            ),
        )
        assertTrue(
            com.xiwei.sujian.feature.editor.platform.EditorEditSource.values().contains(
                com.xiwei.sujian.feature.editor.platform.EditorEditSource.PROGRAMMATIC,
            ),
        )
    }

    @Test
    fun pipelineOutputCarriesEditSource() {
        // #595 四：PipelineOutput.Edited 携带来源 — 撤销/恢复/程序化替换
        // 不再通过 View 可变侧信道标记。
        val editedFields = com.xiwei.sujian.feature.editor.pipeline.PipelineOutput.Edited::class.java.declaredFields
        assertTrue(editedFields.any { it.name == "source" })
        val viewFields = com.xiwei.sujian.feature.editor.platform.SujianEditorView::class.java.declaredFields
        assertFalse(viewFields.any { it.name == "pendingEditSource" })
    }

    @Test
    fun externalReplaceAndExternalSource_areRemoved() {
        val externalReplaceClass =
            EditorDocumentUpdate::class.java.declaredClasses.firstOrNull {
                it.simpleName == "ExternalReplace"
            }
        assertTrue("ExternalReplace must be removed", externalReplaceClass == null)

        val externalSourceClass =
            try {
                Class.forName("com.xiwei.sujian.feature.editor.session.ExternalSource")
            } catch (_: ClassNotFoundException) {
                null
            }
        assertTrue("ExternalSource must be removed", externalSourceClass == null)
    }
}
