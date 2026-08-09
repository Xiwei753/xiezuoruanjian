@file:Suppress("StringLiteralDuplication") // 测试固件字符串天然重复

package com.xiwei.sujian.feature.editor.session

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import com.xiwei.sujian.feature.editor.window.EditableTextTarget

/**
 * #595 五/六：文档版本因果顺序与保存提交契约测试。
 *
 * - repositoryRevision 单调锚点可比较（更旧 → IgnoreOlder）；
 * - 父版本链（parentVersion）含 committed → 后代版本可应用；
 * - 无共同锚点且父链不含 committed 的不同版本 → 不得默认 Apply（类型化冲突）；
 * - 空 committed（从未建立版本事实）→ 首次应用；
 * - 保存成功（markSaved）原子推进 committed/sessionBase/lastSaved + 清 localDirty；
 * - 外部事实应用后 sessionBaseVersion = sourceVersion（不再保留旧 base）；
 * - baseVersion 按 target 从 store 读取（documentCommittedVersionFor）。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class DocumentVersionCausalityTest {
    private fun createCoordinator(): EditorSessionCoordinator {
        return EditorSessionCoordinator(
            com.xiwei.sujian.core.interop.app.AppServiceBridge(
                com.xiwei.sujian.core.interop.app.WriterAppServiceHolder(
                    "/tmp/sujian_test_workspace_595_causality",
                    "/tmp/sujian_test_workspace_595_causality",
                ),
            ),
        )
    }

    private fun lease(targetId: String): EditorInputLease = EditorInputLease(targetId, 0UL, 0L)

    @Test
    fun firstFactWithEmptyCommitted_isApplicable() {
        // 章节首次加载：committed 为空 → 直接可应用（即使版本本身不可比较）。
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))
        val first =
            TargetDocumentFact(
                "t1",
                "repo v1",
                DocumentVersion(contentHash = "hash-1", syncCommitId = "commit-1"),
                DocumentVersion(),
                DocumentFactOrigin.REPOSITORY_LOAD,
            )
        assertEquals(ExternalContentDecision.Apply, coordinator.shouldApplyExternalContent(first))
    }

    @Test
    fun unrelatedVersion_doesNotDefaultToApply() {
        // #595 五：不同 hash、无共同 revision 锚点、父链不含 committed → 冲突。
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))
        coordinator.applyExternalContentFact(
            TargetDocumentFact(
                "t1",
                "v1",
                DocumentVersion(contentHash = "hash-1"),
                DocumentVersion(),
                DocumentFactOrigin.REPOSITORY_LOAD,
            ),
        )
        val unrelated =
            TargetDocumentFact(
                "t1",
                "v9",
                DocumentVersion(contentHash = "hash-9", syncCommitId = "commit-9"),
                DocumentVersion(),
                DocumentFactOrigin.SYNC_MERGED,
            )
        assertEquals(
            ExternalContentDecision.IgnoreUncomparableConflict,
            coordinator.shouldApplyExternalContent(unrelated),
        )
    }

    @Test
    fun descendantViaParentChain_isApplicable() {
        // #595 五：同步合并结果携带 parentVersion=同步前磁盘版本 → 可比较 → Apply。
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))
        coordinator.applyExternalContentFact(
            TargetDocumentFact(
                "t1",
                "saved",
                DocumentVersion(contentHash = "hash-1"),
                DocumentVersion(),
                DocumentFactOrigin.REPOSITORY_LOAD,
            ),
        )
        val merged =
            TargetDocumentFact(
                "t1",
                "merged",
                DocumentVersion(
                    contentHash = "hash-2",
                    syncCommitId = "commit-2",
                    parentVersion = DocumentVersion(contentHash = "hash-1"),
                ),
                DocumentVersion(contentHash = "hash-1"),
                DocumentFactOrigin.SYNC_MERGED,
            )
        assertEquals(ExternalContentDecision.Apply, coordinator.shouldApplyExternalContent(merged))
    }

    @Test
    fun descendantViaGrandparentChain_isApplicable() {
        // 父链可以多级：committed 是 incoming 的祖父 → 仍可比较。
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))
        coordinator.applyExternalContentFact(
            TargetDocumentFact(
                "t1",
                "v1",
                DocumentVersion(contentHash = "hash-1"),
                DocumentVersion(),
                DocumentFactOrigin.REPOSITORY_LOAD,
            ),
        )
        val merged =
            TargetDocumentFact(
                "t1",
                "v3",
                DocumentVersion(
                    contentHash = "hash-3",
                    parentVersion =
                        DocumentVersion(
                            contentHash = "hash-2",
                            parentVersion = DocumentVersion(contentHash = "hash-1"),
                        ),
                ),
                DocumentVersion(),
                DocumentFactOrigin.SYNC_MERGED,
            )
        assertEquals(ExternalContentDecision.Apply, coordinator.shouldApplyExternalContent(merged))
    }

    @Test
    fun markSaved_advancesAllDocumentVersionsAtomically() {
        // #595 六：保存回执必须原子推进 committedVersion/sessionBaseVersion/
        // lastSavedVersion + 清除 localDirty（ViewModel/SessionStore/Repository
        // 不得分别更新一部分）。
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))
        coordinator.applyExternalContentFact(
            TargetDocumentFact(
                "t1",
                "old",
                DocumentVersion(contentHash = "hash-1"),
                DocumentVersion(),
                DocumentFactOrigin.REPOSITORY_LOAD,
            ),
        )
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("t1", "edited", 3L, 7L, lease = lease("t1")),
        )
        assertTrue(coordinator.sessionState.localDirty)

        val saved = DocumentVersion(contentHash = "hash-2", repositoryRevision = 5L)
        coordinator.markSaved("t1", saved)

        val state = coordinator.sessionState
        assertFalse("保存成功后 localDirty 必须清除", state.localDirty)
        assertEquals("committedVersion 必须推进到 savedVersion", saved, state.committedVersion)
        assertEquals("sessionBaseVersion 必须推进到 savedVersion", saved, state.sessionBaseVersion)
        assertEquals("markSaved 后保存版本可被读取", saved, coordinator.documentCommittedVersionFor("t1"))
    }

    @Test
    fun markSaved_emptyVersion_isIgnored() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))
        coordinator.applyExternalContentFact(
            TargetDocumentFact(
                "t1",
                "old",
                DocumentVersion(contentHash = "hash-1"),
                DocumentVersion(),
                DocumentFactOrigin.REPOSITORY_LOAD,
            ),
        )
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("t1", "edited", 3L, 7L, lease = lease("t1")),
        )
        coordinator.markSaved("t1", DocumentVersion())
        assertTrue("空版本保存上报必须被忽略（不推进版本）", coordinator.sessionState.localDirty)
    }

    @Test
    fun applyExternalContentFact_setsSessionBaseToSourceVersion() {
        // #595 五：session 被 reset 到新正文后，新 base 必须是 sourceVersion
        // （旧实现保留 fact.baseVersion — 下一次同步仍以过时祖先判断冲突）。
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))
        val fact =
            TargetDocumentFact(
                "t1",
                "merged",
                DocumentVersion(contentHash = "hash-2", syncCommitId = "commit-2"),
                DocumentVersion(contentHash = "hash-1"),
                DocumentFactOrigin.SYNC_MERGED,
            )
        coordinator.applyExternalContentFact(fact)
        assertEquals("hash-2", coordinator.sessionState.sessionBaseVersion.contentHash)
        assertEquals("hash-2", coordinator.sessionState.committedVersion.contentHash)
        assertEquals("hash-2", coordinator.documentCommittedVersionFor("t1").contentHash)
        assertFalse(coordinator.sessionState.localDirty)
    }

    @Test
    fun committedVersion_isPerTarget() {
        // #595 五：baseVersion 必须按 target 从 store 读取 — 活动状态属于其他
        // target 时 B 的同步事件不得携带 A 的 base。
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))
        coordinator.registerTarget(EditableTextTarget("t2", isPersistent = true))
        coordinator.applyExternalContentFact(
            TargetDocumentFact(
                "t1",
                "v1",
                DocumentVersion(contentHash = "hash-a"),
                DocumentVersion(),
                DocumentFactOrigin.REPOSITORY_LOAD,
            ),
        )
        coordinator.applyExternalContentFact(
            TargetDocumentFact(
                "t2",
                "v2",
                DocumentVersion(contentHash = "hash-b"),
                DocumentVersion(),
                DocumentFactOrigin.REPOSITORY_LOAD,
            ),
        )
        assertEquals("hash-a", coordinator.documentCommittedVersionFor("t1").contentHash)
        assertEquals("hash-b", coordinator.documentCommittedVersionFor("t2").contentHash)
        // 无记录的 target → 空版本。
        assertTrue(coordinator.documentCommittedVersionFor("unknown").isEmpty)
    }

    @Test
    fun documentVersion_usesRealCommitIdNotTimeAnchor() {
        // #595 五：DocumentVersion 携带真实 commit/manifest ID；lastSyncTime
        // 时间锚点不得出现在版本结构里（不表达因果顺序）。
        val v = DocumentVersion(contentHash = "h", syncCommitId = "commit-abc")
        assertEquals("commit-abc", v.syncCommitId)
        assertFalse(v.isEmpty)
        assertTrue(DocumentVersion::class.java.declaredFields.none { it.name == "syncManifestRevision" })
    }

    @Test
    fun repositoryLoadUncomparable_appliesTrustingDisk() {
        // #595 四：Repository 加载（用户主动打开章节）版本不可比较时仍 Apply —
        // 用户主动加载信任磁盘内容（Git 回退/外部修改后用户想看到磁盘）。
        // 旧实现 loadChapter 自行填 parentVersion=previousCommitted 伪造后代；
        // 新实现不填 parent，shouldApplyExternalContent 对 REPOSITORY_LOAD 放行。
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))
        coordinator.applyExternalContentFact(
            TargetDocumentFact(
                "t1",
                "v1",
                DocumentVersion(contentHash = "hash-1"),
                DocumentVersion(),
                DocumentFactOrigin.REPOSITORY_LOAD,
            ),
        )
        val reloaded =
            TargetDocumentFact(
                "t1",
                "v2-disk-changed",
                // 无 parent，不伪称后代
                DocumentVersion(contentHash = "hash-2"),
                DocumentVersion(),
                DocumentFactOrigin.REPOSITORY_LOAD,
            )
        assertEquals(
            "REPOSITORY_LOAD 不可比较时信任磁盘直接 Apply",
            ExternalContentDecision.Apply,
            coordinator.shouldApplyExternalContent(reloaded),
        )
    }

    @Test
    fun syncMergedUncomparable_remainsConflict() {
        // #595 四：同步合并版本不可比较（Git 回退/外部修改/迟到 IO）→ 冲突，
        // 不得盲目覆盖用户输入（与 REPOSITORY_LOAD 区分）。
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))
        coordinator.applyExternalContentFact(
            TargetDocumentFact(
                "t1",
                "v1",
                DocumentVersion(contentHash = "hash-1"),
                DocumentVersion(),
                DocumentFactOrigin.REPOSITORY_LOAD,
            ),
        )
        val merged =
            TargetDocumentFact(
                "t1",
                "v2-sync",
                // 无 parent
                DocumentVersion(contentHash = "hash-2", syncCommitId = "commit-2"),
                DocumentVersion(),
                DocumentFactOrigin.SYNC_MERGED,
            )
        assertEquals(
            "SYNC_MERGED 不可比较时必须冲突，不盲目覆盖",
            ExternalContentDecision.IgnoreUncomparableConflict,
            coordinator.shouldApplyExternalContent(merged),
        )
    }
}
