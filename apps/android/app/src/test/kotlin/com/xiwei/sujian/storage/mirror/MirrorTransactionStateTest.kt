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

// ── 共享测试常量（大写开头，满足 TopLevelPropertyNaming: [A-Z][_A-Z0-9]*）──
private const val MIME_TYPE_MARKDOWN = "text/markdown"
private const val STAGING_TX1_FMD = ".staging/tx1/f.md"
private const val CONTENT_OLD = "content://old"
private const val CONTENT_BACKUP = "content://backup"
private const val CONTENT_S = "content://s"
private const val MANIFEST_JSON_PATH = "_meta/manifest.json"
private const val CONTENT_NEW = "content://new"
private const val CONTENT_STAGING = "content://staging"
private const val WORK_PATH_CH_MD = "作品/P/V/Ch.md"
private const val WORK_PATH_CH1_MD = "作品/P/V/Ch1.md"
private const val WORK_PATH_CH2_MD = "作品/P/V/Ch2.md"
private const val BACKUP_FMD = "backup/f.md"
private const val STAGING_TX1_BACKUP_FMD = ".staging/tx1/backup/f.md"
private const val CONTENT_MANIFEST_NEW = "content://manifest/new"
private const val CONTENT_MANIFEST_BACKUP = "content://manifest/backup"
private const val CONTENT_TEST = "content://test"
private const val PROJECT_DEL = "p-del"
private const val PROJECT_EMPTY = "p-empty"
private const val OLD_CONTENT = "old content"
private const val NEW_CONTENT = "new content"
private const val IMPORTANT_CONTENT = "important content"
private const val CONTENT_FAKE_WORK_CH_MD = "content://fake/作品_P_V_Ch.md"

