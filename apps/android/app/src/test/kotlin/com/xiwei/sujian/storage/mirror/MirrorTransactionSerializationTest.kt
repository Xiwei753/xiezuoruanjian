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
 * #649 评论 5562715833：镜像事务序列化单元测试。
 *
 * 覆盖：
 * - manifest transaction journal 步骤
 * - MirrorTransactionType / Phase 序列化
 * - affectedProjectIds / manifestContentHash 序列化
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class MirrorTransactionSerializationTest {
    // ── Manifest transaction journal steps ──

    @Test
    fun manifestTransaction_journalStepTracking() {
        // Simulate manifest transaction: stage → journal → promote → journal → commit → journal
        val baseJournal =
            PendingMirrorPublish(
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
        val step1 =
            baseJournal.copy(
                manifestStagedRef =
                    StagedMirrorRef(
                        "tx1",
                        "content://ms",
                        ".staging/tx1/_meta/manifest.json",
                        MANIFEST_JSON_PATH,
                        "application/json",
                    ),
            )
        assertNull(step1.manifestNewRef)
        assertNull(step1.manifestBackupRef)
        assertFalse(step1.isManifestCommitted)

        // Step 2: manifest promoted
        val step2 =
            step1.copy(
                manifestNewRef = MirrorFileRef("content://mn", MANIFEST_JSON_PATH),
                manifestBackupRef = MirrorFileRef("content://mo", MANIFEST_JSON_PATH),
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
            val journal =
                PendingMirrorPublish(
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
        val phases =
            listOf(
                PendingMirrorPublish.PHASE_STAGE,
                PendingMirrorPublish.PHASE_PROMOTE,
                PendingMirrorPublish.PHASE_CLEANUP,
            )
        for (phase in phases) {
            val journal =
                PendingMirrorPublish(
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

    // ── #649 评论 5564820566：PHASE_ROLLBACK 序列化 ──

    @Test
    fun phase_serialization_includesRollback() {
        val phases =
            listOf(
                PendingMirrorPublish.PHASE_STAGE,
                PendingMirrorPublish.PHASE_PROMOTE,
                PendingMirrorPublish.PHASE_CLEANUP,
                PendingMirrorPublish.PHASE_ROLLBACK,
            )
        for (phase in phases) {
            val journal =
                PendingMirrorPublish(
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

    // ── #649 评论 5564820566 问题 5：affectedProjectIds 序列化 ──

    @Test
    fun pendingMirrorPublish_jsonRoundTrip_affectedProjectIds() {
        val journal =
            PendingMirrorPublish(
                txId = "tx1",
                backend = MirrorBackend.MEDIA_STORE,
                treeUri = null,
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
                affectedProjectIds = setOf("p1", "p2"),
            )

        val json = journal.toJson()
        val restored = PendingMirrorPublish.fromJson(json)!!
        assertEquals(setOf("p1", "p2"), restored.affectedProjectIds)
    }

    @Test
    fun pendingMirrorPublish_jsonRoundTrip_emptyAffectedProjectIds() {
        val journal =
            PendingMirrorPublish(
                txId = "tx1",
                backend = MirrorBackend.MEDIA_STORE,
                treeUri = null,
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

        val json = journal.toJson()
        val restored = PendingMirrorPublish.fromJson(json)!!
        assertTrue(restored.affectedProjectIds.isEmpty())
    }

    @Test
    fun pendingMirrorPublish_jsonRoundTrip_manifestContentHash() {
        val journal =
            PendingMirrorPublish(
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
                manifestNewContentHash = "sha256:new_manifest_hash",
                manifestOldContentHash = "sha256:old_manifest_hash",
            )

        val json = journal.toJson()
        val restored = PendingMirrorPublish.fromJson(json)!!
        assertEquals("sha256:new_manifest_hash", restored.manifestNewContentHash)
        assertEquals("sha256:old_manifest_hash", restored.manifestOldContentHash)
    }

    @Test
    fun pendingMirrorPublish_jsonRoundTrip_nullManifestContentHash() {
        val journal =
            PendingMirrorPublish(
                txId = "tx1",
                backend = MirrorBackend.MEDIA_STORE,
                treeUri = null,
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

        val json = journal.toJson()
        val restored = PendingMirrorPublish.fromJson(json)!!
        assertNull(restored.manifestNewContentHash)
        assertNull(restored.manifestOldContentHash)
    }
}
