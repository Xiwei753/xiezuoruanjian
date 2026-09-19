package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.interop.diagnostics.DiagnosticsInterop

/**
 * 从 MirrorPublishExecutor 提取，只负责单项目发布流程。
 *
 * 包括：准备、stage、frozen plan、promote、manifest commit 和 cleanup 阶段。
 * 各阶段委托给 [MirrorPublishStageExecutor] 和 [MirrorPublishPromoteExecutor]。
 */
internal class MirrorPublishProjectExecutor(
    private val stateStore: ReadableMirrorStateStore,
    private val journalWriter: MirrorJournalWriter,
    private val planner: MirrorPublishPlanner,
    private val rollbackExecutor: MirrorRollbackExecutor,
    private val manifestExecutor: MirrorManifestTransactionExecutor,
    private val source: MirrorSnapshotSource,
    private val router: MirrorStorageRouter,
    private val callbacks: MirrorPublishExecutorCallbacks,
    private val workspace: MirrorTransactionWorkspace,
) {
    private val cleanupTransactionExecutor = MirrorCleanupTransactionExecutor(stateStore, workspace)
    private val stageExecutor =
        MirrorPublishStageExecutor(stateStore, planner, journalWriter, source, router, workspace)
    private val promoteExecutor = MirrorPublishPromoteExecutor(journalWriter, rollbackExecutor, workspace)
    private val ensurePendingRecovered get() = callbacks.ensurePendingRecovered
    private val logNotLoaded get() = callbacks.logNotLoaded
    private val logPublishAborted get() = callbacks.logPublishAborted

    internal suspend fun publishProject(projectId: String): MirrorPublishResult {
        try {
            return executePublishProject(projectId)
        } catch (e: Exception) {
            DiagnosticsInterop.e(TAG, "Failed to publish project: ${e.message}", e)
            return MirrorPublishResult.RetryableFailure
        }
    }

    private suspend fun executePublishProject(projectId: String): MirrorPublishResult {
        if (!ensurePendingRecovered()) {
            return MirrorPublishResult.PendingRecovery
        }
        val context =
            stageExecutor.preparePublishContext(projectId, logNotLoaded, logPublishAborted)
                ?: return MirrorPublishResult.RetryableFailure
        if (!stageExecutor.writePublishStageJournal(projectId, context, logPublishAborted)) {
            return MirrorPublishResult.RetryableFailure
        }
        val stageResult =
            stageExecutor.stageAllContent(projectId, context, logPublishAborted)
                ?: return MirrorPublishResult.RetryableFailure
        val frozenContext =
            stageExecutor.buildPublishFrozenPlanAndJournal(projectId, context, stageResult, logPublishAborted)
                ?: return MirrorPublishResult.RetryableFailure
        val promoteResult =
            promoteExecutor.promoteAllStaged(
                projectId = projectId,
                stageResult = stageResult,
                storage = context.storage,
                frozenContext = frozenContext,
                logPublishAborted = logPublishAborted,
            ) ?: return MirrorPublishResult.RetryableFailure
        val manifestResult =
            commitPublishManifest(projectId, context, stageResult, promoteResult, frozenContext)
                ?: return MirrorPublishResult.RetryableFailure
        val cleanupJournal =
            persistPublishCommittedBaseline(projectId, promoteResult, manifestResult)
                ?: return MirrorPublishResult.RetryableFailure
        return finalizePublishCleanup(projectId, context, cleanupJournal)
    }

    private suspend fun commitPublishManifest(
        projectId: String,
        context: MirrorPublishStageExecutor.PublishContext,
        stageResult: MirrorPublishStageExecutor.StageResult,
        promoteResult: MirrorPublishPromoteExecutor.PromoteResult,
        frozenContext: MirrorPublishStageExecutor.FrozenPlanContext,
    ): MirrorManifestTransactionExecutor.ManifestTransactionResult? {
        val manifestTargetJson =
            frozenPlanToManifestJson(frozenContext.frozenPlan, promoteResult.promotedEntries)
                ?: run {
                    rollbackExecutor.rollbackWholePublishTransaction(
                        context.txId,
                        promoteResult.items,
                        stageResult.stagedRefs,
                        context.storage,
                        promoteResult.currentJournal,
                    )
                    return null
                }
        val manifestResult =
            manifestExecutor.publishManifestWithDesiredTransactional(
                MirrorManifestTransactionExecutor.ManifestTransactionParams(
                    projectId = projectId,
                    snapshot = null,
                    desiredEntries = promoteResult.promotedEntries,
                    txId = context.txId,
                    journalContext = promoteResult.currentJournal,
                    items = promoteResult.items,
                    storage = context.storage,
                    prebuiltTargetJson = manifestTargetJson,
                    // Issue #717 评论 5741567193 A 部分：冻结 committed baseline 身份，
                    // 供 resolveManifestOldIdentity 不再 fallback 到公开 _meta/manifest.json。
                    committedManifestHash = frozenContext.committedManifestHash,
                    oldBaselineExists = frozenContext.oldBaselineExists,
                ),
            )
        if (manifestResult == null) {
            logPublishAborted(projectId, "manifest write failed")
            rollbackExecutor.rollbackWholePublishTransaction(
                context.txId,
                promoteResult.items,
                stageResult.stagedRefs,
                context.storage,
                promoteResult.currentJournal,
            )
            return null
        }
        return manifestResult
    }

    private fun persistPublishCommittedBaseline(
        projectId: String,
        promoteResult: MirrorPublishPromoteExecutor.PromoteResult,
        manifestResult: MirrorManifestTransactionExecutor.ManifestTransactionResult,
    ): PendingMirrorPublish? {
        val committedItems = promoteResult.items.mapValues { it.value.copy(state = PendingItem.STATE_COMMITTED) }
        val currentJournal =
            manifestResult.committedJournal.copy(
                phase = PendingMirrorPublish.PHASE_CLEANUP,
                newEntries = promoteResult.promotedEntries,
                stagedRefs = emptyMap(),
                items = committedItems,
            )
        if (!journalWriter.persistPendingJournal(currentJournal)) {
            DiagnosticsInterop.w(
                TAG,
                "Publish project $projectId: cleanup journal write failed, keeping journal for retry",
            )
            return null
        }
        if (!stateStore.putChapterEntries(promoteResult.promotedEntries)) {
            DiagnosticsInterop.w(
                TAG,
                "Publish project $projectId: putChapterEntries failed, keeping journal for retry",
            )
            return null
        }
        if (!journalWriter.persistCommittedBaselineFromJournal(manifestResult.committedJournal)) {
            DiagnosticsInterop.w(
                TAG,
                "Publish project $projectId: persistCommittedBaseline failed, keeping journal for retry",
            )
            return null
        }
        // Issue #717 评论 5741567193 A 部分：正常 publish 收口——
        // 拿到 committed baseline 后先确保私有 manifest 已物化，再把 manifestUri 收口到私有路径。
        // 正常事务只保留三份事实：committed baseline、私有 workspace manifest、pending journal。
        if (!consolidatePrivateManifestFromJournal(projectId, manifestResult.committedJournal, "Publish")) {
            return null
        }
        if (!stateStore.addPublishedProjectId(projectId)) {
            DiagnosticsInterop.w(
                TAG,
                "Publish project $projectId: addPublishedProjectId failed, keeping journal for retry",
            )
            return null
        }
        return currentJournal
    }

    /**
     * Issue #717 评论 5741567193 A 部分：正常 publish/delete 收口。
     *
     * 拿到 committed baseline 后：
     * 1. 调用 [MirrorTransactionWorkspace.ensureCommittedManifest] 确保私有 manifest 已物化；
     * 2. 把 [ReadableMirrorStateStore.setManifestUri] 收口到私有 [MirrorTransactionWorkspace.manifestFile] 路径。
     *
     * 不再让遗留公开 URI（`Download/Sujian/_meta/manifest.json`）参与正常事务。
     */
    private fun consolidatePrivateManifestFromJournal(
        projectId: String,
        committedJournal: PendingMirrorPublish,
        operation: String,
    ): Boolean {
        val json = committedJournal.manifestTargetJson
        val hash = committedJournal.manifestNewContentHash
        if (json == null || hash == null) {
            DiagnosticsInterop.w(
                TAG,
                "$operation project $projectId: committed journal missing manifestTargetJson/newContentHash, " +
                    "cannot consolidate private manifest",
            )
            return false
        }
        val ensureResult = workspace.ensureCommittedManifest(json.toByteArray(Charsets.UTF_8), hash)
        when (ensureResult) {
            is EnsureCommittedManifestResult.Success -> Unit
            is EnsureCommittedManifestResult.HashMismatch -> {
                DiagnosticsInterop.w(
                    TAG,
                    "$operation project $projectId: private manifest hash mismatch " +
                        "(existing=${ensureResult.existingHash}, expected=${ensureResult.expectedHash}), " +
                        "keeping journal for retry",
                )
                return false
            }
            is EnsureCommittedManifestResult.WriteFailed -> {
                DiagnosticsInterop.w(
                    TAG,
                    "$operation project $projectId: private manifest write failed: ${ensureResult.cause?.message}",
                )
                return false
            }
        }
        val manifestPath = workspace.manifestFile().absolutePath
        if (!stateStore.setManifestUri(manifestPath)) {
            DiagnosticsInterop.w(
                TAG,
                "$operation project $projectId: setManifestUri consolidation failed, keeping journal for retry",
            )
            return false
        }
        return true
    }

    private fun finalizePublishCleanup(
        projectId: String,
        context: MirrorPublishStageExecutor.PublishContext,
        cleanupJournal: PendingMirrorPublish,
    ): MirrorPublishResult {
        if (cleanupTransactionExecutor.cleanupCommittedTransaction(
                cleanupJournal,
                context.storage,
                allLiveKeys = context.allKeys,
            )
        ) {
            stateStore.clearPendingPublish()
            return MirrorPublishResult.Committed
        } else {
            DiagnosticsInterop.w(
                TAG,
                "Publish project $projectId: cleanup partial failure, keeping journal for retry",
            )
            return MirrorPublishResult.RetryableFailure
        }
    }

    companion object {
        private const val TAG = "ReadableMirrorPublisher"
    }
}
