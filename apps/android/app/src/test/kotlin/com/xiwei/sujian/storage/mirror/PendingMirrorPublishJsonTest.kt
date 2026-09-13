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
 * #649 评论 5562715833：PendingMirrorPublish JSON 序列化/反序列化单元测试。
 *
 * 覆盖：
 * - upsertProject / deleteProject / nullManifestFields round-trip
 * - 非法 JSON 反序列化返回 null
 * - PendingItem 各状态（含 rollback 新状态）的 JSON round-trip
 * - oldContentHash 序列化
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class PendingMirrorPublishJsonTest {
    // ── PendingMirrorPublish JSON round-trip ──

    @Test
    fun pendingMirrorPublish_jsonRoundTrip_upsertProject() {
        val (key1, key2, journal) = buildUpsertProjectFixture()
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
        assertEquals(CONTENT_MANIFEST_NEW, restored.manifestNewRef!!.uri)
        assertNotNull(restored.manifestBackupRef)
        assertEquals(CONTENT_MANIFEST_BACKUP, restored.manifestBackupRef!!.uri)

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
        val journal =
            PendingMirrorPublish(
                txId = "tx-del",
                backend = MirrorBackend.MEDIA_STORE,
                treeUri = null,
                projectId = PROJECT_DEL,
                transactionType = MirrorTransactionType.DELETE_PROJECT,
                phase = PendingMirrorPublish.PHASE_CLEANUP,
                oldEntries =
                    mapOf(
                        ChapterKey(PROJECT_DEL, "v1", "ch1") to
                            ChapterMirrorEntry("content://old/1", "作品/Del/V/Ch.md", 100L, "sha256:x"),
                    ),
                newEntries = emptyMap(),
                stagedRefs = emptyMap(),
                items = emptyMap(),
                removedProjectIds = setOf(PROJECT_DEL),
                manifestOldRef = null,
                manifestStagedRef = null,
                manifestNewRef = MirrorFileRef(CONTENT_MANIFEST_NEW, MANIFEST_JSON_PATH),
                manifestBackupRef = MirrorFileRef(CONTENT_MANIFEST_BACKUP, MANIFEST_JSON_PATH),
                isManifestCommitted = true,
            )

        val json = journal.toJson()
        val restored = PendingMirrorPublish.fromJson(json)

        assertNotNull(restored)
        restored!!
        assertEquals(MirrorTransactionType.DELETE_PROJECT, restored.transactionType)
        assertEquals(PendingMirrorPublish.PHASE_CLEANUP, restored.phase)
        assertTrue(restored.isManifestCommitted)
        assertEquals(setOf(PROJECT_DEL), restored.removedProjectIds)
        assertTrue(restored.newEntries.isEmpty())
        assertNull(restored.manifestOldRef)
        assertNull(restored.manifestStagedRef)
        assertNotNull(restored.manifestNewRef)
        assertNotNull(restored.manifestBackupRef)
    }

    @Test
    fun pendingMirrorPublish_jsonRoundTrip_nullManifestFields() {
        val journal =
            PendingMirrorPublish(
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

    // ── PendingItem JSON serialization ──

    @Test
    fun pendingItem_jsonRoundTrip_allStates() {
        // #649 评论 5565067997 修复 1：新增 STATE_BACKUP_READY / STATE_OLD_VACATED，
        // 旧 STATE_OLD_BACKED_UP 反序列化时映射到 STATE_BACKUP_READY（normalizeState）。
        val states =
            listOf(
                PendingItem.STATE_STAGED,
                PendingItem.STATE_BACKUP_READY,
                PendingItem.STATE_OLD_VACATED,
                // 旧状态，round-trip 后映射到 STATE_BACKUP_READY
                PendingItem.STATE_OLD_BACKED_UP,
                PendingItem.STATE_PROMOTED,
                PendingItem.STATE_COMMITTED,
            )
        val key = ChapterKey("p1", "v1", "ch1")
        val staged = StagedMirrorRef("tx1", CONTENT_S, STAGING_TX1_FMD, "f.md", MIME_TYPE_MARKDOWN)

        for (state in states) {
            val item =
                PendingItem(
                    key = key,
                    stagedRef = staged,
                    oldRef = MirrorFileRef(CONTENT_OLD, "f.md"),
                    backupOldRef =
                        if (state != PendingItem.STATE_STAGED) {
                            MirrorFileRef(
                                CONTENT_BACKUP,
                                BACKUP_FMD,
                            )
                        } else {
                            null
                        },
                    promotedRef =
                        if (state == PendingItem.STATE_PROMOTED || state == PendingItem.STATE_COMMITTED) {
                            MirrorFileRef(
                                CONTENT_NEW,
                                "f.md",
                            )
                        } else {
                            null
                        },
                    state = state,
                )

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

            // #649 评论 5565067997 修复 1：STATE_OLD_BACKED_UP 反序列化映射到 STATE_BACKUP_READY
            val expectedState = PendingItem.normalizeState(state)
            assertEquals(
                "State $state should round-trip (normalized: $expectedState)",
                expectedState,
                restoredItem.state,
            )
            assertEquals("stagedRef should round-trip", staged.stagingUri, restoredItem.stagedRef?.stagingUri)
            assertEquals("oldRef should round-trip", CONTENT_OLD, restoredItem.oldRef?.uri)
        }
    }

    // ── #649 评论 5564820566 问题 2：rollback 新状态 ──

    @Test
    fun pendingItem_jsonRoundTrip_rollbackStates() {
        val states =
            listOf(
                PendingItem.STATE_ROLLBACK_NEW_REMOVED,
                PendingItem.STATE_ROLLBACK_OLD_RESTORED,
            )
        val key = ChapterKey("p1", "v1", "ch1")
        val staged = StagedMirrorRef("tx1", CONTENT_S, STAGING_TX1_FMD, "f.md", MIME_TYPE_MARKDOWN)

        for (state in states) {
            val item =
                PendingItem(
                    key = key,
                    stagedRef = staged,
                    oldRef = MirrorFileRef(CONTENT_OLD, "f.md"),
                    backupOldRef = MirrorFileRef(CONTENT_BACKUP, BACKUP_FMD),
                    promotedRef = MirrorFileRef(CONTENT_NEW, "f.md"),
                    state = state,
                )

            val journal =
                PendingMirrorPublish(
                    txId = "tx1",
                    backend = MirrorBackend.MEDIA_STORE,
                    treeUri = null,
                    projectId = "p1",
                    transactionType = MirrorTransactionType.UPSERT_PROJECT,
                    phase = PendingMirrorPublish.PHASE_ROLLBACK,
                    oldEntries = emptyMap(),
                    newEntries = emptyMap(),
                    stagedRefs = emptyMap(),
                    items = mapOf(key to item),
                    removedProjectIds = emptySet(),
                    manifestOldRef = null,
                    manifestStagedRef = null,
                    manifestNewRef = null,
                    manifestBackupRef = MirrorFileRef(CONTENT_MANIFEST_BACKUP, MANIFEST_JSON_PATH),
                )

            val json = journal.toJson()
            val restored = PendingMirrorPublish.fromJson(json)!!
            val restoredItem = restored.items[key]!!

            assertEquals("Rollback state $state should round-trip", state, restoredItem.state)
            assertEquals(PendingMirrorPublish.PHASE_ROLLBACK, restored.phase)
        }
    }

    // ── #649 评论 5566303837 问题 2：oldContentHash 序列化 ──

    @Test
    fun pendingItem_jsonRoundTrip_oldContentHash() {
        val key = ChapterKey("p1", "v1", "ch1")
        val staged = StagedMirrorRef("tx1", CONTENT_S, STAGING_TX1_FMD, "f.md", MIME_TYPE_MARKDOWN)
        val item =
            PendingItem(
                key = key,
                stagedRef = staged,
                oldRef = MirrorFileRef(CONTENT_OLD, "f.md"),
                backupOldRef = MirrorFileRef(CONTENT_BACKUP, BACKUP_FMD),
                promotedRef = null,
                state = PendingItem.STATE_OLD_VACATED,
                oldContentHash = "sha256:abc123",
            )

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
        assertEquals("sha256:abc123", restoredItem.oldContentHash)
    }
}
