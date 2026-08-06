package com.xiwei.sujian.editor.v2.coordinator

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 二：Reducer 同 hash 守卫契约测试。
 *
 * 规则（“正文事件版本顺序”一节）：同一个 hash 的事件无论正文当前是否因本地输入
 * 发生变化，都不能再次作为新 Repository 版本覆盖正文 — hash 相同意味着仓库/磁盘
 * 内容没有变化，正文差异只可能来自本地输入/未保存内容。
 *
 * 覆盖场景（issue 原文）：加载 H1 / 正文 A → 用户输入得到正文 B，lastRepositoryHash
 * 仍是 H1 → 旧 RepositoryLoaded(H1, A) 晚到 → 不得因正文不同而再次应用 A（本地
 * 未保存内容不得被旧加载事件覆盖）。SyncMerged 的同一规则：迟到的同 hash 合并
 * 事件不得覆盖本地输入。
 */
class RepositoryHashGuardContractTest {

    private fun createCoordinator(): EditorSessionCoordinator {
        return EditorSessionCoordinator(com.xiwei.sujian.data.AppServiceBridge(
            com.xiwei.sujian.data.WriterAppServiceHolder("/tmp/sujian_test_workspace_595_hashguard")
        ))
    }

    @Test
    fun repositoryLoaded_sameHash_neverReappliesOverLocalInput() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))

        // 加载 H1 / 正文 A
        val load = EditorDocumentUpdate.RepositoryLoaded("t1", "A", fileHash = "hash-1", revision = 0L, contentVersion = 1L)
        assertTrue(coordinator.shouldApplyRepositoryLoad(load))
        coordinator.applyRepositoryLoaded(load)
        assertEquals("hash-1", coordinator.sessionState.lastRepositoryHash)

        // 用户输入得到正文 B（lastRepositoryHash 仍是 H1）
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("t1", "B", revision = 1L, contentVersion = 2L, transactionId = 7L)
        )
        assertEquals("B", coordinator.sessionState.text)

        // 旧的 RepositoryLoaded(H1, A) 晚到（contentVersion 更新，但 hash 相同）
        val lateLoad = EditorDocumentUpdate.RepositoryLoaded("t1", "A", fileHash = "hash-1", revision = 0L, contentVersion = 3L)
        assertFalse(
            "#595 二：同一 hash 的加载事件不得覆盖本地输入（issue：旧 RepositoryLoaded " +
            "因正文不同再次应用 A → 本地未保存内容被旧加载事件覆盖）",
            coordinator.shouldApplyRepositoryLoad(lateLoad),
        )
        // 状态必须保持本地输入 B
        assertEquals("B", coordinator.sessionState.text)
        assertEquals(EditorSessionOrigin.LOCAL_INPUT, coordinator.sessionState.origin)
    }

    @Test
    fun repositoryLoaded_sameHash_replayIsIdempotent() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))

        val first = EditorDocumentUpdate.RepositoryLoaded("t1", "A", fileHash = "hash-1", revision = 0L, contentVersion = 1L)
        assertTrue(coordinator.shouldApplyRepositoryLoad(first))
        coordinator.applyRepositoryLoaded(first)

        // 同一 hash + 同一正文重放：幂等，不 reset。
        assertFalse(
            "同一 hash 重放必须被拒绝（无论正文是否相同）",
            coordinator.shouldApplyRepositoryLoad(first),
        )
    }

    @Test
    fun repositoryLoaded_newHash_stillApplies() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))

        val first = EditorDocumentUpdate.RepositoryLoaded("t1", "A", fileHash = "hash-1", revision = 0L, contentVersion = 1L)
        coordinator.applyRepositoryLoaded(first)

        // 新 hash（仓库内容真实变化）→ 仍可应用。
        val second = EditorDocumentUpdate.RepositoryLoaded("t1", "C", fileHash = "hash-2", revision = 0L, contentVersion = 2L)
        assertTrue("新 hash 的加载事件必须可应用", coordinator.shouldApplyRepositoryLoad(second))
    }

    @Test
    fun syncMerged_sameHash_neverReappliesOverLocalInput() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))

        // 同步合并 H1 → 应用
        val merge = EditorDocumentUpdate.SyncMerged("t1", "C", manifestRevision = 1L, fileHash = "hash-1", revision = 0L, contentVersion = 1L)
        assertTrue(coordinator.shouldApplyExternalUpdate(merge))
        coordinator.applySyncMerged(merge)
        assertEquals("hash-1", coordinator.sessionState.lastRepositoryHash)

        // 用户输入得到 D（lastRepositoryHash 仍是 H1）
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("t1", "D", revision = 1L, contentVersion = 2L, transactionId = 9L)
        )

        // 下一个同步周期重复发射同一 hash 的合并事件（contentVersion 更新）
        val reEmit = EditorDocumentUpdate.SyncMerged("t1", "C", manifestRevision = 2L, fileHash = "hash-1", revision = 0L, contentVersion = 3L)
        assertFalse(
            "#595 二：同一 hash 的合并事件不得覆盖本地输入 — 磁盘正文未变化时，" +
            "正文差异只可能来自本地输入",
            coordinator.shouldApplyExternalUpdate(reEmit),
        )
        assertEquals("D", coordinator.sessionState.text)
        assertEquals(EditorSessionOrigin.LOCAL_INPUT, coordinator.sessionState.origin)
    }

    @Test
    fun syncMerged_newHash_stillApplies() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))

        val first = EditorDocumentUpdate.SyncMerged("t1", "C", manifestRevision = 1L, fileHash = "hash-1", revision = 0L, contentVersion = 1L)
        coordinator.applySyncMerged(first)

        // 新 hash（磁盘内容真实变化）→ 仍可应用。
        val second = EditorDocumentUpdate.SyncMerged("t1", "E", manifestRevision = 2L, fileHash = "hash-2", revision = 0L, contentVersion = 2L)
        assertTrue("新 hash 的合并事件必须可应用", coordinator.shouldApplyExternalUpdate(second))
    }

    @Test
    fun syncMerged_emptyHash_usesVersionAndTextRulesOnly() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))

        // 空 fileHash 不触发 hash 守卫 — 版本旧则拒绝，版本新且正文不同则应用。
        val first = EditorDocumentUpdate.SyncMerged("t1", "C", manifestRevision = 1L, fileHash = "", revision = 0L, contentVersion = 1L)
        assertTrue(coordinator.shouldApplyExternalUpdate(first))
        coordinator.applySyncMerged(first)

        val old = EditorDocumentUpdate.SyncMerged("t1", "X", manifestRevision = 1L, fileHash = "", revision = 0L, contentVersion = 0L)
        assertFalse("旧 contentVersion 必须被拒绝", coordinator.shouldApplyExternalUpdate(old))
    }
}
