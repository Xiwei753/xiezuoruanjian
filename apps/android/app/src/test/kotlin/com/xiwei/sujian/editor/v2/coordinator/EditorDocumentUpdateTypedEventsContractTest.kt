package com.xiwei.sujian.editor.v2.coordinator

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 二：EditorDocumentUpdate 类型化事件与 contentVersion 版本比较契约测试。
 *
 * 验证：
 * - 5 种事件子类型存在且携带 contentVersion；
 * - shouldApplyExternalUpdate 按 contentVersion 判断新旧；
 * - applySyncMerged/applyUndoRestored/applyProgrammaticReplace 设置正确 origin；
 * - applyExternalUpdate 统一分派；
 * - nextContentVersion 单调递增。
 */
class EditorDocumentUpdateTypedEventsContractTest {

    private fun createCoordinator(): EditorSessionCoordinator {
        return EditorSessionCoordinator(com.xiwei.sujian.data.AppServiceBridge(
            com.xiwei.sujian.data.WriterAppServiceHolder("/tmp/sujian_test_workspace_595_typed")
        ))
    }

    @Test
    fun allFiveSubtypesExist() {
        val localInput = EditorDocumentUpdate.LocalInput("t", "text", 1L, 1L, 1L)
        val repoLoaded = EditorDocumentUpdate.RepositoryLoaded("t", "text", "hash", 0L, 1L)
        val syncMerged = EditorDocumentUpdate.SyncMerged("t", "text", 1L, "hash", 0L, 1L)
        val undoRestored = EditorDocumentUpdate.UndoRestored("t", "text", 1L, 0L, 1L, 1L)
        val programmaticReplace = EditorDocumentUpdate.ProgrammaticReplace("t", "text", 1L, 0L, 1L, 1L)

        assertEquals(1L, localInput.contentVersion)
        assertEquals(1L, repoLoaded.contentVersion)
        assertEquals(1L, syncMerged.contentVersion)
        assertEquals(1L, undoRestored.contentVersion)
        assertEquals(1L, programmaticReplace.contentVersion)
    }

    @Test
    fun syncMerged_carriesManifestRevisionAndFileHash() {
        val update = EditorDocumentUpdate.SyncMerged(
            targetId = "t1",
            text = "merged text",
            manifestRevision = 42L,
            fileHash = "sync-hash-1",
            revision = 0L,
            contentVersion = 1L,
        )
        assertEquals("t1", update.targetId)
        assertEquals("merged text", update.text)
        assertEquals(42L, update.manifestRevision)
        assertEquals("sync-hash-1", update.fileHash)
    }

    @Test
    fun undoRestored_carriesSnapshotId() {
        val update = EditorDocumentUpdate.UndoRestored(
            targetId = "t1",
            text = "restored",
            snapshotId = 99L,
            revision = 5L,
            contentVersion = 1L,
            transactionId = 7L,
        )
        assertEquals(99L, update.snapshotId)
        assertEquals(5L, update.revision)
        assertEquals(7L, update.transactionId)
    }

    @Test
    fun programmaticReplace_carriesCommandId() {
        val update = EditorDocumentUpdate.ProgrammaticReplace(
            targetId = "t1",
            text = "replaced",
            commandId = 55L,
            revision = 3L,
            contentVersion = 1L,
            transactionId = 8L,
        )
        assertEquals(55L, update.commandId)
        assertEquals(3L, update.revision)
        assertEquals(8L, update.transactionId)
    }

    @Test
    fun nextContentVersion_isMonotonicallyIncreasing() {
        val coordinator = createCoordinator()
        val v1 = coordinator.nextContentVersion()
        val v2 = coordinator.nextContentVersion()
        val v3 = coordinator.nextContentVersion()
        assertTrue("v2 > v1", v2 > v1)
        assertTrue("v3 > v2", v3 > v2)
    }

