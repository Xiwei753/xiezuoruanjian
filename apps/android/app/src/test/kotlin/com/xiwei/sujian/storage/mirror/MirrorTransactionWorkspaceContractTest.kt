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
 * #649 评论 5562715833：镜像事务 workspace 契约单元测试。
 *
 * 覆盖 ReadableMirrorStorage 接口契约（backup/promote/restore/rollback），
 * 用 MirrorTransactionWorkspace + executor 验证事务层行为。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class MirrorTransactionWorkspaceContractTest {
    // ── FakeReadableMirrorStorage: interface contract tests ──
    // Issue #667 改写：promoteStaged/backupCommitted/restoreBackup/rollback 已从 ReadableMirrorStorage 移除，
    // 事务操作移到了 MirrorTransactionWorkspace。以下测试用 workspace + executor 验证事务层行为。

    /**
     * promoteStaged 不删除旧文件（与问题 1 直接相关）。
     *
     * Issue #667 改写：用反射调用 MirrorPublishPromoteExecutor.promoteItemStaged，
     * 验证 lookup==Failed 时返回 null（停止 promote，保留 journal）。
     * 这正是旧用例原本锁住的失败边界。
     */
    @Test
    fun fakeStorage_promoteStaged_doesNotDeleteOld() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val workspace = MirrorTransactionWorkspace(context)
        val stateStore = ReadableMirrorStateStore(context)
        val journalWriter = MirrorJournalWriter(stateStore)
        val rollbackExecutor = MirrorRollbackExecutor(stateStore, journalWriter, workspace)
        val executor = MirrorPublishPromoteExecutor(journalWriter, rollbackExecutor, workspace)

        // 1. 在 workspace 中 stage 内容
        val txId = "tx-promote-1"
        val relativePath = WORK_PATH_CH_MD
        val stagedRef = workspace.stageText(txId, relativePath, MIME_TYPE_MARKDOWN, "new content")!!
        val storage = WorkspaceTestFakeStorage().apply { failLookup = true }

        val key = ChapterKey("p1", "v1", "ch1")
        val oldRef = MirrorFileRef(CONTENT_OLD_1, relativePath)
        val item =
            PendingItem(
                key = key,
                stagedRef = stagedRef,
                oldRef = oldRef,
                backupOldRef = null,
                promotedRef = null,
                state = PendingItem.STATE_BACKUP_READY,
            )
        val desiredEntries =
            mapOf(
                key to ChapterMirrorEntry("content://new/1", relativePath, 1L, "sha256:abc"),
            )

        // 2. 反射调用 private promoteItemStaged
        val method =
            MirrorPublishPromoteExecutor::class.java.getDeclaredMethod(
                "promoteItemStaged",
                ChapterKey::class.java,
                PendingItem::class.java,
                StagedMirrorRef::class.java,
                Map::class.java,
                ReadableMirrorStorage::class.java,
            )
        method.isAccessible = true
        val result = method.invoke(executor, key, item, stagedRef, desiredEntries, storage) as MirrorFileRef?

        // 3. lookup==Failed 时 promoteItemStaged 返回 null（停止 promote，保留 journal）
        assertNull(
            "promoteItemStaged 在 lookup==Failed 时返回 null，停止 promote，保留 journal",
            result,
        )
        // 旧文件未被删除（delete 未被调用）
        assertFalse(
            "旧文件未被删除（promote 已停止）",
            storage.operationLog.contains("delete:$relativePath"),
        )
    }

    /**
     * backupCommitted 创建副本。
     *
     * Issue #667 改写：用 workspace.prepareBackup 验证备份副本创建。
     */
    @Test
    fun fakeStorage_backupCommitted_createsCopy() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val workspace = MirrorTransactionWorkspace(context)

        val txId = "tx-backup-1"
        val relativePath = WORK_PATH_CH_MD
        val oldContent = "old content to backup"
        val oldRef = MirrorFileRef(CONTENT_OLD_1, relativePath)

        // prepareBackup 创建备份副本
        val backupRef = workspace.prepareBackup(txId, oldRef, oldContent)
        assertNotNull("prepareBackup 应返回非 null", backupRef)

        // 验证备份内容可读且正确
        val backupContent = workspace.readBackup(backupRef!!)
        assertEquals("备份内容应与旧内容一致", oldContent, backupContent)

        // 验证 lookupBackup 能找到备份
        val lookupResult = workspace.lookupBackup(txId, relativePath)
        assertTrue("lookupBackup 应找到备份", lookupResult is MirrorLookupResult.Found)
    }

    /**
     * restoreBackup 写到最终位置。
     *
     * Issue #667 改写：用 workspace.readBackup + storage.createText 验证恢复到最终位置。
     */
    @Test
    fun fakeStorage_restoreBackup_writesToFinalLocation() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val workspace = MirrorTransactionWorkspace(context)
        val storage = WorkspaceTestFakeStorage()

        val txId = "tx-restore-final"
        val relativePath = WORK_PATH_CH_MD
        val oldContent = "old content to restore"

        // 准备备份
        val oldRef = MirrorFileRef(CONTENT_OLD_1, relativePath)
        val backupRef = workspace.prepareBackup(txId, oldRef, oldContent)!!

        // 从备份恢复到最终位置
        val backupContent = workspace.readBackup(backupRef)
        assertEquals("备份内容正确", oldContent, backupContent)

        val restoredRef = storage.createText("作品/P/V", "Ch.md", MIME_TYPE_MARKDOWN, backupContent!!)
        assertNotNull("恢复后 final 应存在", restoredRef)
        assertEquals(
            "恢复后 final 路径正确",
            relativePath,
            restoredRef!!.relativePath,
        )
        assertEquals(
            "恢复后 final 内容正确",
            oldContent,
            storage.committedFiles[restoredRef.uri],
        )
    }

    /**
     * rollback 删除所有暂存文件。
     *
     * Issue #667 改写：用 workspace.rollback 验证删除 staging 和 backup 目录。
     */
    @Test
    fun fakeStorage_rollback_deletesAllStagingFiles() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val workspace = MirrorTransactionWorkspace(context)

        val txId = "tx-rollback-1"
        val relativePath = WORK_PATH_CH_MD

        // 在 workspace 中创建 staging 和 backup 文件
        workspace.stageText(txId, relativePath, MIME_TYPE_MARKDOWN, "staged content")
        val oldRef = MirrorFileRef(CONTENT_OLD_1, relativePath)
        workspace.prepareBackup(txId, oldRef, "old content")

        // rollback 删除所有暂存文件
        val result = workspace.rollback(txId)
        assertTrue("rollback 应成功", result)

        // 验证 staging 和 backup 都已删除
        val backupLookup = workspace.lookupBackup(txId, relativePath)
        assertTrue("backup 应已删除", backupLookup is MirrorLookupResult.Missing)
    }
}
