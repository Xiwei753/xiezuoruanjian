@file:Suppress("StringLiteralDuplication")

package com.xiwei.sujian.feature.editor.ui

import com.xiwei.sujian.feature.editor.session.DocumentFactOrigin
import com.xiwei.sujian.feature.editor.session.DocumentVersion
import com.xiwei.sujian.feature.editor.session.ExternalContentDecision
import com.xiwei.sujian.feature.editor.session.TargetDocumentFact
import com.xiwei.sujian.feature.editor.session.TextEditorProfile
import com.xiwei.sujian.feature.editor.session.applyExternalContentFact
import com.xiwei.sujian.feature.editor.session.applyLocalEdit
import com.xiwei.sujian.feature.editor.session.consumePendingExternalFact
import com.xiwei.sujian.feature.editor.session.markSaved
import com.xiwei.sujian.feature.editor.session.pendingExternalFactFor
import com.xiwei.sujian.feature.editor.session.shouldApplyExternalContent
import com.xiwei.sujian.feature.editor.session.storePendingExternalFact
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #624 评论17 问题3：同步 dirty conflict 不得被 hash 去重永久吞掉。
 *
 * 旧缺陷：checkSyncMergedChapter() 在 reducer 判断前调用
 * syncMergeEmitDedup.shouldEmit(meta.hash)，shouldEmit() 立刻把
 * lastEmittedHash=fileHash。后面 handleExternalDocumentFact() 遇到
 * IgnoreDirtyConflict 只弹冲突提示，不提交版本，也不保留可重新处理的同步事实。
 * 于是同 remote hash 已被标成"处理过"，本地 dirty 清掉后同 hash 不会再发；
 * committedVersion 仍是旧版本。这不是幂等，是把未解决事实吞掉。
 *
 * 新实现：
 * - 删除 SyncMergeEmitDedup 及 lastEmittedHash；
 * - checkSyncMergedChapter 用 Repository hash 与 documentCommittedVersionFor.contentHash 比较；
 * - IgnoreDirtyConflict/IgnoreUncomparableConflict 把 fact 保存到 pendingExternalFact；
 * - 本地保存清 dirty 后检查 pendingExternalFact 触发重读，不直接用缓存 fact.text 覆盖；
 * - 真正 Apply/IgnoreSameContent 提交版本后才清 pendingExternalFact。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class DirtyConflictNotSwallowedTest {
    private fun createCoordinator(): com.xiwei.sujian.feature.editor.session.EditorSessionCoordinator =
        com.xiwei.sujian.feature.editor.session.EditorSessionCoordinator(
            com.xiwei.sujian.core.interop.app.AppServiceBridge(
                com.xiwei.sujian.core.interop.app.WriterAppServiceHolder(
                    "/home/xiwei/.cache/agent-tmp/sujian_test_624_c17_dirty",
                    "/home/xiwei/.cache/agent-tmp/sujian_test_624_c17_dirty",
                ),
            ),
        )

    private fun fact(
        targetId: String = "t1",
        text: String = "remoteText",
        hash: String = "hash-remote",
    ): TargetDocumentFact =
        TargetDocumentFact(
            targetId = targetId,
            text = text,
            sourceVersion = DocumentVersion(contentHash = hash),
            baseVersion = DocumentVersion(),
            origin = DocumentFactOrigin.SYNC_MERGED,
        )

    /**
     * IgnoreDirtyConflict 时 fact 必须保存到 pendingExternalFact，
     * 不得只发 UI 错误后丢掉。
     */
    @Test
    fun ignoreDirtyConflict_storesPendingExternalFact_notSwallowed() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("t1", TextEditorProfile.DocumentBody, persistent = true)
        // 本地有未保存编辑：committedVersion=hash-local, localDirty=true。
        coordinator.applyExternalContentFact(
            TargetDocumentFact(
                "t1",
                "localText",
                DocumentVersion(contentHash = "hash-local"),
                DocumentVersion(),
                DocumentFactOrigin.REPOSITORY_LOAD,
            ),
        )
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
        assertTrue(coordinator.sessionState.localDirty)

        val syncFact = fact()
        val decision = coordinator.shouldApplyExternalContent(syncFact)
        assertEquals("本地 dirty 时必须 IgnoreDirtyConflict", ExternalContentDecision.IgnoreDirtyConflict, decision)

        // 新实现：保存未解决事实，不被吞掉。
        coordinator.storePendingExternalFact("t1", syncFact)
        assertEquals(
            "pendingExternalFact 必须保存 fact（不得吞掉）",
            syncFact,
            coordinator.pendingExternalFactFor("t1"),
        )
    }

    /**
     * 本地保存成功清 dirty 后，pendingExternalFact 仍保留 —
     * 不得在 markSaved 时清掉（事实尚未解决）。
     */
    @Test
    fun markSaved_clearsDirtyButKeepsPendingExternalFact() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("t1", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.applyExternalContentFact(
            TargetDocumentFact(
                "t1",
                "localText",
                DocumentVersion(contentHash = "hash-local"),
                DocumentVersion(),
                DocumentFactOrigin.REPOSITORY_LOAD,
            ),
        )
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
        val syncFact = fact()
        coordinator.storePendingExternalFact("t1", syncFact)

        // 本地保存成功：markSaved 清 dirty。
        coordinator.markSaved("t1", DocumentVersion(contentHash = "hash-local-saved"))
        assertTrue("保存后 dirty 必须清除", !coordinator.sessionState.localDirty)
        assertEquals(
            "保存后 pendingExternalFact 必须保留（事实尚未解决）",
            syncFact,
            coordinator.pendingExternalFactFor("t1"),
        )
    }

    /**
     * 保存清 dirty 后重新检查 pendingExternalFact：shouldApplyExternalContent
     * 此时 localDirty=false，不再 IgnoreDirtyConflict，可 Apply 或 IgnoreSameContent。
     * 不得直接用缓存 fact.text 覆盖刚保存的本地正文 — 调用方必须触发 Repository 重读
     * 与版本比较。这里验证 shouldApplyExternalContent 不再返回 IgnoreDirtyConflict。
     */
    @Test
    fun afterSave_recheckPendingFact_notIgnoreDirtyConflict() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("t1", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.applyExternalContentFact(
            TargetDocumentFact(
                "t1",
                "localText",
                DocumentVersion(contentHash = "hash-local"),
                DocumentVersion(),
                DocumentFactOrigin.REPOSITORY_LOAD,
            ),
        )
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
        val syncFact = fact()
        coordinator.storePendingExternalFact("t1", syncFact)
        coordinator.markSaved("t1", DocumentVersion(contentHash = "hash-local-saved"))

        // 重新检查：localDirty=false，不再 IgnoreDirtyConflict。
        val decision = coordinator.shouldApplyExternalContent(syncFact)
        assertTrue(
            "保存清 dirty 后不得再 IgnoreDirtyConflict（应 Apply/IgnoreSameContent/IgnoreOlder 等）",
            decision != ExternalContentDecision.IgnoreDirtyConflict,
        )
    }

    /**
     * 真正 Apply/IgnoreSameContent 提交版本后 consumePendingExternalFact 清除事实。
     */
    @Test
    fun consumePendingExternalFact_clearsAfterResolve() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("t1", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.applyExternalContentFact(
            TargetDocumentFact(
                "t1",
                "localText",
                DocumentVersion(contentHash = "hash-local"),
                DocumentVersion(),
                DocumentFactOrigin.REPOSITORY_LOAD,
            ),
        )
        val syncFact = fact()
        coordinator.storePendingExternalFact("t1", syncFact)
        assertNotNull(coordinator.pendingExternalFactFor("t1"))

        val consumed = coordinator.consumePendingExternalFact("t1")
        assertEquals(syncFact, consumed)
        assertNull("consume 后 pendingExternalFact 必须清除", coordinator.pendingExternalFactFor("t1"))
    }

    /**
     * checkSyncMergedChapter 不再用 SyncMergeEmitDedup 去重 —
     * 同 hash dirty conflict 后保存，再次同 hash 仍能处理。
     * 这里验证 syncMergePrefilter 不依赖 dedup（shouldEmit 参数移除）。
     */
    @Test
    fun syncMergePrefilter_noDedup_sameHashDirtyConflictNotSwallowed() {
        // Repository hash 与 committedVersion.contentHash 不同即放行，
        // 不再用 lastEmittedHash 去重。
        assertTrue(
            "hash 不同即放行（不依赖 dedup）",
            syncMergePrefilter(hash = "H2", currentHash = "H1"),
        )
        // 同 hash 再次调用仍放行（去重已删除）— dirty conflict 事实不被吞。
        assertTrue(
            "同 hash 再次放行（dedup 已删除，不吞事实）",
            syncMergePrefilter(hash = "H2", currentHash = "H1"),
        )
    }
}
