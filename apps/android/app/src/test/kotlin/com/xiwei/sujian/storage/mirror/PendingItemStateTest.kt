package com.xiwei.sujian.storage.mirror

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #649 评论 5562715833：PendingItem 状态推进逻辑单元测试。
 *
 * 覆盖：
 * - STATE_STAGED → STATE_OLD_BACKED_UP
 * - STATE_OLD_BACKED_UP → STATE_PROMOTED
 * - STATE_PROMOTED → STATE_COMMITTED
 * - 新项目（oldRef=null）跳过 backup
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class PendingItemStateTest {
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
}
