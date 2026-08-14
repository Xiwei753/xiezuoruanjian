package com.xiwei.sujian.feature.editor.ui

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #624 评论11 第5项 / 评论17 问题3：同步合并前置筛选只做 hash 比较。
 *
 * 旧实现 `isSyncMergeApplicable` 还比较 `content != currentContent` — 拿
 * 「同步后的磁盘正文」和「刚打开章节时的冷路径旧 UI 字符串」比较。评论9 之后
 * 本地正常输入不再更新 `_uiState.content`，这个比较会错误吞掉 hash 真变化的
 * 同步事实。
 *
 * #624 评论17 问题3：删除 SyncMergeEmitDedup hash 去重 — 发射端不维护
 * lastEmittedHash。Repository hash 与 documentCommittedVersion.contentHash
 * 不同即放行，真正的 Replay/Older/SameContent 判断只由 shouldApplyExternalContent 做。
 * 同 hash dirty conflict 事实不得被永久吞掉。
 */
class SyncMergePrefilterTest {
    /** hash 真变化即放行 — 不再拿冷路径正文做第二套比较，也不做 dedup。 */
    @Test
    fun hashChanged_passesWithoutColdPathContentComparison() {
        assertTrue(
            syncMergePrefilter(
                hash = "H2",
                currentHash = "H1",
            ),
        )
    }

    /** 空 hash 不可作为同步事实来源。 */
    @Test
    fun emptyHash_blocked() {
        assertFalse(
            syncMergePrefilter(
                hash = "",
                currentHash = "H1",
            ),
        )
    }

    /** hash 未变（磁盘与当前记录一致）— 无事可做。 */
    @Test
    fun equalHash_blocked() {
        assertFalse(
            syncMergePrefilter(
                hash = "H1",
                currentHash = "H1",
            ),
        )
    }

    /**
     * #624 评论17 问题3：同 hash 再次放行（dedup 已删除）—
     * dirty conflict 事实不被永久吞掉。
     */
    @Test
    fun sameHashRepeated_passes_noDedupSuppression() {
        assertTrue(syncMergePrefilter(hash = "H2", currentHash = "H1"))
        assertTrue("同 hash 再次放行（dedup 已删除）", syncMergePrefilter(hash = "H2", currentHash = "H1"))
    }
}