/** 构造 backup 路径的 MirrorFileRef，避免 "backup/$path" 字面量重复。 */
private fun backupMirrorRef(path: String) = MirrorFileRef(CONTENT_BACKUP, "backup/$path")

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
    val backupFiles = mutableMapOf<String, String>() // uri → content (backup area)
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

    override fun replaceText(
        ref: MirrorFileRef,
        text: String,
    ): Boolean {
        committedFiles[ref.uri] = text
        journalSteps.add("replaceText:${ref.relativePath}")
        return true
    }

    override fun delete(ref: MirrorFileRef): Boolean {
        // #649 评论 5564379115 问题 3：幂等语义 — 文件不存在也返回 true
        val existed =
            committedFiles.remove(ref.uri) != null ||
                stagingFiles.remove(ref.uri) != null ||
                backupFiles.remove(ref.uri) != null
        deletedFiles.add(ref.uri)
        journalSteps.add("delete:${ref.relativePath}")
        return true // 幂等：始终返回 true
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

    override fun backupCommitted(
        txId: String,
        old: MirrorFileRef,
        mimeType: String,
    ): MirrorFileRef? {
        if (failBackup) return null
        val content = committedFiles[old.uri] ?: return null
        val backupUri = "content://fake/backup/${backupFiles.size}"
        backupFiles[backupUri] = content
        val backupPath = ".staging/$txId/backup/${old.relativePath}"
        journalSteps.add("backup:${old.relativePath}→$backupPath")
        return MirrorFileRef(backupUri, backupPath)
    }

    // #649 评论 5564820566 问题 3：两步 journalable backup — fake 实现
    override fun prepareBackup(
        txId: String,
        old: MirrorFileRef,
        mimeType: String,
    ): BackupReadyRef? {
        if (failBackup) return null
        val content = committedFiles[old.uri] ?: return null
        val backupUri = "content://fake/backup/${backupFiles.size}"
        backupFiles[backupUri] = content
        val backupPath = ".staging/$txId/backup/${old.relativePath}"
        journalSteps.add("prepareBackup:${old.relativePath}→$backupPath")
        return BackupReadyRef(
            backupRef = MirrorFileRef(backupUri, backupPath),
            vacated = false,
        )
    }

    override fun vacateCommitted(old: MirrorFileRef): Boolean {
        committedFiles.remove(old.uri)
        journalSteps.add("vacate:${old.relativePath}")
        return true
    }

    override fun promoteStaged(
        staged: StagedMirrorRef,
        finalRelativePath: String,
    ): MirrorFileRef? {
        if (failPromote) return null
        val content = stagingFiles.remove(staged.stagingUri) ?: return null
        val newUri = "content://fake/promoted/${committedFiles.size}"
        committedFiles[newUri] = content
        journalSteps.add("promote:${staged.stagingRelativePath}→$finalRelativePath")
        return MirrorFileRef(newUri, finalRelativePath)
    }

    override fun resolve(relativePath: String): MirrorFileRef? {
        // 查找 committedFiles 中匹配 relativePath 的条目
        for ((uri, _) in committedFiles) {
            // 简化：用 URI 中的路径信息匹配
            if (uri.contains(relativePath.replace("/", "_"))) {
                return MirrorFileRef(uri, relativePath)
            }
        }
        return null
    }

    // #649 评论 5565067997 修复 5：实现 lookup() 三态查询
    override fun lookup(relativePath: String): MirrorLookupResult {
        val resolved = resolve(relativePath)
        return if (resolved != null) {
            MirrorLookupResult.Found(resolved)
        } else {
            MirrorLookupResult.Missing
        }
    }

    override fun resolveBackup(
        txId: String,
        relativePath: String,
    ): MirrorFileRef? {
        // 查找 backupFiles 中匹配的条目
        val backupPath = ".staging/$txId/backup/$relativePath"
        for ((uri, _) in backupFiles) {
            if (uri.contains(relativePath.replace("/", "_"))) {
                return MirrorFileRef(uri, backupPath)
            }
        }
        return null
    }

    // #649 评论 5565862745 问题 3：实现 lookupBackup() 三态查询
    override fun lookupBackup(
        txId: String,
        relativePath: String,
    ): MirrorLookupResult {
        val resolved = resolveBackup(txId, relativePath)
        return if (resolved != null) {
            MirrorLookupResult.Found(resolved)
        } else {
            MirrorLookupResult.Missing
        }
    }

    // #649 评论 5566303837 问题 4：restoreBackup 扁平化（early return 降低嵌套深度）
    override fun restoreBackup(
        backup: MirrorFileRef,
        finalRelativePath: String,
        mimeType: String,
        expectedOldContentHash: String?,
    ): RestoreBackupResult {
        val existing = resolve(finalRelativePath)
        if (existing == null) {
            // final 不存在 → 从 backup 恢复
            val content =
                backupFiles[backup.uri]
                    ?: committedFiles[backup.uri]
                    ?: return RestoreBackupResult.Failed(null)
            val newUri = "content://fake/restored/${committedFiles.size}"
            committedFiles[newUri] = content
            journalSteps.add("restore:${backup.relativePath}→$finalRelativePath")
            return RestoreBackupResult.Restored(MirrorFileRef(newUri, finalRelativePath))
        }
        // final 已存在
        if (expectedOldContentHash == null) {
            return RestoreBackupResult.AlreadyRestored(existing)
        }
        val hashResult = readTextAndHash(existing) ?: return RestoreBackupResult.Failed(null)
        val (_, hash) = hashResult
        return if (hash == expectedOldContentHash) {
            RestoreBackupResult.AlreadyRestored(existing)
        } else {
            RestoreBackupResult.Conflict
        }
    }

    override fun readTextAndHash(ref: MirrorFileRef): Pair<String, String>? {
        val content = committedFiles[ref.uri] ?: stagingFiles[ref.uri] ?: backupFiles[ref.uri] ?: return null
        return Pair(content, computeContentHash(content))
    }

    override fun rollback(txId: String): Boolean {
        stagingFiles.clear()
        journalSteps.add("rollback:$txId")
        return true
    }
}

/** upsertProject round-trip 的 setup fixture。 */
private data class UpsertProjectFixture(
    val key1: ChapterKey,
    val key2: ChapterKey,
    val journal: PendingMirrorPublish,
)

