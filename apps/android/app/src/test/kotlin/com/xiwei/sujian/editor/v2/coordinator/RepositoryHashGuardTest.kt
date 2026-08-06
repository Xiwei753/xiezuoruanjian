@file:Suppress("StringLiteralDuplication") // 测试固件字符串天然重复
package com.xiwei.sujian.editor.v2.coordinator

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 二：Reducer 文档版本守卫契约测试。
 *
 * 规则（issue 解决二）：
 * - 同 sourceVersion 重放 → IgnoreReplay（幂等，新 collector 读到当前文档事实
 *   也不会再次执行副作用）；
 * - localDirty=true → IgnoreDirtyConflict — 禁止直接 reset（本地未保存内容
 *   不得被旧加载/迟到合并事件覆盖）；
 * - 正文一致 → IgnoreSameContent（无需 reset）。
 */
class RepositoryHashGuardContractTest {

    private fun createCoordinator(): EditorSessionCoordinator {
        return EditorSessionCoordinator(com.xiwei.sujian.data.AppServiceBridge(
            com.xiwei.sujian.data.WriterAppServiceHolder("/tmp/sujian_test_workspace_595_hashguard")
        ))
    }

    @Test
    fun repositoryLoad_sameVersion_neverReappliesOverLocalInput() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))

        // 加载 H1 / 正文 A
        val load = TargetDocumentFact(
            "t1", "A",
            DocumentVersion(contentHash = "hash-1"),
            DocumentVersion(),
            DocumentFactOrigin.REPOSITORY_LOAD,
        )
        coordinator.applyExternalContentFact(load)
        assertEquals("hash-1", coordinator.sessionState.committedVersion.contentHash)

        // 用户输入得到正文 B（committedVersion 仍是 H1，localDirty=true）
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("t1", "B", revision = 1L, transactionId = 7L)
        )
        assertEquals("B", coordinator.sessionState.text)
        assertTrue(coordinator.sessionState.localDirty)

        // 旧的 RepositoryLoaded(H1, A) 重放 → 幂等忽略（同 sourceVersion 先于
        // dirty 判断），不得覆盖本地输入（issue：旧加载事件不得覆盖本地未保存内容）。
        val decision = coordinator.shouldApplyExternalContent(load)
        assertTrue(
            "#595 二：同版本重放必须被忽略（IgnoreReplay），不得覆盖本地输入",
            decision == ExternalContentDecision.IgnoreReplay ||
                decision == ExternalContentDecision.IgnoreDirtyConflict,
        )
        // 状态必须保持本地输入 B
        assertEquals("B", coordinator.sessionState.text)
        assertEquals(EditorSessionOrigin.LOCAL_INPUT, coordinator.sessionState.origin)
    }

    @Test
    fun repositoryLoad_sameVersion_notDirty_replayIsIdempotent() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))

        val first = TargetDocumentFact(
            "t1", "A",
            DocumentVersion(contentHash = "hash-1"),
            DocumentVersion(),
            DocumentFactOrigin.REPOSITORY_LOAD,
        )
        coordinator.applyExternalContentFact(first)

        // 同一 sourceVersion 重放：幂等忽略。
        assertEquals(
            "同 sourceVersion 重放必须 IgnoreReplay",
            ExternalContentDecision.IgnoreReplay,
            coordinator.shouldApplyExternalContent(first),
        )
    }

    @Test
    fun repositoryLoad_newVersion_stillApplies() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))

        val first = TargetDocumentFact(
            "t1", "A",
            DocumentVersion(contentHash = "hash-1"),
            DocumentVersion(),
            DocumentFactOrigin.REPOSITORY_LOAD,
        )
        coordinator.applyExternalContentFact(first)

        // 新版本（仓库内容真实变化）→ 仍可应用（#595 五：加载事实携带
        // parentVersion=上次已知版本，与 committed 构成因果链）。
        val second = TargetDocumentFact(
            "t1", "C",
            DocumentVersion(contentHash = "hash-2", parentVersion = DocumentVersion(contentHash = "hash-1")),
            DocumentVersion(contentHash = "hash-1"),
            DocumentFactOrigin.REPOSITORY_LOAD,
        )
        assertEquals("新版本的加载事实必须可应用", ExternalContentDecision.Apply, coordinator.shouldApplyExternalContent(second))
    }

    @Test
    fun syncMerged_sameVersion_neverReappliesOverLocalInput() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))

        // 同步合并 H1 → 记录版本
        val merge = TargetDocumentFact(
            "t1", "C",
            DocumentVersion(contentHash = "hash-1", syncCommitId = "commit-1"),
            DocumentVersion(),
            DocumentFactOrigin.SYNC_MERGED,
        )
        coordinator.applyExternalContentFact(merge)
        assertEquals("hash-1", coordinator.sessionState.committedVersion.contentHash)

        // 用户输入得到 D（committedVersion 仍是 H1，localDirty=true）
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("t1", "D", revision = 1L, transactionId = 9L)
        )

        // 下一个同步周期重复发射同一版本的合并事实 → 幂等忽略，不得覆盖本地输入。
        val decision = coordinator.shouldApplyExternalContent(merge)
        assertTrue(
            "#595 二：同版本合并事实必须被忽略（IgnoreReplay），不得覆盖本地输入",
            decision == ExternalContentDecision.IgnoreReplay ||
                decision == ExternalContentDecision.IgnoreDirtyConflict,
        )
        assertEquals("D", coordinator.sessionState.text)
        assertEquals(EditorSessionOrigin.LOCAL_INPUT, coordinator.sessionState.origin)
    }

    @Test
    fun syncMerged_newVersion_stillApplies() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))

        val first = TargetDocumentFact(
            "t1", "C",
            DocumentVersion(contentHash = "hash-1", syncCommitId = "commit-1"),
            DocumentVersion(),
            DocumentFactOrigin.SYNC_MERGED,
        )
        coordinator.applyExternalContentFact(first)

        // 新版本（磁盘内容真实变化）→ 仍可应用。
        val second = TargetDocumentFact(
            "t1", "E",
            DocumentVersion(contentHash = "hash-2", syncCommitId = "commit-2", parentVersion = DocumentVersion(contentHash = "hash-1")),
            DocumentVersion(contentHash = "hash-1"),
            DocumentFactOrigin.SYNC_MERGED,
        )
        assertEquals("新版本的合并事实必须可应用", ExternalContentDecision.Apply, coordinator.shouldApplyExternalContent(second))
    }

    @Test
    fun syncMerged_emptyVersion_isRejected() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))

        val empty = TargetDocumentFact(
            "t1", "C", DocumentVersion(), DocumentVersion(), DocumentFactOrigin.SYNC_MERGED,
        )
        assertEquals("空版本锚点必须被拒绝", ExternalContentDecision.IgnoreEmptyVersion, coordinator.shouldApplyExternalContent(empty))
    }

    @Test
    fun sameContentDifferentVersion_isIgnoredWithoutReset() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))

        // 会话正文已是 "C"（如预准备 session 装载），外部事实正文相同 → IgnoreSameContent。
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("t1", "C", revision = 1L, transactionId = 1L)
        )
        coordinator.markSaved("t1", DocumentVersion(contentHash = "saved-c"))
        assertFalse(coordinator.sessionState.localDirty)

        val sameText = TargetDocumentFact(
            "t1", "C",
            DocumentVersion(contentHash = "hash-2"),
            DocumentVersion(contentHash = "hash-1"),
            DocumentFactOrigin.REPOSITORY_LOAD,
        )
        assertEquals(
            ExternalContentDecision.IgnoreSameContent,
            coordinator.shouldApplyExternalContent(sameText),
        )
    }
}
