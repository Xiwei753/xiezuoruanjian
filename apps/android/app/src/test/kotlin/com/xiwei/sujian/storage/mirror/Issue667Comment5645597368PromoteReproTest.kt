package com.xiwei.sujian.storage.mirror

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * Issue #667 评论 5645597368 问题 1 回归测试。
 *
 * 目标：验证 [MirrorPublishPromoteExecutor.promoteItemStaged] 在查旧文件失败时停止 promote 并返回 null。
 *
 * ## 回归策略
 * 用反射调用 private `promoteItemStaged` 方法，构造自定义 [ReadableMirrorStorage]：
 * - 问题 1.A：`storage.lookup` 返回 [MirrorLookupResult.Failed]，断言 `promoteItemStaged` 返回 null（停止 promote）。
 * - 问题 1.B：`storage.lookup` 返回 [MirrorLookupResult.Found] 但 `storage.delete` 返回 false，
 *   断言 `promoteItemStaged` 返回 null（停止 promote）。
 *
 * 修复后这些测试断言期望行为（返回 null），证明 bug 已修复。
 *
 * 相关源文件：MirrorPublishPromoteExecutor.kt 第 225-263 行。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class Issue667Comment5645597368PromoteReproTest {

    private lateinit var context: Context
    private lateinit var workspace: MirrorTransactionWorkspace
    private lateinit var executor: MirrorPublishPromoteExecutor

    @Before
    fun setUp() {
        context = ApplicationProvider.getApplicationContext()
        workspace = MirrorTransactionWorkspace(context)
        val stateStore = ReadableMirrorStateStore(context)
        val journalWriter = MirrorJournalWriter(stateStore)
        val rollbackExecutor = MirrorRollbackExecutor(stateStore, journalWriter, workspace)
        executor = MirrorPublishPromoteExecutor(journalWriter, rollbackExecutor, workspace)
    }

    /**
     * 问题 1.A 回归：promoteItemStaged 在 storage.lookup 返回 Failed 时停止 promote 返回 null。
     *
     * 修复后行为：lookup == Failed 时记日志后直接 return null，停止 promote，保留 journal。
     *
     * 此测试断言期望行为（promoteItemStaged 返回 null），证明 bug 已修复。
     */
    @Test
    fun problem1A_promoteItemStaged_stopsWhenLookupFailed() {
        // 1. 在 workspace 中 stage 内容，让 readStaged 能返回非 null
        val txId = "tx-test-1a"
        val relativePath = "作品/P/V/Ch.md"
        val stagedRef = workspace.stageText(txId, relativePath, "text/markdown", "new content")
        assertNotNull("stageText 应成功", stagedRef)

        // 2. 构造自定义 storage，lookup 返回 Failed
        val storage = LookupFailedStorage()

        // 3. 构造 PendingItem，设置 oldRef（非 null），state = STATE_BACKUP_READY
        //    （非 STATE_OLD_VACATED，让 findExistingPromotedRef 返回 null）
        val key = ChapterKey("p1", "v1", "ch1")
        val oldRef = MirrorFileRef("content://old/1", relativePath)
        val item = PendingItem(
            key = key,
            stagedRef = stagedRef,
            oldRef = oldRef,
            backupOldRef = null,
            promotedRef = null,
            state = PendingItem.STATE_BACKUP_READY,
        )

        // 4. 构造 desiredEntries
        val desiredEntries = mapOf(
            key to ChapterMirrorEntry(
                uri = "content://new/1",
                relativePath = relativePath,
                revision = 1L,
                contentHash = "sha256:abc",
            ),
        )

        // 5. 用反射调用 private promoteItemStaged
        val result = invokePromoteItemStaged(key, item, stagedRef!!, desiredEntries, storage)

        // 6. 断言期望行为：lookup == Failed 时 promoteItemStaged 返回 null（停止 promote，保留 journal）
        assertNull(
            "问题1.A 回归：lookup 返回 Failed 时 promoteItemStaged 返回 null（停止 promote，保留 journal）。",
            result,
        )
    }

    /**
     * 问题 1.B 回归：promoteItemStaged 在 storage.delete 返回 false 时停止 promote 返回 null。
     *
     * 修复后行为：lookup 返回 Found 但 delete 返回 false 时记日志后直接 return null，停止 promote。
     *
     * 此测试断言期望行为（promoteItemStaged 返回 null），证明 bug 已修复。
     */
    @Test
    fun problem1B_promoteItemStaged_stopsWhenDeleteOldFailed() {
        // 1. 在 workspace 中 stage 内容
        val txId = "tx-test-1b"
        val relativePath = "作品/P/V/Ch.md"
        val stagedRef = workspace.stageText(txId, relativePath, "text/markdown", "new content")
        assertNotNull("stageText 应成功", stagedRef)

        // 2. 构造自定义 storage，lookup 返回 Found 但 delete 返回 false
        val storage = DeleteFailedStorage()

        // 3. 构造 PendingItem，设置 oldRef（非 null），state = STATE_BACKUP_READY
        val key = ChapterKey("p1", "v1", "ch1")
        val oldRef = MirrorFileRef("content://old/1", relativePath)
        val item = PendingItem(
            key = key,
            stagedRef = stagedRef,
            oldRef = oldRef,
            backupOldRef = null,
            promotedRef = null,
            state = PendingItem.STATE_BACKUP_READY,
        )

        // 4. 构造 desiredEntries
        val desiredEntries = mapOf(
            key to ChapterMirrorEntry(
                uri = "content://new/1",
                relativePath = relativePath,
                revision = 1L,
                contentHash = "sha256:abc",
            ),
        )

        // 5. 用反射调用 private promoteItemStaged
        val result = invokePromoteItemStaged(key, item, stagedRef!!, desiredEntries, storage)

        // 6. 断言期望行为：delete 失败时 promoteItemStaged 返回 null（停止 promote，保留 journal）
        assertNull(
            "问题1.B 回归：delete 返回 false 时 promoteItemStaged 返回 null（停止 promote，保留 journal）。",
            result,
        )
    }

    /**
     * 问题 1.C 佐证：promoteItemStaged 在 lookup 返回 Missing 时继续创建新文件（正确行为）。
     *
     * 此测试验证 Missing 分支是正确的，作为对照。修复前后行为一致。
     */
    @Test
    fun problem1C_promoteItemStaged_continuesWhenLookupMissing_correctBehavior() {
        val txId = "tx-test-1c"
        val relativePath = "作品/P/V/Ch.md"
        val stagedRef = workspace.stageText(txId, relativePath, "text/markdown", "new content")!!

        val storage = LookupMissingStorage()

        val key = ChapterKey("p1", "v1", "ch1")
        val oldRef = MirrorFileRef("content://old/1", relativePath)
        val item = PendingItem(
            key = key,
            stagedRef = stagedRef,
            oldRef = oldRef,
            backupOldRef = null,
            promotedRef = null,
            state = PendingItem.STATE_BACKUP_READY,
        )

        val desiredEntries = mapOf(
            key to ChapterMirrorEntry("content://new/1", relativePath, 1L, "sha256:abc"),
        )

        val result = invokePromoteItemStaged(key, item, stagedRef, desiredEntries, storage)

        // Missing 时继续创建新文件是正确行为
        assertNotNull(
            "对照：lookup 返回 Missing 时 promoteItemStaged 返回非 null（正确行为）",
            result,
        )
    }

    // ── 反射工具 ──

    private fun invokePromoteItemStaged(
        key: ChapterKey,
        item: PendingItem,
        staged: StagedMirrorRef,
        desiredEntries: Map<ChapterKey, ChapterMirrorEntry>,
        storage: ReadableMirrorStorage,
    ): MirrorFileRef? {
        val method = MirrorPublishPromoteExecutor::class.java.getDeclaredMethod(
            "promoteItemStaged",
            ChapterKey::class.java,
            PendingItem::class.java,
            StagedMirrorRef::class.java,
            Map::class.java,
            ReadableMirrorStorage::class.java,
        )
        method.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        return method.invoke(executor, key, item, staged, desiredEntries, storage) as MirrorFileRef?
    }

    // ── 自定义 ReadableMirrorStorage 实现 ──

    /** lookup 始终返回 Failed（模拟查询失败）。 */
    private class LookupFailedStorage : ReadableMirrorStorage {
        override fun createText(
            relativeDir: String,
            displayName: String,
            mimeType: String,
            text: String,
        ): MirrorFileRef {
            val path = if (relativeDir.isBlank()) displayName else "$relativeDir/$displayName"
            return MirrorFileRef("content://fake/new/${path.hashCode()}", path)
        }

        override fun replaceText(ref: MirrorFileRef, text: String): Boolean = true
        override fun delete(ref: MirrorFileRef): Boolean = true
        override fun isSupported(): Boolean = true
        override fun resolve(relativePath: String): MirrorFileRef? = null
        override fun lookup(relativePath: String): MirrorLookupResult =
            MirrorLookupResult.Failed(SecurityException("simulated lookup failure"))
        override fun readTextAndHash(ref: MirrorFileRef): Pair<String, String>? = null
    }

    /** lookup 返回 Found 但 delete 返回 false（模拟删除失败）。 */
    private class DeleteFailedStorage : ReadableMirrorStorage {
        override fun createText(
            relativeDir: String,
            displayName: String,
            mimeType: String,
            text: String,
        ): MirrorFileRef {
            val path = if (relativeDir.isBlank()) displayName else "$relativeDir/$displayName"
            return MirrorFileRef("content://fake/new/${path.hashCode()}", path)
        }

        override fun replaceText(ref: MirrorFileRef, text: String): Boolean = true
        override fun delete(ref: MirrorFileRef): Boolean = false // 删除失败
        override fun isSupported(): Boolean = true
        override fun resolve(relativePath: String): MirrorFileRef? =
            MirrorFileRef("content://old/1", relativePath)
        override fun lookup(relativePath: String): MirrorLookupResult =
            MirrorLookupResult.Found(MirrorFileRef("content://old/1", relativePath))
        override fun readTextAndHash(ref: MirrorFileRef): Pair<String, String>? = null
    }

    /** lookup 始终返回 Missing（对照用）。 */
    private class LookupMissingStorage : ReadableMirrorStorage {
        override fun createText(
            relativeDir: String,
            displayName: String,
            mimeType: String,
            text: String,
        ): MirrorFileRef {
            val path = if (relativeDir.isBlank()) displayName else "$relativeDir/$displayName"
            return MirrorFileRef("content://fake/new/${path.hashCode()}", path)
        }

        override fun replaceText(ref: MirrorFileRef, text: String): Boolean = true
        override fun delete(ref: MirrorFileRef): Boolean = true
        override fun isSupported(): Boolean = true
        override fun resolve(relativePath: String): MirrorFileRef? = null
        override fun lookup(relativePath: String): MirrorLookupResult = MirrorLookupResult.Missing
        override fun readTextAndHash(ref: MirrorFileRef): Pair<String, String>? = null
    }
}
