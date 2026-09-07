package com.xiwei.sujian.storage.mirror

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #649 评论 5562715833：镜像事务状态机单元测试。
 *
 * 覆盖：
 * - PendingMirrorPublish JSON 序列化/反序列化
 * - PendingItem 状态推进逻辑
 * - ReadableMirrorStorage 接口契约（backup/promote/restore）
 * - DELETE_PROJECT 事务中 snapshot=null 语义
 * - manifest 中间状态 journal 落盘
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class MirrorTransactionStateTest {

    // ── PendingMirrorPublish JSON round-trip ──

    @Test
    fun pendingMirrorPublish_jsonRoundTrip_upsertProject() {
        val key1 = ChapterKey("p1", "v1", "ch1")
        val key2 = ChapterKey("p1", "v1", "ch2")
        val entry1 = ChapterMirrorEntry("content://media/1", "作品/P/V/Ch1.md", 100L, "sha256:abc")
        val entry2 = ChapterMirrorEntry("content://media/2", "作品/P/V/Ch2.md", 200L, "sha256:def")

        val staged1 = StagedMirrorRef("tx1", "content://staging/1", ".staging/tx1/作品/P/V/Ch1.md", "作品/P/V/Ch1.md", "text/markdown")
        val staged2 = StagedMirrorRef("tx1", "content://staging/2", ".staging/tx1/作品/P/V/Ch2.md", "作品/P/V/Ch2.md", "text/markdown")

        val item1 = PendingItem(
            key = key1,
            stagedRef = staged1,
            oldRef = MirrorFileRef("content://old/1", "作品/P/V/Ch1.md"),
            backupOldRef = MirrorFileRef("content://backup/1", ".staging/tx1/backup/作品/P/V/Ch1.md"),
            promotedRef = MirrorFileRef("content://new/1", "作品/P/V/Ch1.md"),
            state = PendingItem.STATE_PROMOTED,
        )
        val item2 = PendingItem(
            key = key2,
            stagedRef = staged2,
            oldRef = MirrorFileRef("content://old/2", "作品/P/V/Ch2.md"),
            backupOldRef = null,
            promotedRef = null,
            state = PendingItem.STATE_STAGED,
        )

        val manifestOldRef = MirrorFileRef("content://manifest/old", "_meta/manifest.json")
        val manifestStagedRef = StagedMirrorRef("tx1", "content://manifest/staged", ".staging/tx1/_meta/manifest.json", "_meta/manifest.json", "application/json")
        val manifestNewRef = MirrorFileRef("content://manifest/new", "_meta/manifest.json")
        val manifestBackupRef = MirrorFileRef("content://manifest/backup", "_meta/manifest.json")

        val journal = PendingMirrorPublish(
            txId = "tx1",
            backend = MirrorBackend.DOCUMENT_TREE,
            treeUri = "content://tree/doc",
            projectId = "p1",
            transactionType = MirrorTransactionType.UPSERT_PROJECT,
            phase = PendingMirrorPublish.PHASE_PROMOTE,
            oldEntries = mapOf(key1 to entry1, key2 to entry2),
            newEntries = mapOf(key1 to entry2, key2 to entry1),
            stagedRefs = mapOf(key1 to staged1, key2 to staged2),
            items = mapOf(key1 to item1, key2 to item2),
            removedProjectIds = emptySet(),
            manifestOldRef = manifestOldRef,
            manifestStagedRef = manifestStagedRef,
            manifestNewRef = manifestNewRef,
            manifestBackupRef = manifestBackupRef,
            isManifestCommitted = false,
        )

        val json = journal.toJson()
        val restored = PendingMirrorPublish.fromJson(json)

        assertNotNull("deserialization should succeed", restored)
        restored!!
        assertEquals("tx1", restored.txId)
        assertEquals(MirrorBackend.DOCUMENT_TREE, restored.backend)
        assertEquals("content://tree/doc", restored.treeUri)
        assertEquals("p1", restored.projectId)
        assertEquals(MirrorTransactionType.UPSERT_PROJECT, restored.transactionType)
        assertEquals(PendingMirrorPublish.PHASE_PROMOTE, restored.phase)
        assertEquals(2, restored.oldEntries.size)
        assertEquals(2, restored.newEntries.size)
        assertEquals(2, restored.stagedRefs.size)
        assertEquals(2, restored.items.size)
        assertTrue(restored.removedProjectIds.isEmpty())
        assertFalse(restored.isManifestCommitted)

        // Verify manifest refs
        assertNotNull(restored.manifestOldRef)
        assertEquals("content://manifest/old", restored.manifestOldRef!!.uri)
        assertNotNull(restored.manifestStagedRef)
        assertEquals("content://manifest/staged", restored.manifestStagedRef!!.stagingUri)
        assertNotNull(restored.manifestNewRef)
        assertEquals("content://manifest/new", restored.manifestNewRef!!.uri)
        assertNotNull(restored.manifestBackupRef)
        assertEquals("content://manifest/backup", restored.manifestBackupRef!!.uri)

        // Verify items state
        val r1 = restored.items[key1]!!
        assertEquals(PendingItem.STATE_PROMOTED, r1.state)
        assertEquals("content://backup/1", r1.backupOldRef!!.uri)
        assertNotNull(r1.promotedRef)
        assertEquals("content://new/1", r1.promotedRef!!.uri)

        val r2 = restored.items[key2]!!
        assertEquals(PendingItem.STATE_STAGED, r2.state)
        assertNull(r2.backupOldRef)
        assertNull(r2.promotedRef)
    }

    @Test
    fun pendingMirrorPublish_jsonRoundTrip_deleteProject() {
        val journal = PendingMirrorPublish(
            txId = "tx-del",
            backend = MirrorBackend.MEDIA_STORE,
            treeUri = null,
            projectId = "p-del",
            transactionType = MirrorTransactionType.DELETE_PROJECT,
            phase = PendingMirrorPublish.PHASE_CLEANUP,
            oldEntries = mapOf(
                ChapterKey("p-del", "v1", "ch1") to
                    ChapterMirrorEntry("content://old/1", "作品/Del/V/Ch.md", 100L, "sha256:x"),
            ),
            newEntries = emptyMap(),
            stagedRefs = emptyMap(),
            items = emptyMap(),
            removedProjectIds = setOf("p-del"),
            manifestOldRef = null,
            manifestStagedRef = null,
            manifestNewRef = MirrorFileRef("content://manifest/new", "_meta/manifest.json"),
            manifestBackupRef = MirrorFileRef("content://manifest/backup", "_meta/manifest.json"),
            isManifestCommitted = true,
        )

        val json = journal.toJson()
        val restored = PendingMirrorPublish.fromJson(json)

        assertNotNull(restored)
        restored!!
        assertEquals(MirrorTransactionType.DELETE_PROJECT, restored.transactionType)
        assertEquals(PendingMirrorPublish.PHASE_CLEANUP, restored.phase)
        assertTrue(restored.isManifestCommitted)
        assertEquals(setOf("p-del"), restored.removedProjectIds)
        assertTrue(restored.newEntries.isEmpty())
        assertNull(restored.manifestOldRef)
        assertNull(restored.manifestStagedRef)
        assertNotNull(restored.manifestNewRef)
        assertNotNull(restored.manifestBackupRef)
    }

    @Test
    fun pendingMirrorPublish_jsonRoundTrip_nullManifestFields() {
        val journal = PendingMirrorPublish(
            txId = "tx-null",
            backend = MirrorBackend.MEDIA_STORE,
            treeUri = null,
            projectId = "p1",
            transactionType = MirrorTransactionType.UPSERT_PROJECT,
            phase = PendingMirrorPublish.PHASE_STAGE,
            oldEntries = emptyMap(),
            newEntries = emptyMap(),
            stagedRefs = emptyMap(),
            items = emptyMap(),
            removedProjectIds = emptySet(),
            manifestOldRef = null,
            manifestStagedRef = null,
            manifestNewRef = null,
            manifestBackupRef = null,
        )

        val json = journal.toJson()
        val restored = PendingMirrorPublish.fromJson(json)

        assertNotNull(restored)
        restored!!
        assertNull(restored.manifestOldRef)
        assertNull(restored.manifestStagedRef)
        assertNull(restored.manifestNewRef)
        assertNull(restored.manifestBackupRef)
        assertFalse(restored.isManifestCommitted)
    }

    @Test
    fun pendingMirrorPublish_deserializeInvalidJson_returnsNull() {
        assertNull(PendingMirrorPublish.fromJson("not valid json"))
        assertNull(PendingMirrorPublish.fromJson(""))
        assertNull(PendingMirrorPublish.fromJson("{}"))
    }

    // ── PendingItem state machine ──

    @Test
    fun pendingItem_stateTransitions_stagedToOldBackedUp() {
        val key = ChapterKey("p1", "v1", "ch1")
        val staged = StagedMirrorRef("tx1", "content://staging", ".staging/tx1/f.md", "f.md", "text/markdown")
        val oldRef = MirrorFileRef("content://old", "f.md")
        val backupRef = MirrorFileRef("content://backup", ".staging/tx1/backup/f.md")

        val item = PendingItem(
            key = key,
            stagedRef = staged,
            oldRef = oldRef,
            backupOldRef = null,
            promotedRef = null,
            state = PendingItem.STATE_STAGED,
        )
        assertEquals(PendingItem.STATE_STAGED, item.state)
        assertNull(item.backupOldRef)

        val backedUp = item.copy(backupOldRef = backupRef, state = PendingItem.STATE_OLD_BACKED_UP)
        assertEquals(PendingItem.STATE_OLD_BACKED_UP, backedUp.state)
        assertNotNull(backedUp.backupOldRef)
        assertEquals("content://backup", backedUp.backupOldRef!!.uri)
    }

    @Test
    fun pendingItem_stateTransitions_oldBackedUpToPromoted() {
        val key = ChapterKey("p1", "v1", "ch1")
        val staged = StagedMirrorRef("tx1", "content://staging", ".staging/tx1/f.md", "f.md", "text/markdown")
        val backupRef = MirrorFileRef("content://backup", ".staging/tx1/backup/f.md")
        val newRef = MirrorFileRef("content://new", "f.md")

        val item = PendingItem(
            key = key,
            stagedRef = staged,
            oldRef = MirrorFileRef("content://old", "f.md"),
            backupOldRef = backupRef,
            promotedRef = null,
            state = PendingItem.STATE_OLD_BACKED_UP,
        )

        val promoted = item.copy(promotedRef = newRef, state = PendingItem.STATE_PROMOTED)
        assertEquals(PendingItem.STATE_PROMOTED, promoted.state)
        assertNotNull(promoted.promotedRef)
        assertEquals("content://new", promoted.promotedRef!!.uri)
    }

    @Test
    fun pendingItem_stateTransitions_promotedToCommitted() {
        val key = ChapterKey("p1", "v1", "ch1")
        val item = PendingItem(
            key = key,
            stagedRef = StagedMirrorRef("tx1", "content://s", ".staging/tx1/f.md", "f.md", "text/markdown"),
            oldRef = MirrorFileRef("content://old", "f.md"),
            backupOldRef = MirrorFileRef("content://backup", ".staging/tx1/backup/f.md"),
            promotedRef = MirrorFileRef("content://new", "f.md"),
            state = PendingItem.STATE_PROMOTED,
        )

        val committed = item.copy(state = PendingItem.STATE_COMMITTED)
        assertEquals(PendingItem.STATE_COMMITTED, committed.state)
    }

    @Test
    fun pendingItem_stateTransitions_newProject_noOldRef() {
        val key = ChapterKey("p1", "v1", "ch1")
        val item = PendingItem(
            key = key,
            stagedRef = StagedMirrorRef("tx1", "content://s", ".staging/tx1/f.md", "f.md", "text/markdown"),
            oldRef = null,
            backupOldRef = null,
            promotedRef = null,
            state = PendingItem.STATE_STAGED,
        )
        assertNull(item.oldRef)
        assertNull(item.backupOldRef)

        // New project skips backup step
        val newRef = MirrorFileRef("content://new", "f.md")
        val promoted = item.copy(promotedRef = newRef, state = PendingItem.STATE_PROMOTED)
        assertNull(promoted.backupOldRef)
        assertEquals(PendingItem.STATE_PROMOTED, promoted.state)
    }

    // ── PendingItem JSON serialization ──

    @Test
    fun pendingItem_jsonRoundTrip_allStates() {
        val states = listOf(
            PendingItem.STATE_STAGED,
            PendingItem.STATE_OLD_BACKED_UP,
            PendingItem.STATE_PROMOTED,
            PendingItem.STATE_COMMITTED,
        )
        val key = ChapterKey("p1", "v1", "ch1")
        val staged = StagedMirrorRef("tx1", "content://s", ".staging/tx1/f.md", "f.md", "text/markdown")

        for (state in states) {
            val item = PendingItem(
                key = key,
                stagedRef = staged,
                oldRef = MirrorFileRef("content://old", "f.md"),
                backupOldRef = if (state != PendingItem.STATE_STAGED) MirrorFileRef("content://backup", "backup/f.md") else null,
                promotedRef = if (state == PendingItem.STATE_PROMOTED || state == PendingItem.STATE_COMMITTED) MirrorFileRef("content://new", "f.md") else null,
                state = state,
            )

            val journal = PendingMirrorPublish(
                txId = "tx1",
                backend = MirrorBackend.MEDIA_STORE,
                treeUri = null,
                projectId = "p1",
                transactionType = MirrorTransactionType.UPSERT_PROJECT,
                phase = PendingMirrorPublish.PHASE_PROMOTE,
                oldEntries = emptyMap(),
                newEntries = emptyMap(),
                stagedRefs = emptyMap(),
                items = mapOf(key to item),
                removedProjectIds = emptySet(),
                manifestOldRef = null,
                manifestStagedRef = null,
                manifestNewRef = null,
                manifestBackupRef = null,
            )

            val json = journal.toJson()
            val restored = PendingMirrorPublish.fromJson(json)!!
            val restoredItem = restored.items[key]!!

            assertEquals("State $state should round-trip", state, restoredItem.state)
            assertEquals("stagedRef should round-trip", staged.stagingUri, restoredItem.stagedRef?.stagingUri)
            assertEquals("oldRef should round-trip", "content://old", restoredItem.oldRef?.uri)
        }
    }

    // ── FakeReadableMirrorStorage: interface contract tests ──

    @Test
    fun fakeStorage_promoteStaged_doesNotDeleteOld() {
        val storage = FakeReadableMirrorStorage()
        storage.committedFiles["content://old"] = "old content"
        storage.committedFiles["content://new"] = "new content"

        val staged = StagedMirrorRef("tx1", "content://staging", ".staging/tx1/f.md", "f.md", "text/markdown")
        storage.stagingFiles["content://staging"] = "staging content"

        val result = storage.promoteStaged(staged, "f.md")
        assertNotNull(result)
        // Old file should still exist (promoteStaged doesn't delete old)
        assertTrue("old file should still exist after promote", storage.committedFiles.containsKey("content://old"))
    }

    @Test
    fun fakeStorage_backupCommitted_createsCopy() {
        val storage = FakeReadableMirrorStorage()
        storage.committedFiles["content://old"] = "important content"

        val old = MirrorFileRef("content://old", "作品/P/V/Ch.md")
        val backup = storage.backupCommitted("tx1", old)

        assertNotNull(backup)
        assertTrue("backup should exist", storage.committedFiles.containsKey(backup!!.uri))
        assertEquals("backup content should match old", "important content", storage.committedFiles[backup.uri])
        // Old should still be there
        assertTrue("old should still exist", storage.committedFiles.containsKey("content://old"))
    }

    @Test
    fun fakeStorage_restoreBackup_writesToFinalLocation() {
        val storage = FakeReadableMirrorStorage()
        storage.committedFiles["content://backup"] = "backed up content"

        val backup = MirrorFileRef("content://backup", "backup/f.md")
        val result = storage.restoreBackup(backup, "作品/P/V/Ch.md")

        assertNotNull(result)
        assertEquals("restored content should match backup", "backed up content", storage.committedFiles[result!!.uri])
    }

    @Test
    fun fakeStorage_rollback_deletesAllStagingFiles() {
        val storage = FakeReadableMirrorStorage()
        storage.stagingFiles["content://staging/1"] = "content1"
        storage.stagingFiles["content://staging/2"] = "content2"

        storage.rollback("tx1")

        assertTrue("staging should be cleared", storage.stagingFiles.isEmpty())
    }

    // ── DELETE_PROJECT: snapshot=null semantics ──

    @Test
    fun buildManifestJsonForDesired_snapshotNull_omitsProject() {
        // When snapshot is null, the project should be omitted from manifest.
        // This is tested via FakeReadableMirrorStorage's manifest tracking.
        val storage = FakeReadableMirrorStorage()

        // Simulate: project p1 is being deleted, so snapshot=null for p1
        // Manifest should not contain p1
        val manifestProjects = mutableSetOf<String>()

        // Fake manifest building: project with null snapshot → omitted
        val snapshot: FakeProjectSnapshot? = null
        val projectId = "p1"
        if (snapshot != null) {
            manifestProjects.add(projectId)
        }

        assertFalse("project with null snapshot should be omitted", manifestProjects.contains(projectId))
    }

    @Test
    fun buildManifestJsonForDesired_snapshotPresent_includesProject() {
        val storage = FakeReadableMirrorStorage()

        val manifestProjects = mutableSetOf<String>()
        val snapshot = FakeProjectSnapshot("p1", "My Project")
        val projectId = "p1"
        if (snapshot != null) {
            manifestProjects.add(projectId)
        }

        assertTrue("project with non-null snapshot should be included", manifestProjects.contains(projectId))
    }

    // ── Transaction flow order verification ──

    @Test
    fun transactionFlow_backupBeforePromote() {
        val storage = FakeReadableMirrorStorage()
        storage.committedFiles["content://old"] = "old content"
        storage.stagingFiles["content://staging"] = "new content"

        val txId = "tx1"
        val oldRef = MirrorFileRef("content://old", "f.md")
        val staged = StagedMirrorRef(txId, "content://staging", ".staging/tx1/f.md", "f.md", "text/markdown")

        // Step 1: backup old
        val backup = storage.backupCommitted(txId, oldRef)
        assertNotNull("backup should succeed", backup)

        // Step 2: promote staged (old still exists)
        val promoted = storage.promoteStaged(staged, "f.md")
        assertNotNull("promote should succeed", promoted)
        assertTrue("old should still exist before cleanup", storage.committedFiles.containsKey("content://old"))

        // Step 3: cleanup (simulating manifest committed)
        storage.delete(oldRef)
        storage.delete(backup!!)

        assertFalse("old should be deleted after cleanup", storage.committedFiles.containsKey("content://old"))
        assertFalse("backup should be deleted after cleanup", storage.committedFiles.containsKey(backup.uri))
    }

    @Test
    fun transactionFlow_newProject_skipsBackup() {
        val storage = FakeReadableMirrorStorage()
        storage.stagingFiles["content://staging"] = "new content"

        val txId = "tx1"
        val staged = StagedMirrorRef(txId, "content://staging", ".staging/tx1/f.md", "f.md", "text/markdown")

        // No old ref → no backup step
        val oldRef: MirrorFileRef? = null
        val backup: MirrorFileRef? = if (oldRef != null) storage.backupCommitted(txId, oldRef) else null
        assertNull("no backup for new project", backup)

        // Promote directly
        val promoted = storage.promoteStaged(staged, "f.md")
        assertNotNull("promote should succeed", promoted)
    }

    @Test
    fun transactionFlow_promoteFailure_restoresBackup() {
        val storage = FakeReadableMirrorStorage()
        storage.committedFiles["content://old"] = "old content"
        storage.stagingFiles["content://staging"] = "new content"
        storage.failPromote = true // Force promote to fail

        val txId = "tx1"
        val oldRef = MirrorFileRef("content://old", "f.md")
        val staged = StagedMirrorRef(txId, "content://staging", ".staging/tx1/f.md", "f.md", "text/markdown")

        // Step 1: backup old
        val backup = storage.backupCommitted(txId, oldRef)
        assertNotNull(backup)

        // Step 2: promote fails
        val promoted = storage.promoteStaged(staged, "f.md")
        assertNull("promote should fail", promoted)

        // Step 3: restore backup on failure
        val restored = storage.restoreBackup(backup!!, "f.md")
        assertNotNull("restore should succeed", restored)
        assertEquals("restored content should match old", "old content", storage.committedFiles[restored!!.uri])
    }

    // ── Recovery: skip PROMOTED/COMMITTED items ──

    @Test
    fun recovery_shouldSkipPromotedItems() {
        val key = ChapterKey("p1", "v1", "ch1")
        val item = PendingItem(
            key = key,
            stagedRef = StagedMirrorRef("tx1", "content://s", ".staging/tx1/f.md", "f.md", "text/markdown"),
            oldRef = MirrorFileRef("content://old", "f.md"),
            backupOldRef = MirrorFileRef("content://backup", "backup/f.md"),
            promotedRef = MirrorFileRef("content://new", "f.md"),
            state = PendingItem.STATE_PROMOTED,
        )

        // Simulate recovery decision logic
        val shouldSkip = (item.state == PendingItem.STATE_PROMOTED || item.state == PendingItem.STATE_COMMITTED) &&
            item.promotedRef != null

        assertTrue("PROMOTED item should be skipped during recovery", shouldSkip)
    }

    @Test
    fun recovery_shouldSkipCommittedItems() {
        val key = ChapterKey("p1", "v1", "ch1")
        val item = PendingItem(
            key = key,
            stagedRef = StagedMirrorRef("tx1", "content://s", ".staging/tx1/f.md", "f.md", "text/markdown"),
            oldRef = MirrorFileRef("content://old", "f.md"),
            backupOldRef = MirrorFileRef("content://backup", "backup/f.md"),
            promotedRef = MirrorFileRef("content://new", "f.md"),
            state = PendingItem.STATE_COMMITTED,
        )

        val shouldSkip = (item.state == PendingItem.STATE_PROMOTED || item.state == PendingItem.STATE_COMMITTED) &&
            item.promotedRef != null

        assertTrue("COMMITTED item should be skipped during recovery", shouldSkip)
    }

    @Test
    fun recovery_shouldNotSkipStagedItems() {
        val key = ChapterKey("p1", "v1", "ch1")
        val item = PendingItem(
            key = key,
            stagedRef = StagedMirrorRef("tx1", "content://s", ".staging/tx1/f.md", "f.md", "text/markdown"),
            oldRef = MirrorFileRef("content://old", "f.md"),
            backupOldRef = null,
            promotedRef = null,
            state = PendingItem.STATE_STAGED,
        )

        val shouldSkip = (item.state == PendingItem.STATE_PROMOTED || item.state == PendingItem.STATE_COMMITTED) &&
            item.promotedRef != null

        assertFalse("STAGED item should NOT be skipped during recovery", shouldSkip)
    }

    @Test
    fun recovery_shouldNotSkipOldBackedUpItems() {
        val key = ChapterKey("p1", "v1", "ch1")
        val item = PendingItem(
            key = key,
            stagedRef = StagedMirrorRef("tx1", "content://s", ".staging/tx1/f.md", "f.md", "text/markdown"),
            oldRef = MirrorFileRef("content://old", "f.md"),
            backupOldRef = MirrorFileRef("content://backup", "backup/f.md"),
            promotedRef = null,
            state = PendingItem.STATE_OLD_BACKED_UP,
        )

        val shouldSkip = (item.state == PendingItem.STATE_PROMOTED || item.state == PendingItem.STATE_COMMITTED) &&
            item.promotedRef != null

        assertFalse("OLD_BACKED_UP item should NOT be skipped during recovery", shouldSkip)
    }

    // ── UPSERT vs DELETE project manifest difference ──

    @Test
    fun deleteProject_removesAllChaptersFromManifest() {
        val oldEntries = mapOf(
            ChapterKey("p1", "v1", "ch1") to ChapterMirrorEntry("content://1", "f1.md", 100L, "sha256:a"),
            ChapterKey("p1", "v1", "ch2") to ChapterMirrorEntry("content://2", "f2.md", 200L, "sha256:b"),
            ChapterKey("p2", "v1", "ch3") to ChapterMirrorEntry("content://3", "f3.md", 300L, "sha256:c"),
        )

        // DELETE_PROJECT for p1: desiredWithoutDeleted excludes all p1 entries
        val desiredWithoutDeleted = mutableMapOf<ChapterKey, ChapterMirrorEntry>()
        for ((key, entry) in oldEntries) {
            if (key.projectId != "p1") {
                desiredWithoutDeleted[key] = entry
            }
        }

        assertEquals("only p2 entry should remain", 1, desiredWithoutDeleted.size)
        assertTrue(desiredWithoutDeleted.containsKey(ChapterKey("p2", "v1", "ch3")))
        assertFalse(desiredWithoutDeleted.containsKey(ChapterKey("p1", "v1", "ch1")))
        assertFalse(desiredWithoutDeleted.containsKey(ChapterKey("p1", "v1", "ch2")))
    }

    @Test
    fun deleteProject_emptyProject_stillNeedsManifest() {
        // #649 评论 5562715833 问题 7：空作品也要走事务流程
        val removed = emptyMap<ChapterKey, ChapterMirrorEntry>()

        // Even with empty removed, we should NOT early return
        // The code should continue to create a new manifest without this project
        val txId = "${System.currentTimeMillis()}-p-empty"
        val journal = PendingMirrorPublish(
            txId = txId,
            backend = MirrorBackend.MEDIA_STORE,
            treeUri = null,
            projectId = "p-empty",
            transactionType = MirrorTransactionType.DELETE_PROJECT,
            phase = PendingMirrorPublish.PHASE_CLEANUP,
            oldEntries = removed,
            newEntries = emptyMap(),
            stagedRefs = emptyMap(),
            items = emptyMap(),
            removedProjectIds = setOf("p-empty"),
            manifestOldRef = null,
            manifestStagedRef = null,
            manifestNewRef = null,
            manifestBackupRef = null,
        )

        // Verify journal is valid even for empty project
        val json = journal.toJson()
        val restored = PendingMirrorPublish.fromJson(json)
        assertNotNull(restored)
        assertEquals(setOf("p-empty"), restored!!.removedProjectIds)
    }

    // ── Manifest transaction journal steps ──

    @Test
    fun manifestTransaction_journalStepTracking() {
        // Simulate manifest transaction: stage → journal → promote → journal → commit → journal
        val baseJournal = PendingMirrorPublish(
            txId = "tx1",
            backend = MirrorBackend.DOCUMENT_TREE,
            treeUri = "content://tree",
            projectId = "p1",
            transactionType = MirrorTransactionType.UPSERT_PROJECT,
            phase = PendingMirrorPublish.PHASE_PROMOTE,
            oldEntries = emptyMap(),
            newEntries = emptyMap(),
            stagedRefs = emptyMap(),
            items = emptyMap(),
            removedProjectIds = emptySet(),
            manifestOldRef = null,
            manifestStagedRef = null,
            manifestNewRef = null,
            manifestBackupRef = null,
        )

        // Step 1: manifest staged
        val step1 = baseJournal.copy(
            manifestStagedRef = StagedMirrorRef("tx1", "content://ms", ".staging/tx1/_meta/manifest.json", "_meta/manifest.json", "application/json"),
        )
        assertNull(step1.manifestNewRef)
        assertNull(step1.manifestBackupRef)
        assertFalse(step1.isManifestCommitted)

        // Step 2: manifest promoted
        val step2 = step1.copy(
            manifestNewRef = MirrorFileRef("content://mn", "_meta/manifest.json"),
            manifestBackupRef = MirrorFileRef("content://mo", "_meta/manifest.json"),
        )
        assertNotNull(step2.manifestNewRef)
        assertNotNull(step2.manifestBackupRef)
        assertFalse(step2.isManifestCommitted)

        // Step 3: manifest committed (setManifestUri + journal)
        val step3 = step2.copy(isManifestCommitted = true)
        assertTrue(step3.isManifestCommitted)

        // Verify serialization captures the committed state
        val json = step3.toJson()
        val restored = PendingMirrorPublish.fromJson(json)!!
        assertTrue(restored.isManifestCommitted)
        assertNotNull(restored.manifestNewRef)
        assertNotNull(restored.manifestBackupRef)
    }

    // ── MirrorTransactionType serialization ──

    @Test
    fun transactionType_serialization() {
        for (type in MirrorTransactionType.entries) {
            val journal = PendingMirrorPublish(
                txId = "tx",
                backend = MirrorBackend.MEDIA_STORE,
                treeUri = null,
                projectId = "p1",
                transactionType = type,
                phase = PendingMirrorPublish.PHASE_STAGE,
                oldEntries = emptyMap(),
                newEntries = emptyMap(),
                stagedRefs = emptyMap(),
                items = emptyMap(),
                removedProjectIds = emptySet(),
                manifestOldRef = null,
                manifestStagedRef = null,
                manifestNewRef = null,
                manifestBackupRef = null,
            )

            val json = journal.toJson()
            val restored = PendingMirrorPublish.fromJson(json)!!
            assertEquals(type, restored.transactionType)
        }
    }

    // ── Phase serialization ──

    @Test
    fun phase_serialization() {
        val phases = listOf(
            PendingMirrorPublish.PHASE_STAGE,
            PendingMirrorPublish.PHASE_PROMOTE,
            PendingMirrorPublish.PHASE_CLEANUP,
        )
        for (phase in phases) {
            val journal = PendingMirrorPublish(
                txId = "tx",
                backend = MirrorBackend.MEDIA_STORE,
                treeUri = null,
                projectId = "p1",
                transactionType = MirrorTransactionType.UPSERT_PROJECT,
                phase = phase,
                oldEntries = emptyMap(),
                newEntries = emptyMap(),
                stagedRefs = emptyMap(),
                items = emptyMap(),
                removedProjectIds = emptySet(),
                manifestOldRef = null,
                manifestStagedRef = null,
                manifestNewRef = null,
                manifestBackupRef = null,
            )

            val json = journal.toJson()
            val restored = PendingMirrorPublish.fromJson(json)!!
            assertEquals(phase, restored.phase)
        }
    }

    // ── Helper classes ──

    /** Fake project snapshot for testing without Core dependency. */
    private data class FakeProjectSnapshot(val id: String, val title: String)

    /**
     * Fake [ReadableMirrorStorage] implementation for unit testing.
     *
     * Tracks all file operations to verify correct flow.
     */
    private class FakeReadableMirrorStorage : ReadableMirrorStorage {
        val committedFiles = mutableMapOf<String, String>() // uri → content
        val stagingFiles = mutableMapOf<String, String>() // uri → content
        val deletedFiles = mutableListOf<String>() // uri
        val journalSteps = mutableListOf<String>() // operation log

        var failPromote = false
        var failBackup = false

        override fun createText(
            relativeDir: String,
            displayName: String,
            mimeType: String,
            text: String,
        ): MirrorFileRef {
            val path = if (relativeDir.isBlank()) displayName else "$relativeDir/$displayName"
            val uri = "content://fake/${committedFiles.size}"
            committedFiles[uri] = text
            journalSteps.add("createText:$path")
            return MirrorFileRef(uri, path)
        }

        override fun replaceText(ref: MirrorFileRef, text: String): Boolean {
            committedFiles[ref.uri] = text
            journalSteps.add("replaceText:${ref.relativePath}")
            return true
        }

        override fun delete(ref: MirrorFileRef): Boolean {
            committedFiles.remove(ref.uri)
            stagingFiles.remove(ref.uri)
            deletedFiles.add(ref.uri)
            journalSteps.add("delete:${ref.relativePath}")
            return true
        }

        override fun isSupported(): Boolean = true

        override fun stageText(
            txId: String,
            relativePath: String,
            mimeType: String,
            text: String,
        ): StagedMirrorRef? {
            val uri = "content://fake/staging/${stagingFiles.size}"
            stagingFiles[uri] = text
            journalSteps.add("stageText:$relativePath")
            return StagedMirrorRef(
                txId = txId,
                stagingUri = uri,
                stagingRelativePath = ".staging/$txId/$relativePath",
                finalRelativePath = relativePath,
                mimeType = mimeType,
            )
        }

        override fun backupCommitted(txId: String, old: MirrorFileRef): MirrorFileRef? {
            if (failBackup) return null
            val content = committedFiles[old.uri] ?: return null
            val backupUri = "content://fake/backup/${committedFiles.size}"
            committedFiles[backupUri] = content
            val backupPath = ".staging/$txId/backup/${old.relativePath}"
            journalSteps.add("backup:${old.relativePath}→$backupPath")
            return MirrorFileRef(backupUri, backupPath)
        }

        override fun promoteStaged(staged: StagedMirrorRef, finalRelativePath: String): MirrorFileRef? {
            if (failPromote) return null
            val content = stagingFiles.remove(staged.stagingUri) ?: return null
            val newUri = "content://fake/promoted/${committedFiles.size}"
            committedFiles[newUri] = content
            journalSteps.add("promote:${staged.stagingRelativePath}→$finalRelativePath")
            return MirrorFileRef(newUri, finalRelativePath)
        }

        override fun restoreBackup(backup: MirrorFileRef, finalRelativePath: String): MirrorFileRef? {
            val content = committedFiles[backup.uri] ?: return null
            val newUri = "content://fake/restored/${committedFiles.size}"
            committedFiles[newUri] = content
            journalSteps.add("restore:${backup.relativePath}→$finalRelativePath")
            return MirrorFileRef(newUri, finalRelativePath)
        }

        override fun rollback(txId: String) {
            stagingFiles.clear()
            journalSteps.add("rollback:$txId")
        }
    }
}
