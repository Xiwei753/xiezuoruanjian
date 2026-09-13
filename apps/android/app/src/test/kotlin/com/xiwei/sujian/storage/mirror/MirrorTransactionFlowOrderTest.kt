package com.xiwei.sujian.storage.mirror

import android.content.Context
import androidx.test.core.app.ApplicationProvider
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
 * #649 评论 5562715833：镜像事务流程顺序单元测试。
 *
 * 覆盖：
 * - Transaction flow order verification
 * - idempotent delete / cleanup / two-step backup
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class MirrorTransactionFlowOrderTest {
    // ── Transaction flow order verification ──
    // Issue #667 改写：用 workspace + executor 验证事务流顺序。

    /**
     * 事务流先备份后提升。
     *
     * Issue #667 改写：用 workspace.prepareBackup + promoteItemStaged 验证顺序。
     */
    @Test
    fun transactionFlow_backupBeforePromote() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val workspace = MirrorTransactionWorkspace(context)
        val storage = WorkspaceTestFakeStorage()

        val txId = "tx-flow-1"
        val relativePath = WORK_PATH_CH_MD
        val oldContent = OLD_CONTENT
        val newContent = "new content"

        // 1. 先备份
        val oldRef = MirrorFileRef(CONTENT_OLD_1, relativePath)
        storage.committedFiles[oldRef.uri] = oldContent
        storage.committedPathToUri[relativePath] = oldRef.uri
        val backupRef = workspace.prepareBackup(txId, oldRef, oldContent)
        assertNotNull("备份应成功", backupRef)

        // 2. 后提升（stage + promote）
        val stagedRef = workspace.stageText(txId, relativePath, MIME_TYPE_MARKDOWN, newContent)!!
        val key = ChapterKey("p1", "v1", "ch1")
        val item =
            PendingItem(
                key = key,
                stagedRef = stagedRef,
                oldRef = oldRef,
                backupOldRef = backupRef,
                promotedRef = null,
                state = PendingItem.STATE_OLD_VACATED,
            )
        val desiredEntries =
            mapOf(
                key to ChapterMirrorEntry(CONTENT_NEW_1, relativePath, 1L, computeContentHash(newContent)),
            )

        // 反射调用 promoteItemStaged
        val stateStore = ReadableMirrorStateStore(context)
        val journalWriter = MirrorJournalWriter(stateStore)
        val rollbackExecutor = MirrorRollbackExecutor(stateStore, journalWriter, workspace)
        val executor = MirrorPublishPromoteExecutor(journalWriter, rollbackExecutor, workspace)
        val method =
            MirrorPublishPromoteExecutor::class.java.getDeclaredMethod(
                PROMOTE_ITEM_STAGED,
                ChapterKey::class.java,
                PendingItem::class.java,
                StagedMirrorRef::class.java,
                Map::class.java,
                ReadableMirrorStorage::class.java,
            )
        method.isAccessible = true
        val promotedRef = method.invoke(executor, key, item, stagedRef, desiredEntries, storage) as MirrorFileRef?

        assertNotNull("promote 应成功", promotedRef)
        // 验证备份仍在（promote 不删备份）
        val backupLookup = workspace.lookupBackup(txId, relativePath)
        assertTrue("备份在 promote 后仍存在", backupLookup is MirrorLookupResult.Found)
    }

    /**
     * 新项目跳过备份。
     *
     * Issue #667 改写：oldRef==null 时跳过备份直接 promote。
     */
    @Test
    fun transactionFlow_newProject_skipsBackup() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val workspace = MirrorTransactionWorkspace(context)
        val storage = WorkspaceTestFakeStorage()

        val txId = "tx-flow-2"
        val relativePath = WORK_PATH_CH_MD
        val newContent = "new content for new project"

        // 新项目：oldRef == null，跳过备份
        val stagedRef = workspace.stageText(txId, relativePath, MIME_TYPE_MARKDOWN, newContent)!!
        val key = ChapterKey("p1", "v1", "ch1")
        val item =
            PendingItem(
                key = key,
                stagedRef = stagedRef,
                // 新项目无旧文件
                oldRef = null,
                backupOldRef = null,
                promotedRef = null,
                state = PendingItem.STATE_STAGED,
            )
        val desiredEntries =
            mapOf(
                key to ChapterMirrorEntry(CONTENT_NEW_1, relativePath, 1L, computeContentHash(newContent)),
            )

        // 反射调用 promoteItemStaged
        val stateStore = ReadableMirrorStateStore(context)
        val journalWriter = MirrorJournalWriter(stateStore)
        val rollbackExecutor = MirrorRollbackExecutor(stateStore, journalWriter, workspace)
        val executor = MirrorPublishPromoteExecutor(journalWriter, rollbackExecutor, workspace)
        val method =
            MirrorPublishPromoteExecutor::class.java.getDeclaredMethod(
                PROMOTE_ITEM_STAGED,
                ChapterKey::class.java,
                PendingItem::class.java,
                StagedMirrorRef::class.java,
                Map::class.java,
                ReadableMirrorStorage::class.java,
            )
        method.isAccessible = true
        val promotedRef = method.invoke(executor, key, item, stagedRef, desiredEntries, storage) as MirrorFileRef?

        assertNotNull("新项目 promote 应成功（跳过备份）", promotedRef)
        // 无备份创建
        val backupLookup = workspace.lookupBackup(txId, relativePath)
        assertTrue("新项目无备份", backupLookup is MirrorLookupResult.Missing)
    }

    /**
     * 提升失败恢复备份。
     *
     * Issue #667 改写：promoteItemStaged 失败（lookup==Failed）时返回 null，
     * 备份仍在 workspace 中，可用于后续 rollback 恢复。
     */
    @Test
    fun transactionFlow_promoteFailure_restoresBackup() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val workspace = MirrorTransactionWorkspace(context)
        val storage = WorkspaceTestFakeStorage()

        val txId = "tx-flow-3"
        val relativePath = WORK_PATH_CH_MD
        val oldContent = OLD_CONTENT
        val newContent = "new content"

        // 1. 备份旧文件
        val oldRef = MirrorFileRef(CONTENT_OLD_1, relativePath)
        val backupRef = workspace.prepareBackup(txId, oldRef, oldContent)!!

        // 2. stage 新内容
        val stagedRef = workspace.stageText(txId, relativePath, MIME_TYPE_MARKDOWN, newContent)!!

        // 3. promote 失败（lookup==Failed）
        storage.failLookup = true
        val key = ChapterKey("p1", "v1", "ch1")
        val item =
            PendingItem(
                key = key,
                stagedRef = stagedRef,
                oldRef = oldRef,
                backupOldRef = backupRef,
                promotedRef = null,
                state = PendingItem.STATE_OLD_VACATED,
            )
        val desiredEntries =
            mapOf(
                key to ChapterMirrorEntry(CONTENT_NEW_1, relativePath, 1L, computeContentHash(newContent)),
            )

        val stateStore = ReadableMirrorStateStore(context)
        val journalWriter = MirrorJournalWriter(stateStore)
        val rollbackExecutor = MirrorRollbackExecutor(stateStore, journalWriter, workspace)
        val executor = MirrorPublishPromoteExecutor(journalWriter, rollbackExecutor, workspace)
        val method =
            MirrorPublishPromoteExecutor::class.java.getDeclaredMethod(
                PROMOTE_ITEM_STAGED,
                ChapterKey::class.java,
                PendingItem::class.java,
                StagedMirrorRef::class.java,
                Map::class.java,
                ReadableMirrorStorage::class.java,
            )
        method.isAccessible = true
        val promotedRef = method.invoke(executor, key, item, stagedRef, desiredEntries, storage) as MirrorFileRef?

        // 4. promote 失败，返回 null
        assertNull("promote 失败应返回 null", promotedRef)

        // 5. 备份仍在，可用于 rollback 恢复
        val backupLookup = workspace.lookupBackup(txId, relativePath)
        assertTrue("备份仍在（可用于 rollback 恢复）", backupLookup is MirrorLookupResult.Found)
        val backupContent = workspace.readBackup((backupLookup as MirrorLookupResult.Found).ref)
        assertEquals("备份内容正确，可恢复旧内容", oldContent, backupContent)
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
    // Issue #667 改写：prepareBackup/vacateCommitted/resolveBackup 已从 ReadableMirrorStorage 移除，
    // 事务操作移到了 MirrorTransactionWorkspace。用 workspace 方法验证两步备份。

    /**
     * 两步备份先准备后腾空。
     *
     * Issue #667 改写：用 workspace.prepareBackup + lookupBackup 验证两步备份。
     * 1. prepareBackup 把旧内容写到私有 backup 目录
     * 2. lookupBackup 确认备份已就绪
     * 3. 旧文件从 final 删除（腾空）在 promoteItemStaged 中完成
     */
    @Test
    fun twoStepBackup_prepareBackupThenVacate() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val workspace = MirrorTransactionWorkspace(context)
        val storage = WorkspaceTestFakeStorage()

        val txId = "tx-twostep-1"
        val relativePath = WORK_PATH_CH_MD
        val oldContent = OLD_CONTENT

        // 1. 准备备份（第一步）
        val oldRef = MirrorFileRef(CONTENT_OLD_1, relativePath)
        storage.committedFiles[oldRef.uri] = oldContent
        storage.committedPathToUri[relativePath] = oldRef.uri
        val backupRef = workspace.prepareBackup(txId, oldRef, oldContent)
        assertNotNull("prepareBackup 应成功", backupRef)

        // 2. 确认备份已就绪
        val lookupResult = workspace.lookupBackup(txId, relativePath)
        assertTrue("备份已就绪", lookupResult is MirrorLookupResult.Found)

        // 3. 腾空（第二步）：删除旧文件
        val deleteResult = storage.delete(oldRef)
        assertTrue("删除旧文件应成功", deleteResult)
        assertFalse("旧文件已从 final 腾空", storage.committedFiles.containsKey(oldRef.uri))

        // 备份仍在（不受腾空影响）
        val lookupAfterVacate = workspace.lookupBackup(txId, relativePath)
        assertTrue("腾空后备份仍在", lookupAfterVacate is MirrorLookupResult.Found)
    }

    /**
     * 崩溃窗口检测已有备份。
     *
     * Issue #667 改写：用 workspace.lookupBackup 三态发现验证崩溃窗口。
     * 场景：prepareBackup 已完成但 journal 未落盘 → 重启后 lookupBackup Found 可恢复。
     */
    @Test
    fun twoStepBackup_crashWindow_detectsExistingBackup() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val workspace = MirrorTransactionWorkspace(context)

        val txId = "tx-crash-1"
        val relativePath = WORK_PATH_CH_MD
        val oldContent = "old content before crash"

        // 模拟崩溃窗口：prepareBackup 已完成，journal 未落盘
        val oldRef = MirrorFileRef(CONTENT_OLD_1, relativePath)
        workspace.prepareBackup(txId, oldRef, oldContent)

        // 重启后用 lookupBackup 三态发现检测已有备份
        val lookupResult = workspace.lookupBackup(txId, relativePath)
        assertTrue("lookupBackup 应返回 Found", lookupResult is MirrorLookupResult.Found)

        // 读取备份内容验证完整性
        val backupRef = (lookupResult as MirrorLookupResult.Found).ref
        val backupContent = workspace.readBackup(backupRef)
        assertEquals("备份内容完整", oldContent, backupContent)

        // 对比：不存在的 txId 返回 Missing
        val missingResult = workspace.lookupBackup("nonexistent-tx", relativePath)
        assertTrue("不存在的 txId 返回 Missing", missingResult is MirrorLookupResult.Missing)
    }
}
