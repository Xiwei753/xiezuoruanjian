package com.xiwei.sujian.feature.editor.ui

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #624 评论11 第5项：同步合并前置筛选只做 hash/dedup 判断。
 *
 * 旧实现 `isSyncMergeApplicable` 还比较 `content != currentContent` — 拿
 * 「同步后的磁盘正文」和「刚打开章节时的冷路径旧 UI 字符串」比较。评论9 之后
 * 本地正常输入不再更新 `_uiState.content`，这个比较会错误吞掉 hash 真变化的
 * 同步事实（例如：用户输入并 autosave 后，远端把正文改回与旧 UI 字符串相同的
 * 内容 — chapterHash 已前进，正文却与冷路径字符串相等，事实被提前吞掉）。
 * 正文相同/dirty/版本因果全部交给会话层
 * EditorSessionExternalOps.shouldApplyExternalContent（低频权威 snapshot 比较）。
 */
class SyncMergePrefilterTest {
    /** hash 真变化即放行 — 不再拿冷路径正文做第二套比较。 */
    @Test
    fun hashChanged_passesWithoutColdPathContentComparison() {
        assertTrue(
            syncMergePrefilter(
                hash = "H2",
                currentHash = "H1",
                shouldEmit = true,
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
                shouldEmit = true,
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
                shouldEmit = true,
            ),
        )
    }

    /** 同一 hash 已发射过（去重）— 不放行。 */
    @Test
    fun dedupSuppressed_blocked() {
        assertFalse(
            syncMergePrefilter(
                hash = "H2",
                currentHash = "H1",
                shouldEmit = false,
            ),
        )
    }
}