    @Test
    fun shouldApplyExternalUpdate_rejectsOldContentVersion() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))
        // Apply first update with contentVersion=5
        val first = EditorDocumentUpdate.SyncMerged("t1", "text v1", 1L, "hash-1", 0L, 5L)
        assertTrue(coordinator.shouldApplyExternalUpdate(first))
        coordinator.applySyncMerged(first)
        assertEquals(5L, coordinator.sessionState.lastAppliedContentVersion)

        // Old event with contentVersion=3 (<= 5) must be rejected
        val old = EditorDocumentUpdate.SyncMerged("t1", "text v2", 2L, "hash-2", 0L, 3L)
        assertFalse("Old contentVersion must be rejected", coordinator.shouldApplyExternalUpdate(old))
    }

    @Test
    fun shouldApplyExternalUpdate_acceptsNewContentVersion() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))
        val first = EditorDocumentUpdate.SyncMerged("t1", "text v1", 1L, "hash-1", 0L, 1L)
        assertTrue(coordinator.shouldApplyExternalUpdate(first))
        coordinator.applySyncMerged(first)

        // New event with contentVersion=10 (> 1) and different text must be accepted
        val newUpdate = EditorDocumentUpdate.SyncMerged("t1", "text v2", 2L, "hash-2", 0L, 10L)
        assertTrue("New contentVersion must be accepted", coordinator.shouldApplyExternalUpdate(newUpdate))
    }

    @Test
    fun applySyncMerged_setsSyncMergedOrigin() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))
        val update = EditorDocumentUpdate.SyncMerged("t1", "synced text", 1L, "hash-1", 0L, 1L)
        coordinator.applySyncMerged(update)
        assertEquals(EditorSessionOrigin.SYNC_MERGED, coordinator.sessionState.origin)
        assertEquals(1L, coordinator.sessionState.lastAppliedContentVersion)
    }

    @Test
    fun applyUndoRestored_setsUndoRestoredOrigin() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))
        val update = EditorDocumentUpdate.UndoRestored(
            "t1", "undo text", 1L, 5L, 1L, 10L,
            selectionAnchorUtf8 = 2, selectionHeadUtf8 = 4,
        )
        coordinator.applyUndoRestored(update)
        assertEquals(EditorSessionOrigin.UNDO_RESTORED, coordinator.sessionState.origin)
        assertEquals("undo text", coordinator.sessionState.text)
        assertEquals(5L, coordinator.sessionState.revision)
        assertEquals(10L, coordinator.sessionState.lastAppliedTransactionId)
        assertEquals(2, coordinator.sessionState.selectionAnchorUtf8)
        assertEquals(4, coordinator.sessionState.selectionHeadUtf8)
        assertEquals(1L, coordinator.sessionState.lastAppliedContentVersion)
    }

    @Test
    fun applyProgrammaticReplace_setsProgrammaticReplaceOrigin() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))
        val update = EditorDocumentUpdate.ProgrammaticReplace(
            "t1", "replaced text", 1L, 3L, 1L, 8L,
            selectionAnchorUtf8 = 0, selectionHeadUtf8 = 5,
        )
        coordinator.applyProgrammaticReplace(update)
        assertEquals(EditorSessionOrigin.PROGRAMMATIC_REPLACE, coordinator.sessionState.origin)
        assertEquals("replaced text", coordinator.sessionState.text)
        assertEquals(3L, coordinator.sessionState.revision)
        assertEquals(8L, coordinator.sessionState.lastAppliedTransactionId)
        assertEquals(1L, coordinator.sessionState.lastAppliedContentVersion)
    }

    @Test
    fun applyExternalUpdate_dispatchesToCorrectApplyMethod() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))

        // SyncMerged
        coordinator.applyExternalUpdate(
            EditorDocumentUpdate.SyncMerged("t1", "sync", 1L, "h1", 0L, 1L)
        )
        assertEquals(EditorSessionOrigin.SYNC_MERGED, coordinator.sessionState.origin)

        // UndoRestored
        coordinator.applyExternalUpdate(
            EditorDocumentUpdate.UndoRestored("t1", "undo", 2L, 1L, 2L, 1L)
        )
        assertEquals(EditorSessionOrigin.UNDO_RESTORED, coordinator.sessionState.origin)

        // ProgrammaticReplace
        coordinator.applyExternalUpdate(
            EditorDocumentUpdate.ProgrammaticReplace("t1", "prog", 3L, 2L, 3L, 1L)
        )
        assertEquals(EditorSessionOrigin.PROGRAMMATIC_REPLACE, coordinator.sessionState.origin)
    }

    @Test
    fun editorEditSource_hasAllFourValues() {
        assertEquals(4, com.xiwei.sujian.editor.v2.host.EditorEditSource.values().size)
        assertTrue(com.xiwei.sujian.editor.v2.host.EditorEditSource.values().contains(com.xiwei.sujian.editor.v2.host.EditorEditSource.NORMAL))
        assertTrue(com.xiwei.sujian.editor.v2.host.EditorEditSource.values().contains(com.xiwei.sujian.editor.v2.host.EditorEditSource.UNDO))
        assertTrue(com.xiwei.sujian.editor.v2.host.EditorEditSource.values().contains(com.xiwei.sujian.editor.v2.host.EditorEditSource.REDO))
        assertTrue(com.xiwei.sujian.editor.v2.host.EditorEditSource.values().contains(com.xiwei.sujian.editor.v2.host.EditorEditSource.PROGRAMMATIC))
    }

    @Test
    fun externalReplaceAndExternalSource_areRemoved() {
        val externalReplaceClass = EditorDocumentUpdate::class.java.declaredClasses.firstOrNull {
            it.simpleName == "ExternalReplace"
        }
        assertTrue("ExternalReplace must be removed", externalReplaceClass == null)

        val externalSourceClass = try {
            Class.forName("com.xiwei.sujian.editor.v2.coordinator.ExternalSource")
        } catch (_: ClassNotFoundException) {
            null
        }
        assertTrue("ExternalSource must be removed", externalSourceClass == null)
    }
}
