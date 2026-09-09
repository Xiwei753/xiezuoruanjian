package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger

/**
 * Pending journal 写入参数。
 *
 * 从 ReadableMirrorPublisher.writePendingPublishJournal 的 24 个参数收口为一个数据对象，
 * 消除 LongParameterList。
 */
data class PendingJournalParams(
    val projectId: String,
    val transactionType: MirrorTransactionType,
    val phase: String,
    val txId: String,
    val backend: MirrorBackend,
    val treeUri: String?,
    val oldEntries: Map<ChapterKey, ChapterMirrorEntry>,
    val newEntries: Map<ChapterKey, ChapterMirrorEntry>,
    val stagedRefs: Map<ChapterKey, StagedMirrorRef>,
    val items: Map<ChapterKey, PendingItem>,
    val removedProjectIds: Set<String>,
    val manifestOldRef: MirrorFileRef? = null,
    val manifestStagedRef: StagedMirrorRef? = null,
    val manifestNewRef: MirrorFileRef? = null,
    val manifestBackupRef: MirrorFileRef? = null,
    val isManifestCommitted: Boolean = false,
    val manifestSwapState: ManifestTransactionState = ManifestTransactionState.MANIFEST_STAGED,
    val manifestNewContentHash: String? = null,
    val manifestOldContentHash: String? = null,
    val manifestTargetJson: String? = null,
    val frozenManifestPlan: String? = null,
    val frozenManifestPlanHash: String? = null,
    val journalContext: PendingMirrorPublish? = null,
)

/**
 * Mirror journal 写入器。
 *
 * 从 ReadableMirrorPublisher 提取的 journal 持久化逻辑。
 * #649 评论 5560971132 重构：拆分 LargeClass。
 */
internal class MirrorJournalWriter(
    private val stateStore: ReadableMirrorStateStore,
) {
    /**
     * 写 pendingPublish journal。
     *
     * #649 评论 5563333323 缺口 2：返回 Boolean，失败时调用方停止本轮镜像操作。
     */
    fun writePendingPublishJournal(params: PendingJournalParams): Boolean {
        val effectiveManifestNewContentHash =
            params.manifestNewContentHash ?: params.journalContext?.manifestNewContentHash
        val effectiveManifestOldContentHash =
            params.manifestOldContentHash ?: params.journalContext?.manifestOldContentHash
        val effectiveManifestTargetJson =
            params.manifestTargetJson ?: params.journalContext?.manifestTargetJson
        val effectiveFrozenManifestPlan =
            params.frozenManifestPlan ?: params.journalContext?.frozenManifestPlan
        val effectiveFrozenManifestPlanHash =
            params.frozenManifestPlanHash ?: params.journalContext?.frozenManifestPlanHash
        val journal =
            PendingMirrorPublish(
                txId = params.txId,
                backend = params.backend,
                treeUri = params.treeUri,
                projectId = params.projectId,
                transactionType = params.transactionType,
                phase = params.phase,
                oldEntries = params.oldEntries,
                newEntries = params.newEntries,
                stagedRefs = params.stagedRefs,
                items = params.items,
                removedProjectIds = params.removedProjectIds,
                manifestOldRef = params.manifestOldRef,
                manifestStagedRef = params.manifestStagedRef,
                manifestNewRef = params.manifestNewRef,
                manifestBackupRef = params.manifestBackupRef,
                isManifestCommitted = params.isManifestCommitted,
                manifestSwapState = params.manifestSwapState,
                manifestNewContentHash = effectiveManifestNewContentHash,
                manifestOldContentHash = effectiveManifestOldContentHash,
                manifestTargetJson = effectiveManifestTargetJson,
                frozenManifestPlan = effectiveFrozenManifestPlan,
                frozenManifestPlanHash = effectiveFrozenManifestPlanHash,
            )
        return stateStore.writePendingPublish(journal.toJson())
    }

    /**
     * 前进式持久化 journal：把当前 journal 状态写入磁盘。
     */
    fun persistPendingJournal(journal: PendingMirrorPublish): Boolean =
        stateStore.writePendingPublish(journal.toJson())

    /**
     * 从 journal 持久化 committed manifest baseline（#649 评论 5576949398 问题 2）。
     */
    fun persistCommittedBaselineFromJournal(journal: PendingMirrorPublish): Boolean {
        val json =
            journal.manifestTargetJson ?: run {
                DiagnosticsLogger.w(TAG, "persistCommittedBaseline: manifestTargetJson is null")
                return false
            }
        val hash =
            journal.manifestNewContentHash ?: run {
                DiagnosticsLogger.w(TAG, "persistCommittedBaseline: manifestNewContentHash is null")
                return false
            }
        if (computeContentHash(json) != hash) {
            DiagnosticsLogger.w(TAG, "persistCommittedBaseline: hash mismatch")
            return false
        }
        try {
            mirrorManifestFromJsonStrict(json)
        } catch (e: Exception) {
            DiagnosticsLogger.w(TAG, "persistCommittedBaseline: strict parse failed: ${e.message}")
            return false
        }
        return stateStore.setCommittedManifest(json, hash)
    }

    companion object {
        private const val TAG = "MirrorJournalWriter"
    }
}