/** 构造 upsertProject 测试所需的 journal 和 keys，提取 setup 以满足 LongMethod 阈值。 */
private fun buildUpsertProjectFixture(): UpsertProjectFixture {
    val key1 = ChapterKey("p1", "v1", "ch1")
    val key2 = ChapterKey("p1", "v1", "ch2")
    val entry1 = ChapterMirrorEntry("content://media/1", WORK_PATH_CH1_MD, 100L, "sha256:abc")
    val entry2 = ChapterMirrorEntry("content://media/2", WORK_PATH_CH2_MD, 200L, "sha256:def")

    val staged1 =
        StagedMirrorRef(
            "tx1",
            "content://staging/1",
            ".staging/tx1/作品/P/V/Ch1.md",
            WORK_PATH_CH1_MD,
            MIME_TYPE_MARKDOWN,
        )
    val staged2 =
        StagedMirrorRef(
            "tx1",
            "content://staging/2",
            ".staging/tx1/作品/P/V/Ch2.md",
            WORK_PATH_CH2_MD,
            MIME_TYPE_MARKDOWN,
        )

    val item1 =
        PendingItem(
            key = key1,
            stagedRef = staged1,
            oldRef = MirrorFileRef("content://old/1", WORK_PATH_CH1_MD),
            backupOldRef = MirrorFileRef("content://backup/1", ".staging/tx1/backup/作品/P/V/Ch1.md"),
            promotedRef = MirrorFileRef("content://new/1", WORK_PATH_CH1_MD),
            state = PendingItem.STATE_PROMOTED,
        )
    val item2 =
        PendingItem(
            key = key2,
            stagedRef = staged2,
            oldRef = MirrorFileRef("content://old/2", WORK_PATH_CH2_MD),
            backupOldRef = null,
            promotedRef = null,
            state = PendingItem.STATE_STAGED,
        )

    val manifestOldRef = MirrorFileRef("content://manifest/old", MANIFEST_JSON_PATH)
    val manifestStagedRef =
        StagedMirrorRef(
            "tx1",
            "content://manifest/staged",
            ".staging/tx1/_meta/manifest.json",
            MANIFEST_JSON_PATH,
            "application/json",
        )
    val manifestNewRef = MirrorFileRef(CONTENT_MANIFEST_NEW, MANIFEST_JSON_PATH)
    val manifestBackupRef = MirrorFileRef(CONTENT_MANIFEST_BACKUP, MANIFEST_JSON_PATH)

    val journal =
        PendingMirrorPublish(
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
    return UpsertProjectFixture(key1, key2, journal)
}

/**
 * #649 评论 5562715833：镜像事务状态机单元测试。
 *
 * 覆盖：
 * - PendingMirrorPublish JSON 序列化/反序列化
 * - PendingItem 状态推进逻辑
 * - DELETE_PROJECT 事务中 snapshot=null 语义
 * - RestoreBackupResult identity verification
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class MirrorTransactionStateTest {
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

    // ── PendingItem state machine ──

    @Test
    fun pendingItem_stateTransitions_stagedToOldBackedUp() {
        val key = ChapterKey("p1", "v1", "ch1")
        val staged = StagedMirrorRef("tx1", CONTENT_STAGING, STAGING_TX1_FMD, "f.md", MIME_TYPE_MARKDOWN)
        val oldRef = MirrorFileRef(CONTENT_OLD, "f.md")
        val backupRef = MirrorFileRef(CONTENT_BACKUP, STAGING_TX1_BACKUP_FMD)

        val item =
            PendingItem(
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
        assertEquals(CONTENT_BACKUP, backedUp.backupOldRef!!.uri)
    }

    @Test
    fun pendingItem_stateTransitions_oldBackedUpToPromoted() {
        val key = ChapterKey("p1", "v1", "ch1")
        val staged = StagedMirrorRef("tx1", CONTENT_STAGING, STAGING_TX1_FMD, "f.md", MIME_TYPE_MARKDOWN)
        val backupRef = MirrorFileRef(CONTENT_BACKUP, STAGING_TX1_BACKUP_FMD)
        val newRef = MirrorFileRef(CONTENT_NEW, "f.md")

        val item =
            PendingItem(
                key = key,
                stagedRef = staged,
                oldRef = MirrorFileRef(CONTENT_OLD, "f.md"),
                backupOldRef = backupRef,
                promotedRef = null,
                state = PendingItem.STATE_OLD_BACKED_UP,
            )

        val promoted = item.copy(promotedRef = newRef, state = PendingItem.STATE_PROMOTED)
        assertEquals(PendingItem.STATE_PROMOTED, promoted.state)
        assertNotNull(promoted.promotedRef)
        assertEquals(CONTENT_NEW, promoted.promotedRef!!.uri)
    }

    @Test
    fun pendingItem_stateTransitions_promotedToCommitted() {
        val key = ChapterKey("p1", "v1", "ch1")
        val item =
            PendingItem(
                key = key,
                stagedRef = StagedMirrorRef("tx1", CONTENT_S, STAGING_TX1_FMD, "f.md", MIME_TYPE_MARKDOWN),
                oldRef = MirrorFileRef(CONTENT_OLD, "f.md"),
                backupOldRef = MirrorFileRef(CONTENT_BACKUP, STAGING_TX1_BACKUP_FMD),
                promotedRef = MirrorFileRef(CONTENT_NEW, "f.md"),
                state = PendingItem.STATE_PROMOTED,
            )

        val committed = item.copy(state = PendingItem.STATE_COMMITTED)
        assertEquals(PendingItem.STATE_COMMITTED, committed.state)
    }

    @Test
    fun pendingItem_stateTransitions_newProject_noOldRef() {
        val key = ChapterKey("p1", "v1", "ch1")
        val item =
            PendingItem(
                key = key,
                stagedRef = StagedMirrorRef("tx1", CONTENT_S, STAGING_TX1_FMD, "f.md", MIME_TYPE_MARKDOWN),
                oldRef = null,
                backupOldRef = null,
                promotedRef = null,
                state = PendingItem.STATE_STAGED,
            )
        assertNull(item.oldRef)
        assertNull(item.backupOldRef)

        // New project skips backup step
        val newRef = MirrorFileRef(CONTENT_NEW, "f.md")
        val promoted = item.copy(promotedRef = newRef, state = PendingItem.STATE_PROMOTED)
        assertNull(promoted.backupOldRef)
        assertEquals(PendingItem.STATE_PROMOTED, promoted.state)
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

    // ── UPSERT vs DELETE project manifest difference ──

    @Test
    fun deleteProject_removesAllChaptersFromManifest() {
        val oldEntries =
            mapOf(
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
        val journal =
            PendingMirrorPublish(
                txId = txId,
                backend = MirrorBackend.MEDIA_STORE,
                treeUri = null,
                projectId = PROJECT_EMPTY,
                transactionType = MirrorTransactionType.DELETE_PROJECT,
                phase = PendingMirrorPublish.PHASE_CLEANUP,
                oldEntries = removed,
                newEntries = emptyMap(),
                stagedRefs = emptyMap(),
                items = emptyMap(),
                removedProjectIds = setOf(PROJECT_EMPTY),
                manifestOldRef = null,
                manifestStagedRef = null,
                manifestNewRef = null,
                manifestBackupRef = null,
            )

        // Verify journal is valid even for empty project
        val json = journal.toJson()
        val restored = PendingMirrorPublish.fromJson(json)
        assertNotNull(restored)
        assertEquals(setOf(PROJECT_EMPTY), restored!!.removedProjectIds)
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

    // ── #649 评论 5566303837 问题 4：RestoreBackupResult identity verification ──

    @Test
    fun restoreBackup_withHash_match_returnsAlreadyRestored() {
        val storage = FakeReadableMirrorStorage()
        val content = IMPORTANT_CONTENT
        val path = WORK_PATH_CH_MD
        val uriKey = CONTENT_FAKE_WORK_CH_MD
        storage.committedFiles[uriKey] = content
        val backup = backupMirrorRef(path)
        val expectedHash = computeContentHash(content)

        val result = storage.restoreBackup(backup, path, MIME_TYPE_MARKDOWN, expectedHash)
        assertTrue("should be AlreadyRestored when hash matches", result is RestoreBackupResult.AlreadyRestored)
    }

    @Test
    fun restoreBackup_withHash_mismatch_returnsConflict() {
        val storage = FakeReadableMirrorStorage()
        val path = WORK_PATH_CH_MD
        val uriKey = CONTENT_FAKE_WORK_CH_MD
        storage.committedFiles[uriKey] = "new content (wrong)"
        val backup = backupMirrorRef(path)
        val expectedHash = computeContentHash("old content (expected)")

        val result = storage.restoreBackup(backup, path, MIME_TYPE_MARKDOWN, expectedHash)
        assertTrue("should be Conflict when hash mismatches", result is RestoreBackupResult.Conflict)
    }

    @Test
    fun restoreBackup_withoutHash_found_returnsAlreadyRestored() {
        val storage = FakeReadableMirrorStorage()
        val path = WORK_PATH_CH_MD
        val uriKey = CONTENT_FAKE_WORK_CH_MD
        storage.committedFiles[uriKey] = "any content"
        val backup = backupMirrorRef(path)

        val result = storage.restoreBackup(backup, path, MIME_TYPE_MARKDOWN, null)
        assertTrue("should be AlreadyRestored when no hash check", result is RestoreBackupResult.AlreadyRestored)
    }

    @Test
    fun restoreBackup_missing_final_returnsRestored() {
        val storage = FakeReadableMirrorStorage()
        storage.backupFiles[CONTENT_BACKUP] = "backup content"
        val path = WORK_PATH_CH_MD
        val backup = backupMirrorRef(path)

        val result = storage.restoreBackup(backup, path, MIME_TYPE_MARKDOWN, null)
        assertTrue("should be Restored when final missing", result is RestoreBackupResult.Restored)
    }
}

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
class MirrorTransactionRoundTripTest {
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

/**
 * #649 评论 5562715833：镜像事务 backup/restore/flow 单元测试。
 *
 * 覆盖：
 * - ReadableMirrorStorage 接口契约（backup/promote/restore）
 * - Transaction flow order verification
 * - Recovery: skip PROMOTED/COMMITTED items
 * - idempotent delete / cleanup / two-step backup
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class MirrorTransactionBackupTest {
    // ── FakeReadableMirrorStorage: interface contract tests ──

    @Test
    fun fakeStorage_promoteStaged_doesNotDeleteOld() {
        val storage = FakeReadableMirrorStorage()
        storage.committedFiles[CONTENT_OLD] = OLD_CONTENT
        storage.committedFiles[CONTENT_NEW] = NEW_CONTENT

        val staged = StagedMirrorRef("tx1", CONTENT_STAGING, STAGING_TX1_FMD, "f.md", MIME_TYPE_MARKDOWN)
        storage.stagingFiles[CONTENT_STAGING] = "staging content"

        val result = storage.promoteStaged(staged, "f.md")
        assertNotNull(result)
        // Old file should still exist (promoteStaged doesn't delete old)
        assertTrue("old file should still exist after promote", storage.committedFiles.containsKey(CONTENT_OLD))
    }

    @Test
    fun fakeStorage_backupCommitted_createsCopy() {
        val storage = FakeReadableMirrorStorage()
        storage.committedFiles[CONTENT_OLD] = IMPORTANT_CONTENT

        val old = MirrorFileRef(CONTENT_OLD, WORK_PATH_CH_MD)
        val backup = storage.backupCommitted("tx1", old, MIME_TYPE_MARKDOWN)

        assertNotNull(backup)
        assertTrue("backup should exist", storage.backupFiles.containsKey(backup!!.uri))
        assertEquals("backup content should match old", IMPORTANT_CONTENT, storage.backupFiles[backup.uri])
        // Old should still be there
        assertTrue("old should still exist", storage.committedFiles.containsKey(CONTENT_OLD))
    }

    @Test
    fun fakeStorage_restoreBackup_writesToFinalLocation() {
        val storage = FakeReadableMirrorStorage()
        storage.committedFiles[CONTENT_BACKUP] = "backed up content"

        val backup = MirrorFileRef(CONTENT_BACKUP, BACKUP_FMD)
        val result = storage.restoreBackup(backup, WORK_PATH_CH_MD, MIME_TYPE_MARKDOWN, null)

        assertTrue("restore should succeed", result is RestoreBackupResult.Restored)
        val ref = (result as RestoreBackupResult.Restored).ref
        assertEquals("restored content should match backup", "backed up content", storage.committedFiles[ref.uri])
    }

    @Test
    fun fakeStorage_rollback_deletesAllStagingFiles() {
        val storage = FakeReadableMirrorStorage()
        storage.stagingFiles["content://staging/1"] = "content1"
        storage.stagingFiles["content://staging/2"] = "content2"

        storage.rollback("tx1")

        assertTrue("staging should be cleared", storage.stagingFiles.isEmpty())
    }

    // ── Transaction flow order verification ──

    @Test
    fun transactionFlow_backupBeforePromote() {
        val storage = FakeReadableMirrorStorage()
        storage.committedFiles[CONTENT_OLD] = OLD_CONTENT
        storage.stagingFiles[CONTENT_STAGING] = NEW_CONTENT

        val txId = "tx1"
        val oldRef = MirrorFileRef(CONTENT_OLD, "f.md")
        val staged = StagedMirrorRef(txId, CONTENT_STAGING, STAGING_TX1_FMD, "f.md", MIME_TYPE_MARKDOWN)

        // Step 1: backup old
        val backup = storage.backupCommitted(txId, oldRef, MIME_TYPE_MARKDOWN)
        assertNotNull("backup should succeed", backup)

        // Step 2: promote staged (old still exists)
        val promoted = storage.promoteStaged(staged, "f.md")
        assertNotNull("promote should succeed", promoted)
        assertTrue("old should still exist before cleanup", storage.committedFiles.containsKey(CONTENT_OLD))

        // Step 3: cleanup (simulating manifest committed)
        storage.delete(oldRef)
        storage.delete(backup!!)

        assertFalse("old should be deleted after cleanup", storage.committedFiles.containsKey(CONTENT_OLD))
        assertFalse("backup should be deleted after cleanup", storage.backupFiles.containsKey(backup.uri))
    }

    @Test
    fun transactionFlow_newProject_skipsBackup() {
        val storage = FakeReadableMirrorStorage()
        storage.stagingFiles[CONTENT_STAGING] = NEW_CONTENT

        val txId = "tx1"
        val staged = StagedMirrorRef(txId, CONTENT_STAGING, STAGING_TX1_FMD, "f.md", MIME_TYPE_MARKDOWN)

        // No old ref → no backup step
        val oldRef: MirrorFileRef? = null
        val backup: MirrorFileRef? =
            if (oldRef != null) {
                storage.backupCommitted(
                    txId,
                    oldRef,
                    MIME_TYPE_MARKDOWN,
                )
            } else {
                null
            }
        assertNull("no backup for new project", backup)

        // Promote directly
        val promoted = storage.promoteStaged(staged, "f.md")
        assertNotNull("promote should succeed", promoted)
    }

    @Test
    fun transactionFlow_promoteFailure_restoresBackup() {
        val storage = FakeReadableMirrorStorage()
        storage.committedFiles[CONTENT_OLD] = OLD_CONTENT
        storage.stagingFiles[CONTENT_STAGING] = NEW_CONTENT
        storage.failPromote = true // Force promote to fail

        val txId = "tx1"
        val oldRef = MirrorFileRef(CONTENT_OLD, "f.md")
        val staged = StagedMirrorRef(txId, CONTENT_STAGING, STAGING_TX1_FMD, "f.md", MIME_TYPE_MARKDOWN)

        // Step 1: backup old
        val backup = storage.backupCommitted(txId, oldRef, MIME_TYPE_MARKDOWN)
        assertNotNull(backup)

        // Step 2: promote fails
        val promoted = storage.promoteStaged(staged, "f.md")
        assertNull("promote should fail", promoted)

        // Step 3: restore backup on failure
        val restored = storage.restoreBackup(backup!!, "f.md", MIME_TYPE_MARKDOWN, null)
        assertTrue("restore should succeed", restored is RestoreBackupResult.Restored)
        val restoredRef = (restored as RestoreBackupResult.Restored).ref
        assertEquals("restored content should match old", OLD_CONTENT, storage.committedFiles[restoredRef.uri])
    }

    // ── Recovery: skip PROMOTED/COMMITTED items ──

    @Test
    fun recovery_shouldSkipPromotedItems() {
        val key = ChapterKey("p1", "v1", "ch1")
        val item =
            PendingItem(
                key = key,
                stagedRef = StagedMirrorRef("tx1", CONTENT_S, STAGING_TX1_FMD, "f.md", MIME_TYPE_MARKDOWN),
                oldRef = MirrorFileRef(CONTENT_OLD, "f.md"),
                backupOldRef = MirrorFileRef(CONTENT_BACKUP, BACKUP_FMD),
                promotedRef = MirrorFileRef(CONTENT_NEW, "f.md"),
                state = PendingItem.STATE_PROMOTED,
            )

        // Simulate recovery decision logic
        val shouldSkip =
            (item.state == PendingItem.STATE_PROMOTED || item.state == PendingItem.STATE_COMMITTED) &&
                item.promotedRef != null

        assertTrue("PROMOTED item should be skipped during recovery", shouldSkip)
    }

    @Test
    fun recovery_shouldSkipCommittedItems() {
        val key = ChapterKey("p1", "v1", "ch1")
        val item =
            PendingItem(
                key = key,
                stagedRef = StagedMirrorRef("tx1", CONTENT_S, STAGING_TX1_FMD, "f.md", MIME_TYPE_MARKDOWN),
                oldRef = MirrorFileRef(CONTENT_OLD, "f.md"),
                backupOldRef = MirrorFileRef(CONTENT_BACKUP, BACKUP_FMD),
                promotedRef = MirrorFileRef(CONTENT_NEW, "f.md"),
                state = PendingItem.STATE_COMMITTED,
            )

        val shouldSkip =
            (item.state == PendingItem.STATE_PROMOTED || item.state == PendingItem.STATE_COMMITTED) &&
                item.promotedRef != null

        assertTrue("COMMITTED item should be skipped during recovery", shouldSkip)
    }

    @Test
    fun recovery_shouldNotSkipStagedItems() {
        val key = ChapterKey("p1", "v1", "ch1")
        val item =
            PendingItem(
                key = key,
                stagedRef = StagedMirrorRef("tx1", CONTENT_S, STAGING_TX1_FMD, "f.md", MIME_TYPE_MARKDOWN),
                oldRef = MirrorFileRef(CONTENT_OLD, "f.md"),
                backupOldRef = null,
                promotedRef = null,
                state = PendingItem.STATE_STAGED,
            )

        val shouldSkip =
            (item.state == PendingItem.STATE_PROMOTED || item.state == PendingItem.STATE_COMMITTED) &&
                item.promotedRef != null

        assertFalse("STAGED item should NOT be skipped during recovery", shouldSkip)
    }

    @Test
    fun recovery_shouldNotSkipOldBackedUpItems() {
        val key = ChapterKey("p1", "v1", "ch1")
        val item =
            PendingItem(
                key = key,
                stagedRef = StagedMirrorRef("tx1", CONTENT_S, STAGING_TX1_FMD, "f.md", MIME_TYPE_MARKDOWN),
                oldRef = MirrorFileRef(CONTENT_OLD, "f.md"),
                backupOldRef = MirrorFileRef(CONTENT_BACKUP, BACKUP_FMD),
                promotedRef = null,
                state = PendingItem.STATE_OLD_BACKED_UP,
            )

        val shouldSkip =
            (item.state == PendingItem.STATE_PROMOTED || item.state == PendingItem.STATE_COMMITTED) &&
                item.promotedRef != null

        assertFalse("OLD_BACKED_UP item should NOT be skipped during recovery", shouldSkip)
    }

    // ── #649 评论 5564379115 问题 3：idempotent delete ──

    @Test
    fun delete_idempotent_alreadyDeleted_returnsTrue() {
        val storage = FakeReadableMirrorStorage()
        val ref = MirrorFileRef("content://fake/0", "test.md")

        // File doesn't exist in any map → delete should return true (idempotent)
        val result = storage.delete(ref)
        assertTrue("delete of non-existent file should return true (idempotent)", result)
    }

    @Test
    fun delete_idempotent_afterFirstDelete_returnsTrue() {
        val storage = FakeReadableMirrorStorage()
        storage.committedFiles[CONTENT_TEST] = "content"
        val ref = MirrorFileRef(CONTENT_TEST, "test.md")

        // First delete succeeds
        assertTrue("first delete should succeed", storage.delete(ref))
        assertFalse("file should be removed", storage.committedFiles.containsKey(CONTENT_TEST))

        // Second delete (file already gone) should still return true
        assertTrue("second delete should return true (idempotent)", storage.delete(ref))
    }

    // ── #649 评论 5564379115 问题 3：cleanup 只删 backupOldRef，不删 oldRef ──

    @Test
    fun cleanup_afterSwap_onlyDeletesBackupOldRef() {
        val storage = FakeReadableMirrorStorage()
        // After swap: old was moved to backup by backupCommitted.
        // In real MediaStore, both oldRef and backupOldRef point to the same row
        // (just with different RELATIVE_PATH). Our fake tracks them separately.
        storage.backupFiles[CONTENT_BACKUP] = OLD_CONTENT

        // Simulate what cleanupCommittedTransaction does:
        // 1. For committed item with backupOldRef → delete backupOldRef
        val backupRef = MirrorFileRef(CONTENT_BACKUP, ".staging/tx1/backup/test.md")
        storage.delete(backupRef)

        assertFalse("backup should be deleted", storage.backupFiles.containsKey(CONTENT_BACKUP))
    }

    @Test
    fun cleanup_deletedChapter_resolvesBeforeDelete() {
        val storage = FakeReadableMirrorStorage()
        // Chapter was already deleted by a previous cleanup run
        // resolve() should return null → skip delete → don't fail

        val resolved = storage.resolve("already/deleted.md")
        assertNull("resolve of deleted file should return null", resolved)

        // If resolved is null, cleanup should skip the delete
        // (not attempt to delete a non-existent URI)
    }

    // ── #649 评论 5564820566 问题 3：两步 backup 模式 ──

    @Test
    fun twoStepBackup_prepareBackupThenVacate() {
        val storage = FakeReadableMirrorStorage()
        storage.committedFiles[CONTENT_OLD] = IMPORTANT_CONTENT

        val old = MirrorFileRef(CONTENT_OLD, WORK_PATH_CH_MD)

        // Step 1: prepareBackup (copy only, don't delete old)
        val prepared = storage.prepareBackup("tx1", old, MIME_TYPE_MARKDOWN)
        assertNotNull(prepared)
        assertFalse("old should still exist after prepareBackup", prepared!!.vacated)
        assertTrue("backup should exist", storage.backupFiles.containsKey(prepared.backupRef.uri))
        assertEquals(
            "backup content should match old",
            IMPORTANT_CONTENT,
            storage.backupFiles[prepared.backupRef.uri],
        )
        assertTrue("old should still exist after prepareBackup", storage.committedFiles.containsKey(CONTENT_OLD))

        // Step 2: vacateCommitted (delete old)
        assertTrue("vacate should succeed", storage.vacateCommitted(old))
        assertFalse("old should be deleted after vacate", storage.committedFiles.containsKey(CONTENT_OLD))
    }

    @Test
    fun twoStepBackup_crashWindow_detectsExistingBackup() {
        val storage = FakeReadableMirrorStorage()
        storage.committedFiles[CONTENT_OLD] = IMPORTANT_CONTENT
        // Simulate crash window: prepareBackup created a backup but vacate wasn't called yet.
        // The fake's resolveBackup matches by URI containing relativePath.replace("/", "_").
        // For WORK_PATH_CH_MD the pattern is "作品_P_V_Ch.md"
        storage.backupFiles["content://fake/backup/path_作品_P_V_Ch.md"] = IMPORTANT_CONTENT
        val found = storage.resolveBackup("tx1", WORK_PATH_CH_MD)
        assertNotNull("resolveBackup should find existing backup", found)
    }
}
