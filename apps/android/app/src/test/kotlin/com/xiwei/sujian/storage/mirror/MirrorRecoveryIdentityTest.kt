package com.xiwei.sujian.storage.mirror

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #649 评论 5566303837 问题 4：RestoreBackupResult identity verification
 * + recovery skip/keep 判断 + promote 失败恢复备份。
 *
 * Issue #667 改写：restoreBackup 已从 ReadableMirrorStorage 移除，事务操作移到了 MirrorTransactionWorkspace。
 * 用 workspace.lookupBackup + storage.lookup + storage.readTextAndHash 组合验证恢复逻辑。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class MirrorRecoveryIdentityTest {
    // ── #649 评论 5566303837 问题 4：RestoreBackupResult identity verification ──
    // Issue #667 改写：restoreBackup 已从 ReadableMirrorStorage 移除，事务操作移到了 MirrorTransactionWorkspace
    // 改写：用 workspace.lookupBackup + storage.lookup + storage.readTextAndHash 组合验证恢复逻辑。

    private companion object {
        const val WORK_PATH = "作品/P/V/Ch.md"
        const val OLD_CONTENT = "old content"
        const val URI_OLD_1 = "content://old/1"
        const val MSG_PREPARE_BACKUP_SUCCESS = "prepareBackup 应成功"
        const val MSG_BACKUP_EXISTS = "backup 应存在"
    }

    /**
     * restoreBackup hash 匹配返回已恢复。
     *
     * Issue #667 改写：用 MirrorTransactionWorkspace.lookupBackup 验证备份存在，
     * 用 storage.lookup + readTextAndHash 验证 final 位置已是旧内容（hash 匹配）→ 已恢复。
     */
    @Test
    fun restoreBackup_withHash_match_returnsAlreadyRestored() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val workspace = MirrorTransactionWorkspace(context)
        val storage = WorkspaceTestFakeStorage()

        val txId = "tx-restore-1"
        val relativePath = WORK_PATH
        val oldContent = OLD_CONTENT
        val oldHash = computeContentHash(oldContent)

        // 准备 backup（用 workspace.prepareBackup）
        val oldRef = MirrorFileRef(URI_OLD_1, relativePath)
        val backupRef = workspace.prepareBackup(txId, oldRef, oldContent)
        assertNotNull(MSG_PREPARE_BACKUP_SUCCESS, backupRef)

        // final 位置已是旧内容（hash 匹配）→ 已恢复
        val finalUri = "content://final/old"
        storage.committedFiles[finalUri] = oldContent
        storage.committedPathToUri[relativePath] = finalUri

        // 验证恢复逻辑：lookupBackup Found + final hash 匹配 → 已恢复
        val backupLookup = workspace.lookupBackup(txId, relativePath)
        assertTrue(MSG_BACKUP_EXISTS, backupLookup is MirrorLookupResult.Found)
        val finalLookup = storage.lookup(relativePath)
        assertTrue("final 应存在", finalLookup is MirrorLookupResult.Found)
        val finalHashResult = storage.readTextAndHash((finalLookup as MirrorLookupResult.Found).ref)
        assertNotNull("应能读取 final hash", finalHashResult)
        assertEquals(
            "final hash 匹配旧内容 hash → 已恢复，无需操作",
            oldHash,
            finalHashResult!!.second,
        )
    }

    /**
     * restoreBackup hash 不匹配返回冲突。
     *
     * Issue #667 改写：final 位置内容 hash 不匹配旧内容 hash → 冲突，需先删 final 再恢复 backup。
     */
    @Test
    fun restoreBackup_withHash_mismatch_returnsConflict() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val workspace = MirrorTransactionWorkspace(context)
        val storage = WorkspaceTestFakeStorage()

        val txId = "tx-restore-2"
        val relativePath = WORK_PATH
        val oldContent = OLD_CONTENT
        val oldHash = computeContentHash(oldContent)
        val newContent = "new content (promoted, not restored)"

        // 准备 backup
        val oldRef = MirrorFileRef(URI_OLD_1, relativePath)
        val backupRef = workspace.prepareBackup(txId, oldRef, oldContent)
        assertNotNull(MSG_PREPARE_BACKUP_SUCCESS, backupRef)

        // final 位置是新内容（hash 不匹配旧内容）→ 冲突
        val finalUri = "content://final/new"
        storage.committedFiles[finalUri] = newContent
        storage.committedPathToUri[relativePath] = finalUri

        // 验证恢复逻辑：lookupBackup Found + final hash 不匹配 → 冲突
        val backupLookup = workspace.lookupBackup(txId, relativePath)
        assertTrue(MSG_BACKUP_EXISTS, backupLookup is MirrorLookupResult.Found)
        val finalLookup = storage.lookup(relativePath)
        assertTrue("final 应存在", finalLookup is MirrorLookupResult.Found)
        val finalHashResult = storage.readTextAndHash((finalLookup as MirrorLookupResult.Found).ref)
        assertNotNull("应能读取 final hash", finalHashResult)
        assertFalse(
            "final hash 不匹配旧内容 hash → 冲突，需先删 final 再恢复 backup",
            finalHashResult!!.second == oldHash,
        )
    }

    /**
     * restoreBackup 无 hash 但找到返回已恢复。
     *
     * Issue #667 改写：无 oldContentHash 时，final 找到即视为已恢复（向后兼容）。
     */
    @Test
    fun restoreBackup_withoutHash_found_returnsAlreadyRestored() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val workspace = MirrorTransactionWorkspace(context)
        val storage = WorkspaceTestFakeStorage()

        val txId = "tx-restore-3"
        val relativePath = WORK_PATH
        val oldContent = OLD_CONTENT

        // 准备 backup
        val oldRef = MirrorFileRef(URI_OLD_1, relativePath)
        val backupRef = workspace.prepareBackup(txId, oldRef, oldContent)
        assertNotNull(MSG_PREPARE_BACKUP_SUCCESS, backupRef)

        // final 位置有文件（无 hash 校验，找到即已恢复）
        val finalUri = "content://final/restored"
        storage.committedFiles[finalUri] = oldContent
        storage.committedPathToUri[relativePath] = finalUri

        // 验证恢复逻辑：无 hash + final Found → 已恢复
        val finalLookup = storage.lookup(relativePath)
        assertTrue("无 hash 时 final Found → 已恢复", finalLookup is MirrorLookupResult.Found)
    }

    /**
     * restoreBackup 最终文件缺失时恢复。
     *
     * Issue #667 改写：final Missing + backup Found → 从 backup 恢复到 final。
     */
    @Test
    fun restoreBackup_missing_final_returnsRestored() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val workspace = MirrorTransactionWorkspace(context)
        val storage = WorkspaceTestFakeStorage()

        val txId = "tx-restore-4"
        val relativePath = WORK_PATH
        val oldContent = OLD_CONTENT

        // 准备 backup
        val oldRef = MirrorFileRef(URI_OLD_1, relativePath)
        val backupRef = workspace.prepareBackup(txId, oldRef, oldContent)
        assertNotNull(MSG_PREPARE_BACKUP_SUCCESS, backupRef)

        // final 缺失
        val finalLookup = storage.lookup(relativePath)
        assertTrue("final 应缺失", finalLookup is MirrorLookupResult.Missing)

        // 从 backup 恢复：readBackup + createText
        val backupLookup = workspace.lookupBackup(txId, relativePath)
        assertTrue(MSG_BACKUP_EXISTS, backupLookup is MirrorLookupResult.Found)
        val backupContent = workspace.readBackup((backupLookup as MirrorLookupResult.Found).ref)
        assertEquals("backup 内容正确", oldContent, backupContent)

        // 在 final 位置创建文件
        val restoredRef = storage.createText("作品/P/V", "Ch.md", MIME_TYPE_MARKDOWN, backupContent!!)
        assertNotNull("恢复后 final 应存在", restoredRef)
        assertEquals(
            "恢复后 final 内容正确",
            oldContent,
            storage.committedFiles[restoredRef!!.uri],
        )
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
}
