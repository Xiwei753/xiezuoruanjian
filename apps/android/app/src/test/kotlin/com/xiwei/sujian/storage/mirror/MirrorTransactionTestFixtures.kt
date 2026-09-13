package com.xiwei.sujian.storage.mirror

// ── 共享测试常量（大写开头，满足 TopLevelPropertyNaming: [A-Z][_A-Z0-9]*）──
internal const val MIME_TYPE_MARKDOWN = "text/markdown"
internal const val STAGING_TX1_FMD = ".staging/tx1/f.md"
internal const val CONTENT_OLD = "content://old"
internal const val CONTENT_BACKUP = "content://backup"
internal const val CONTENT_S = "content://s"
internal const val MANIFEST_JSON_PATH = "_meta/manifest.json"
internal const val CONTENT_NEW = "content://new"
internal const val CONTENT_STAGING = "content://staging"
internal const val WORK_PATH_CH_MD = "作品/P/V/Ch.md"
internal const val WORK_PATH_CH1_MD = "作品/P/V/Ch1.md"
internal const val WORK_PATH_CH2_MD = "作品/P/V/Ch2.md"
internal const val BACKUP_FMD = "backup/f.md"
internal const val STAGING_TX1_BACKUP_FMD = ".staging/tx1/backup/f.md"
internal const val CONTENT_MANIFEST_NEW = "content://manifest/new"
internal const val CONTENT_MANIFEST_BACKUP = "content://manifest/backup"
internal const val CONTENT_TEST = "content://test"
internal const val PROJECT_DEL = "p-del"
internal const val PROJECT_EMPTY = "p-empty"
internal const val OLD_CONTENT = "old content"
internal const val NEW_CONTENT = "new content"
internal const val IMPORTANT_CONTENT = "important content"
internal const val CONTENT_FAKE_WORK_CH_MD = "content://fake/作品_P_V_Ch.md"

/** 构造 backup 路径的 MirrorFileRef，避免 "backup/$path" 字面量重复。 */
internal fun backupMirrorRef(path: String) = MirrorFileRef(CONTENT_BACKUP, "backup/$path")

/** Fake project snapshot for testing without Core dependency. */
internal data class FakeProjectSnapshot(val id: String, val title: String)

/**
 * Fake [ReadableMirrorStorage] implementation for unit testing.
 *
 * Tracks all file operations to verify correct flow.
 */
internal class FakeReadableMirrorStorage : ReadableMirrorStorage {
    val committedFiles = mutableMapOf<String, String>() // uri → content
    val stagingFiles = mutableMapOf<String, String>() // uri → content
    val backupFiles = mutableMapOf<String, String>() // uri → content (backup area)
    val deletedFiles = mutableListOf<String>() // uri
    val journalSteps = mutableListOf<String>() // operation log

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

    override fun readTextAndHash(ref: MirrorFileRef): Pair<String, String>? {
        val content = committedFiles[ref.uri] ?: stagingFiles[ref.uri] ?: backupFiles[ref.uri] ?: return null
        return Pair(content, computeContentHash(content))
    }
}

/**
 * Issue #667 改写用 FakeStorage：用 committedPathToUri 精确匹配路径，
 * 支持 lookup 三态查询和失败注入，用于 MirrorTransactionWorkspace 事务层测试。
 */
internal class WorkspaceTestFakeStorage : ReadableMirrorStorage {
    val committedFiles = mutableMapOf<String, String>()
    val committedPathToUri = mutableMapOf<String, String>()
    val operationLog = mutableListOf<String>()
    var failLookup = false
    var failDelete = false

    override fun createText(
        relativeDir: String,
        displayName: String,
        mimeType: String,
        text: String,
    ): MirrorFileRef? {
        val path = if (relativeDir.isBlank()) displayName else "$relativeDir/$displayName"
        val uri = "content://fake/${committedFiles.size}"
        committedFiles[uri] = text
        committedPathToUri[path] = uri
        operationLog.add("createText:$path")
        return MirrorFileRef(uri, path)
    }

    override fun replaceText(
        ref: MirrorFileRef,
        text: String,
    ): Boolean {
        committedFiles[ref.uri] = text
        operationLog.add("replaceText:${ref.relativePath}")
        return true
    }

    override fun delete(ref: MirrorFileRef): Boolean {
        operationLog.add("delete:${ref.relativePath}")
        if (failDelete) return false
        committedFiles.remove(ref.uri)
        committedPathToUri.entries.removeIf { it.value == ref.uri }
        return true
    }

    override fun isSupported(): Boolean = true

    override fun resolve(relativePath: String): MirrorFileRef? {
        operationLog.add("resolve:$relativePath")
        val uri = committedPathToUri[relativePath] ?: return null
        return MirrorFileRef(uri, relativePath)
    }

    override fun lookup(relativePath: String): MirrorLookupResult {
        operationLog.add("lookup:$relativePath")
        if (failLookup) return MirrorLookupResult.Failed(SecurityException("simulated lookup failure"))
        val uri = committedPathToUri[relativePath] ?: return MirrorLookupResult.Missing
        return MirrorLookupResult.Found(MirrorFileRef(uri, relativePath))
    }

    override fun readTextAndHash(ref: MirrorFileRef): Pair<String, String>? {
        val content = committedFiles[ref.uri] ?: return null
        return Pair(content, computeContentHash(content))
    }
}

/** upsertProject round-trip 的 setup fixture。 */
internal data class UpsertProjectFixture(
    val key1: ChapterKey,
    val key2: ChapterKey,
    val journal: PendingMirrorPublish,
)

/** 构造 upsertProject 测试所需的 journal 和 keys，提取 setup 以满足 LongMethod 阈值。 */
internal fun buildUpsertProjectFixture(): UpsertProjectFixture {
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
