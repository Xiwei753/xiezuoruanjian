@file:Suppress("StringLiteralDuplication")

package com.xiwei.sujian.feature.editor.presentation

import com.xiwei.sujian.feature.editor.session.DocumentFactOrigin
import com.xiwei.sujian.feature.editor.session.DocumentVersion
import com.xiwei.sujian.feature.editor.session.ExternalContentDecision
import com.xiwei.sujian.feature.editor.session.PendingExternalVersion
import com.xiwei.sujian.feature.editor.session.TargetDocumentFact
import com.xiwei.sujian.feature.editor.session.TextEditorProfile
import com.xiwei.sujian.feature.editor.session.applyExternalContentFact
import com.xiwei.sujian.feature.editor.session.applyLocalEdit
import com.xiwei.sujian.feature.editor.session.consumePendingExternalFact
import com.xiwei.sujian.feature.editor.session.documentCommittedVersionFor
import com.xiwei.sujian.feature.editor.session.markSaved
import com.xiwei.sujian.feature.editor.session.pendingExternalFactFor
import com.xiwei.sujian.feature.editor.session.shouldApplyExternalContent
import com.xiwei.sujian.feature.editor.session.storePendingExternalFact
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #624 评论17 问题5：pendingExternal reapply 必须保留原 origin，不得硬编码 SYNC_MERGED。
 *
 * 旧缺陷：`EditorSaveOps` 保存成功后调用 `checkSyncMergedChapter()`，后者
 * 1. 不查询 `pendingExternalFactFor(targetId)` — 无条件重读 Repository；
 * 2. 始终用 `origin = DocumentFactOrigin.SYNC_MERGED` 构造 TargetDocumentFact。
 *
 * 若一个 `REPOSITORY_LOAD` origin 的外部事实因 localDirty 被
 * `IgnoreDirtyConflict` 拦截并存入 pendingExternal（origin=REPOSITORY_LOAD），
 * 用户保存清 dirty 后：
 * - 旧 `checkSyncMergedChapter()` 重读 Repository，用 `SYNC_MERGED` origin 发射；
 * - `shouldApplyExternalContent` 对不可比较版本：`SYNC_MERGED`→
 *   `IgnoreUncomparableConflict`（重新存 pending，永不解决）；
 * - 正确行为：用 `REPOSITORY_LOAD` origin→`Apply`（解决）。
 *
 * 新实现：`buildPendingReapplyFact` 纯函数从 pending 取 origin，从 Repository
 * 读最新正文/hash 构造 TargetDocumentFact。`reapplyPendingExternalAfterSave`
 * 在保存成功后调用，替换无条件 `checkSyncMergedChapter()`。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class PendingExternalReapplyOriginTest {
    private fun createCoordinator(): com.xiwei.sujian.feature.editor.session.EditorSessionCoordinator =
        com.xiwei.sujian.feature.editor.session.EditorSessionCoordinator(
            com.xiwei.sujian.core.interop.app.AppServiceBridge(
                com.xiwei.sujian.core.interop.app.WriterAppServiceHolder(
                    "/home/xiwei/.cache/agent-tmp/sujian_test_624_c17_reapply",
                    "/home/xiwei/.cache/agent-tmp/sujian_test_624_c17_reapply",
                ),
            ),
        )

    /** REPOSITORY_LOAD origin 的 pending — reapply 必须保留这个 origin。 */
    @Test
    fun buildPendingReapplyFact_withRepositoryLoadOrigin_preservesOrigin() {
        val pending =
            PendingExternalVersion(
                sourceVersion = DocumentVersion(contentHash = "hash-pending-old"),
                origin = DocumentFactOrigin.REPOSITORY_LOAD,
            )
        val fact =
            buildPendingReapplyFact(
                pending = pending,
                targetId = "t1",
                repositoryContent = "repo-text",
                repositoryHash = "hash-repo-new",
                baseVersion = DocumentVersion(),
                syncCommitId = null,
            )
        assertEquals(
            "REPOSITORY_LOAD pending reapply 必须保留 REPOSITORY_LOAD origin",
            DocumentFactOrigin.REPOSITORY_LOAD,
            fact.origin,
        )
    }

    /** SYNC_MERGED origin 的 pending — reapply 同样保留这个 origin。 */
    @Test
    fun buildPendingReapplyFact_withSyncMergedOrigin_preservesOrigin() {
        val pending =
            PendingExternalVersion(
                sourceVersion = DocumentVersion(contentHash = "hash-pending-old"),
                origin = DocumentFactOrigin.SYNC_MERGED,
            )
        val fact =
            buildPendingReapplyFact(
                pending = pending,
                targetId = "t1",
                repositoryContent = "repo-text",
                repositoryHash = "hash-repo-new",
                baseVersion = DocumentVersion(),
                syncCommitId = "commit-xyz",
            )
        assertEquals(
            "SYNC_MERGED pending reapply 必须保留 SYNC_MERGED origin",
            DocumentFactOrigin.SYNC_MERGED,
            fact.origin,
        )
    }

    /**
     * Repository 重读得到新 hash — fact.sourceVersion.contentHash 必须用新 hash，
     * 不得拿 pending 里旧 hash（pending 只存 sourceVersion 用于冲突判定锚点，
     * 真正事实的版本来自当前 Repository 读结果）。
     */
    @Test
    fun buildPendingReapplyFact_usesRepositoryHashNotPendingHash() {
        val pending =
            PendingExternalVersion(
                sourceVersion = DocumentVersion(contentHash = "hash-pending-stale"),
                origin = DocumentFactOrigin.REPOSITORY_LOAD,
            )
        val fact =
            buildPendingReapplyFact(
                pending = pending,
                targetId = "t1",
                repositoryContent = "repo-text-fresh",
                repositoryHash = "hash-repo-fresh",
                baseVersion = DocumentVersion(contentHash = "hash-base"),
                syncCommitId = null,
            )
        assertEquals(
            "fact.sourceVersion.contentHash 必须用 Repository 新 hash，不是 pending 旧 hash",
            "hash-repo-fresh",
            fact.sourceVersion.contentHash,
        )
        assertNotEquals(
            "不得误用 pending 旧 hash",
            "hash-pending-stale",
            fact.sourceVersion.contentHash,
        )
        assertEquals(
            "fact.text 必须用 Repository 新正文",
            "repo-text-fresh",
            fact.text,
        )
        assertEquals(
            "fact.baseVersion 必须传入的 baseVersion",
            DocumentVersion(contentHash = "hash-base"),
            fact.baseVersion,
        )
    }

    /**
     * 集成验证：REPOSITORY_LOAD pending + 不可比较版本，保存清 dirty 后
     * 用 buildPendingReapplyFact 构造的 fact 调 shouldApplyExternalContent
     * 返回 Apply（解决冲突）。对比：用 SYNC_MERGED origin 构造同样 fact
     * 调 shouldApplyExternalContent 返回 IgnoreUncomparableConflict —
     * 证明 origin 确实改变决策，旧实现硬编码 SYNC_MERGED 导致永不解决。
     */
    @Test
    fun reapplyWithRepositoryLoadOrigin_resolvesConflict_syncMergedOriginDoesNot() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("t1", TextEditorProfile.DocumentBody, persistent = true)
        // 本地建立版本事实：committedVersion = hash-local。
        coordinator.applyExternalContentFact(
            TargetDocumentFact(
                targetId = "t1",
                text = "localText",
                sourceVersion = DocumentVersion(contentHash = "hash-local"),
                baseVersion = DocumentVersion(),
                origin = DocumentFactOrigin.REPOSITORY_LOAD,
            ),
        )
        // 本地编辑：localDirty=true。
        coordinator.applyLocalEdit(
            com.xiwei.sujian.feature.editor.session.EditorDocumentUpdate.LocalInput(
                targetId = "t1",
                operationKind = com.xiwei.sujian.feature.editor.session.EditorOperationKind.INSERT,
                contentChanged = true,
                contentDelta = com.xiwei.sujian.feature.editor.session.EditorContentDelta(insertedChars = 1),
                revision = 2L,
                transactionId = 1L,
            ),
        )

        // 外部 REPOSITORY_LOAD 事实，版本与本地不可比较（不同 hash、无共同父链）。
        val externalFact =
            TargetDocumentFact(
                targetId = "t1",
                text = "externalText",
                sourceVersion =
                    DocumentVersion(
                        contentHash = "hash-external",
                        parentVersion = DocumentVersion(contentHash = "hash-external-parent"),
                    ),
                baseVersion = DocumentVersion(),
                origin = DocumentFactOrigin.REPOSITORY_LOAD,
            )
        // dirty 时必须 IgnoreDirtyConflict。
        assertEquals(
            "dirty 时必须 IgnoreDirtyConflict",
            ExternalContentDecision.IgnoreDirtyConflict,
            coordinator.shouldApplyExternalContent(externalFact),
        )
        // 保存未解决事实（origin=REPOSITORY_LOAD）。
        coordinator.storePendingExternalFact("t1", externalFact)
        val pending = coordinator.pendingExternalFactFor("t1")
        assertEquals(
            "pending 必须保留 REPOSITORY_LOAD origin",
            DocumentFactOrigin.REPOSITORY_LOAD,
            pending?.origin,
        )

        // 用户保存成功：markSaved 清 dirty，committedVersion 推进到 hash-local-saved。
        coordinator.markSaved("t1", DocumentVersion(contentHash = "hash-local-saved"))

        // 保存后用 buildPendingReapplyFact 重读构造 fact —
        // Repository 此时读到新正文/新 hash（同步已落盘），origin 来自 pending=REPOSITORY_LOAD。
        val reapplyFact =
            buildPendingReapplyFact(
                pending = pending!!,
                targetId = "t1",
                repositoryContent = "externalText",
                repositoryHash = "hash-external",
                baseVersion = coordinator.documentCommittedVersionFor("t1"),
                syncCommitId = null,
            )
        val reapplyDecision = coordinator.shouldApplyExternalContent(reapplyFact)
        assertEquals(
            "REPOSITORY_LOAD origin + 不可比较版本 → Apply（冲突解决）",
            ExternalContentDecision.Apply,
            reapplyDecision,
        )

        // 对比：旧实现硬编码 SYNC_MERGED origin 构造同样 fact —
        // shouldApplyExternalContent 返回 IgnoreUncomparableConflict，永不解决。
        val syncMergedFact =
            reapplyFact.copy(origin = DocumentFactOrigin.SYNC_MERGED)
        val syncMergedDecision = coordinator.shouldApplyExternalContent(syncMergedFact)
        assertEquals(
            "SYNC_MERGED origin + 不可比较版本 → IgnoreUncomparableConflict（旧缺陷：永不解决）",
            ExternalContentDecision.IgnoreUncomparableConflict,
            syncMergedDecision,
        )
    }

    /**
     * #624 评论17 问题5：reapply fact 必须标记 isReapply = true，
     * 让 handleExternalDocumentFact 在 IgnoreReplay/IgnoreOlder 分支也消费 pending。
     */
    @Test
    fun buildPendingReapplyFact_setsIsReapplyFlag() {
        val pending =
            PendingExternalVersion(
                sourceVersion = DocumentVersion(contentHash = "hash-old"),
                origin = DocumentFactOrigin.REPOSITORY_LOAD,
            )
        val fact =
            buildPendingReapplyFact(
                pending = pending,
                targetId = "t1",
                repositoryContent = "text",
                repositoryHash = "hash-new",
                baseVersion = DocumentVersion(),
                syncCommitId = null,
            )
        assertTrue("reapply fact 必须标记 isReapply = true", fact.isReapply)
    }

    /**
     * #624 评论17 问题5：shouldConsumePendingAfterFact 纯函数 —
     * reapply fact 的 IgnoreReplay/IgnoreOlder 应消费 pending。
     */
    @Test
    fun shouldConsumePending_reapplyIgnoreReplay_consumes() {
        assertTrue(shouldConsumePendingAfterFact(ExternalContentDecision.IgnoreReplay, isReapply = true))
        assertTrue(shouldConsumePendingAfterFact(ExternalContentDecision.IgnoreOlder, isReapply = true))
    }

    @Test
    fun shouldConsumePending_normalIgnoreReplay_doesNotConsume() {
        assertFalse(shouldConsumePendingAfterFact(ExternalContentDecision.IgnoreReplay, isReapply = false))
        assertFalse(shouldConsumePendingAfterFact(ExternalContentDecision.IgnoreOlder, isReapply = false))
    }

    @Test
    fun shouldConsumePending_applyAndSameContent_alwaysConsumes() {
        assertTrue(shouldConsumePendingAfterFact(ExternalContentDecision.Apply, isReapply = false))
        assertTrue(shouldConsumePendingAfterFact(ExternalContentDecision.IgnoreSameContent, isReapply = false))
        assertTrue(shouldConsumePendingAfterFact(ExternalContentDecision.Apply, isReapply = true))
    }

    @Test
    fun shouldConsumePending_dirtyAndUncomparable_neverConsumes() {
        assertFalse(shouldConsumePendingAfterFact(ExternalContentDecision.IgnoreDirtyConflict, isReapply = true))
        assertFalse(shouldConsumePendingAfterFact(ExternalContentDecision.IgnoreUncomparableConflict, isReapply = true))
        assertFalse(shouldConsumePendingAfterFact(ExternalContentDecision.IgnoreEmptyVersion, isReapply = true))
    }

    /**
     * #624 评论17 问题5 集成：IgnoreReplay 场景（Repository hash == savedHash）
     * 下 reapply fact 必须消费 pending，避免泄漏。
     */
    @Test
    fun reapplyIgnoreReplay_consumesPending() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("t1", TextEditorProfile.DocumentBody, persistent = true)
        // 建立版本事实
        coordinator.applyExternalContentFact(
            TargetDocumentFact(
                targetId = "t1",
                text = "text",
                sourceVersion = DocumentVersion(contentHash = "hash-local"),
                baseVersion = DocumentVersion(),
                origin = DocumentFactOrigin.REPOSITORY_LOAD,
            ),
        )
        // 本地编辑 dirty
        coordinator.applyLocalEdit(
            com.xiwei.sujian.feature.editor.session.EditorDocumentUpdate.LocalInput(
                targetId = "t1",
                operationKind = com.xiwei.sujian.feature.editor.session.EditorOperationKind.INSERT,
                contentChanged = true,
                contentDelta = com.xiwei.sujian.feature.editor.session.EditorContentDelta(insertedChars = 1),
                revision = 2L,
                transactionId = 1L,
            ),
        )
        // REPOSITORY_LOAD fact dirty → IgnoreDirtyConflict → store pending
        val externalFact =
            TargetDocumentFact(
                targetId = "t1",
                text = "externalText",
                sourceVersion = DocumentVersion(contentHash = "hash-external"),
                baseVersion = DocumentVersion(),
                origin = DocumentFactOrigin.REPOSITORY_LOAD,
            )
        assertEquals(
            "dirty 时必须 IgnoreDirtyConflict",
            ExternalContentDecision.IgnoreDirtyConflict,
            coordinator.shouldApplyExternalContent(externalFact),
        )
        coordinator.storePendingExternalFact("t1", externalFact)
        assertNotNull("pending 已存储", coordinator.pendingExternalFactFor("t1"))
        // 保存成功 → committedVersion = hash-saved
        coordinator.markSaved("t1", DocumentVersion(contentHash = "hash-saved"))
        // reapply：Repository hash == savedHash → IgnoreReplay
        val reapplyFact =
            buildPendingReapplyFact(
                pending = coordinator.pendingExternalFactFor("t1")!!,
                targetId = "t1",
                repositoryContent = "savedText",
                repositoryHash = "hash-saved",
                baseVersion = coordinator.documentCommittedVersionFor("t1"),
                syncCommitId = null,
            )
        assertEquals(
            "Repository hash == savedHash → IgnoreReplay",
            ExternalContentDecision.IgnoreReplay,
            coordinator.shouldApplyExternalContent(reapplyFact),
        )
        assertTrue(
            "isReapply fact + IgnoreReplay → 应消费 pending",
            shouldConsumePendingAfterFact(ExternalContentDecision.IgnoreReplay, reapplyFact.isReapply),
        )
        // 模拟消费
        coordinator.consumePendingExternalFact("t1")
        assertNull("pending 应被消费", coordinator.pendingExternalFactFor("t1"))
    }
}
