package com.xiwei.sujian.feature.editor.session

import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.editor.input.TextOffsetUtils
import com.xiwei.sujian.feature.editor.projection.EditResult
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong

/**
 * Issue #1064：前台活动章节的增量校对周期。开章后首轮在本周期结束时触发；
 * 连续输入不重置周期，退后台/切章停止定时器，脏区保留在 Core。
 */
private const val CORRECTION_SCAN_PERIOD_MS = 60_000L

/** Stable document identity captured for each correction request and accept action. */
data class EditorCorrectionIdentity(
    val projectId: String,
    val volumeId: String,
    val chapterId: String,
    val targetId: String,
)

/** Platform view of Core's UTF-8 byte-based correction suggestion. */
data class EditorCorrectionSuggestion(
    val startByte: Int,
    val endByte: Int,
    val original: String,
    val replacement: String,
    val source: String,
    val ruleId: String,
    val severity: String,
    val textRevision: Long,
)

/**
 * Issue #1064：谁提交了当前/最近一次增量检查 — 手动校对立即提交，周期调度
 * 每 60s 在有待检查脏区时提交一次；两者共用同一单飞入口。
 */
enum class EditorCorrectionScanTrigger {
    MANUAL,
    PERIODIC,
}

data class EditorCorrectionState(
    val targetId: String? = null,
    val projectId: String? = null,
    val chapterId: String? = null,
    val sessionId: ULong? = null,
    val textRevision: Long? = null,
    val suggestions: List<EditorCorrectionSuggestion> = emptyList(),
    /** Core 已验证的 UTF-8 字节数（checked_ranges 覆盖部分）；未覆盖字节仍需检查。 */
    val checkedByteCount: Int = 0,
    /** 检查时正文的 UTF-8 字节长度；0 表示还没有可用的检查快照。 */
    val totalByteCount: Int = 0,
    /** Core 报告的待检查区间（脏段或从未验证的段）是否仍存在。 */
    val hasUncheckedSegments: Boolean = false,
    /** 编辑事务已标脏、但周期/手动检查尚未提交。 */
    val hasPendingScanWork: Boolean = false,
    /** 当前是否有增量检查在跑（同一时间最多一个）。 */
    val scanInProgress: Boolean = false,
    val lastScanCompletedAtMillis: Long? = null,
    val lastScanTrigger: EditorCorrectionScanTrigger? = null,
)

enum class EditorCorrectionIgnoreScope {
    ONCE,
    CHAPTER,
    PERMANENT,
    RULE,
}

sealed interface EditorCorrectionAcceptResult {
    data class Applied(
        val identity: EditorCorrectionIdentity,
        val before: com.xiwei.sujian.feature.editor.session.TargetSnapshot,
        val after: com.xiwei.sujian.feature.editor.session.TargetSnapshot,
        val edit: EditResult,
    ) : EditorCorrectionAcceptResult

    data object Stale : EditorCorrectionAcceptResult

    data object Rejected : EditorCorrectionAcceptResult
}

/**
 * Coordinates editor-scoped correction calls. It captures the active input lease and Core
 * revision before each request, then checks the same identity/revision again before publishing
 * suggestions or accepting an edit. UTF-8 offsets remain bytes until the UI presentation edge.
 */
class EditorCorrectionCoordinator(
    private val appServiceBridge: AppServiceBridge,
    private val sessionCoordinator: EditorSessionCoordinator,
    private val currentIdentity: () -> EditorCorrectionIdentity?,
) {
    private val requestGeneration = AtomicLong(0L)
    private val _state = MutableStateFlow(EditorCorrectionState())
    val state: StateFlow<EditorCorrectionState> = _state.asStateFlow()
    private val pendingScope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    /** Invalidated on session close so queued edit mappings cannot target a reopened chapter. */
    private val applyEpoch = AtomicLong(0L)
    /**
     * Stage-one edit mappings must reach Core in transaction order. A single
     * consumer applies them sequentially. No model scan is scheduled here any
     * more (Issue #1064): the ordered apply only records Core's dirty windows.
     */
    private val applyChannel = Channel<PendingApply>(Channel.UNLIMITED)
    private val applyScope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    /**
     * Issue #1064 单飞扫描入口的待办标志：编辑事务/资源变化置位，周期或手动
     * 提交一次增量检查，Core 返回仍有无待检查段时保持置位。
     */
    private val scanWorkPending = AtomicBoolean(false)
    private val scanGate = Any()
    /** 唤醒扫描泵的合并信号 — 只在有待办时发送，重复触发合并成一个。 */
    private val scanSignals = Channel<Unit>(Channel.CONFLATED)
    /** 下一次增量检查的原因；扫描中只更新该标志，结束后补跑一轮。 */
    private var pendingScanTrigger: EditorCorrectionScanTrigger? = null
    private val scanWaiters = mutableListOf<CompletableDeferred<Unit>>()
    /** 当前在跑的增量检查 Job — restore/采纳建议/清空时取消，不与新检查并行。 */
    @Volatile
    private var inFlightScanJob: Job? = null
    private var periodicScanJob: Job? = null
    private var periodicScanTargetId: String? = null
    /** Set once the owning ViewModel is destroyed; no later work may publish or enqueue. */
    private val disposed = AtomicBoolean(false)

    private data class PendingApply(
        val identity: EditorCorrectionIdentity,
        val lease: EditorInputLease,
        val applyEpoch: Long,
        val textRevision: Long,
        val baseRevision: Long,
        val edits: List<uniffi.writer_core.CorrectionEditRangeDto>,
    )

    init {
        applyScope.launch {
            for (request in applyChannel) {
                applyEdits(request)
            }
        }
        // 单飞扫描泵：所有周期/手动请求都经同一入口串行执行，不并行跑模型。
        pendingScope.launch {
            for (signal in scanSignals) {
                drainScanTriggers()
            }
        }
    }

    /**
     * Queue the editor transaction's real display patches; never infer edits from the caret.
     *
     * The stage-one queue carries only revision/ranges/lease. The live kernel
     * may already be at a newer revision when this request reaches Core, so the
     * revision must not be re-read here: Core applies the patches against its
     * committed segment index. Issue #1064: this only records dirty windows —
     * the model runs on the periodic/manual incremental check, never per edit.
     */
    fun enqueueEdits(
        identity: EditorCorrectionIdentity,
        edit: EditResult,
        lease: EditorInputLease,
    ) {
        if (disposed.get()) return
        // Use the lease captured by the edit transaction, never a lease sampled
        // when this queued coroutine happens to run. Reopening the chapter changes
        // the epoch (and usually the session id); replaying an old text/range pair
        // under the new session would poison the new chapter's incremental cache.
        if (lease.targetId != identity.targetId || currentIdentity() != identity) return
        if (sessionCoordinator.currentInputLease() != lease) return
        val edits =
            edit.displayPatches
                .filter { it.newRevision == edit.newRevision }
                .map {
                    uniffi.writer_core.CorrectionEditRangeDto(
                        it.replaceByteStart.toUInt(),
                        it.replaceByteEndExclusive.toUInt(),
                        it.insertedByteStart.toUInt(),
                        it.insertedByteEndExclusive.toUInt(),
                    )
                }
        requestGeneration.incrementAndGet()
        // Issue #1064：编辑事务只保留轻量脏区标记，不再触发 250ms 停抖扫描；
        // 周期检查或手动校对会在下一次轮到时提交增量检查。
        if (edits.isNotEmpty()) scanWorkPending.set(true)
        if (!_state.value.matchesIdentity(identity)) {
            _state.value = EditorCorrectionState(
                targetId = identity.targetId,
                projectId = identity.projectId,
                chapterId = identity.chapterId,
            )
        }
        _state.value = _state.value.copy(
            sessionId = lease.sessionId,
            hasPendingScanWork = scanWorkPending.get(),
        )
        applyChannel.trySend(
            PendingApply(
                identity = identity,
                lease = lease,
                applyEpoch = applyEpoch.get(),
                textRevision = edit.newRevision,
                baseRevision = edit.baseRevision,
                edits = edits,
            ),
        )
    }

    private suspend fun applyEdits(request: PendingApply) {
        if (request.applyEpoch != applyEpoch.get()) return
        if (request.lease.targetId != request.identity.targetId ||
            currentIdentity() != request.identity ||
            sessionCoordinator.currentInputLease() != request.lease
        ) {
            return
        }
        if (request.edits.isNotEmpty()) {
            val applied =
                withContext(Dispatchers.IO) {
                    appServiceBridge.applyCorrectionEdits(
                        sessionId = request.lease.sessionId,
                        projectId = request.identity.projectId,
                        chapterId = request.identity.chapterId,
                        textRevision = request.textRevision.toULong(),
                        baseRevision = request.baseRevision.toULong(),
                        edits = request.edits,
                    )
                }
            if (applied !is BridgeResult.Success) {
                // Dirty windows stay with Core; the next periodic/manual round
                // reconciles whatever this queued mapping could not record.
                scanWorkPending.set(true)
                syncPendingScanFlag()
                return
            }
        }
        // No scan is scheduled here: Issue #1064 runs the model on the chapter's
        // 60s periodic round or on an explicit manual check.
    }

    /**
     * Issue #1064 单飞入口：周期/手动增量检查都从这里提交。扫描进行中只记录
     * 待办（[pendingScanTrigger]），当前一轮结束后由 [drainScanTriggers] 补跑
     * 一轮；任何时刻最多一个模型扫描。
     */
    private fun submitScan(
        trigger: EditorCorrectionScanTrigger,
        waiter: CompletableDeferred<Unit>? = null,
    ) {
        if (disposed.get()) {
            waiter?.complete(Unit)
            return
        }
        var notify = false
        synchronized(scanGate) {
            if (trigger == EditorCorrectionScanTrigger.PERIODIC && !scanWorkPending.get() &&
                _state.value.totalByteCount > 0
            ) {
                waiter?.complete(Unit)
                return
            }
            val merged =
                when {
                    trigger == EditorCorrectionScanTrigger.MANUAL -> EditorCorrectionScanTrigger.MANUAL
                    else -> pendingScanTrigger ?: EditorCorrectionScanTrigger.PERIODIC
                }
            pendingScanTrigger = merged
            waiter?.let { scanWaiters += it }
            notify = true
        }
        if (notify) scanSignals.trySend(Unit)
    }

    /** 按提交顺序消费待办；扫描期间到达的请求合并成一轮补扫。 */
    private suspend fun drainScanTriggers() {
        while (currentCoroutineContext().isActive) {
            val request =
                synchronized(scanGate) {
                    val next = pendingScanTrigger ?: return
                    pendingScanTrigger = null
                    val waiters = scanWaiters.toList()
                    scanWaiters.clear()
                    next to waiters
                }
            val scanJob =
                pendingScope.launch {
                    try {
                        performIncrementalScan(request.first)
                    } catch (cancelled: kotlinx.coroutines.CancellationException) {
                        throw cancelled
                    } catch (_: Exception) {
                        // Transport/bridge failures keep the dirty windows for
                        // the next periodic round; never retry in a tight loop.
                        scanWorkPending.set(true)
                    }
                }
            inFlightScanJob = scanJob
            try {
                scanJob.join()
            } finally {
                if (inFlightScanJob === scanJob) inFlightScanJob = null
                request.second.forEach { it.complete(Unit) }
            }
        }
    }

    /** 取消正在跑的检查与待办；restore/采纳/清空必须抢占单飞入口。 */
    private fun cancelInFlightScan() {
        val waiters: List<CompletableDeferred<Unit>>
        synchronized(scanGate) {
            pendingScanTrigger = null
            waiters = scanWaiters.toList()
            scanWaiters.clear()
        }
        inFlightScanJob?.cancel()
        inFlightScanJob = null
        waiters.forEach { it.complete(Unit) }
    }

    /**
     * Issue #1064：一次增量检查读取当前已结算正文的 revision，交给 Core 只补
     * 脏段与未验证段；不与编辑事务契约耦合，也不修改正文。
     */
    private suspend fun performIncrementalScan(trigger: EditorCorrectionScanTrigger) {
        val identity = currentIdentity() ?: return
        val lease = sessionCoordinator.currentInputLease() ?: return
        if (lease.targetId != identity.targetId) return
        val generation = requestGeneration.incrementAndGet()
        val before =
            withContext(Dispatchers.IO) { sessionCoordinator.queryTargetSnapshot(identity.targetId) }
                ?: return
        if (!snapshotMatchesLease(before, lease)) return
        markScanInProgress(identity, lease)
        try {
            val response =
                withContext(Dispatchers.IO) {
                    // Issue #1064：周期/手动都走 Core 的增量入口，只补脏段与
                    // 未验证段；不调用整章 All 扫描。
                    appServiceBridge.scanCorrectionPending(
                        sessionId = lease.sessionId,
                        projectId = identity.projectId,
                        chapterId = identity.chapterId,
                        textRevision = before.revision.toULong(),
                        text = before.text,
                    )
                }
            val snapshot = (response as? BridgeResult.Success)?.data ?: return
            val stillCurrent =
                withContext(Dispatchers.IO) {
                    sessionCoordinator.currentInputLease() == lease &&
                        sessionCoordinator.queryTargetSnapshot(identity.targetId)?.revision == before.revision
                }
            if (!stillCurrent || generation != requestGeneration.get() || currentIdentity() != identity) {
                return
            }
            if (sessionCoordinator.sessionState.revision != before.revision) return
            if (snapshot.textRevision.toLong() != before.revision) return
            scanWorkPending.set(snapshot.hasUncheckedSegments)
            publishSnapshot(identity, lease, before.text, before.revision, snapshot, trigger)
        } finally {
            clearScanInProgress(identity)
        }
    }

    /**
     * Issue #1064：前台活动章节的 60s 周期调度。开章后首轮在周期结束时触发；
     * 连续输入不重置周期。切章/退后台由 [stopPeriodicScan] 取消，未完成脏区
     * 留在 Core，回前台重新开始一轮新的 60s。
     */
    fun startPeriodicScan(identity: EditorCorrectionIdentity) {
        if (disposed.get()) return
        synchronized(scanGate) {
            if (periodicScanJob?.isActive == true && periodicScanTargetId == identity.targetId) return
            periodicScanJob?.cancel()
            periodicScanTargetId = identity.targetId
            periodicScanJob =
                pendingScope.launch {
                    while (isActive) {
                        delay(CORRECTION_SCAN_PERIOD_MS)
                        val lease = sessionCoordinator.currentInputLease() ?: continue
                        if (lease.targetId != identity.targetId) return@launch
                        if (currentIdentity() != identity) continue
                        submitScan(EditorCorrectionScanTrigger.PERIODIC)
                    }
                }
        }
    }

    /** 后台/切章/页面销毁停止本章定时器；脏区与缓存都保留。 */
    fun stopPeriodicScan(targetId: String? = null) {
        synchronized(scanGate) {
            if (targetId != null && periodicScanTargetId != null && periodicScanTargetId != targetId) return
            periodicScanJob?.cancel()
            periodicScanJob = null
            periodicScanTargetId = null
        }
    }

    /** 词典/模型/规则变化后重新标脏，等下一次周期或手动检查。 */
    fun markScanPending() {
        if (disposed.get()) return
        scanWorkPending.set(true)
        syncPendingScanFlag()
    }

    /**
     * Issue #1064：手动校对入口 — 立即把当前已结算正文交给 Core 做一次增量
     * 检查（只补脏段与未验证段）。扫描进行中时合并为一次补跑，不并行跑模型；
     * 不自动修改正文。
     */
    suspend fun checkChapter(identity: EditorCorrectionIdentity) {
        if (disposed.get()) return
        val lease = sessionCoordinator.currentInputLease() ?: return
        if (lease.targetId != identity.targetId || currentIdentity() != identity) return
        val waiter = CompletableDeferred<Unit>()
        submitScan(EditorCorrectionScanTrigger.MANUAL, waiter)
        waiter.await()
    }

    /**
     * Issue #1064：开章/回前台/同步回流只恢复 Core 的缓存快照，绝不在这里
     * 触发扫描（旧的 restore 取消后立即补扫路径已删除）。恢复结果只更新展示
     * 与进度；仍有未检查段时留给 60s 周期或手动校对。
     */
    suspend fun restoreChapterSnapshot(identity: EditorCorrectionIdentity) {
        cancelInFlightScan()
        val lease = sessionCoordinator.currentInputLease() ?: return
        if (lease.targetId != identity.targetId || currentIdentity() != identity) return
        val generation = requestGeneration.incrementAndGet()
        val before = withContext(Dispatchers.IO) { sessionCoordinator.queryTargetSnapshot(identity.targetId) } ?: return
        if (!snapshotMatchesLease(before, lease)) return

        val response =
            withContext(Dispatchers.IO) {
                appServiceBridge.getCorrectionChapterSnapshot(
                    sessionId = lease.sessionId,
                    projectId = identity.projectId,
                    chapterId = identity.chapterId,
                    textRevision = before.revision.toULong(),
                )
            }
        val snapshot = (response as? BridgeResult.Success)?.data ?: return

        val stillCurrent =
            withContext(Dispatchers.IO) {
                sessionCoordinator.currentInputLease() == lease &&
                    sessionCoordinator.queryTargetSnapshot(identity.targetId)?.revision == before.revision
            }
        if (!stillCurrent || generation != requestGeneration.get() || currentIdentity() != identity) return
        if (snapshot.textRevision.toLong() != before.revision) return

        scanWorkPending.set(snapshot.hasUncheckedSegments)
        publishSnapshot(
            identity = identity,
            lease = lease,
            text = before.text,
            revision = before.revision,
            snapshot = snapshot,
            trigger = null,
        )
    }

    suspend fun accept(
        identity: EditorCorrectionIdentity,
        suggestion: EditorCorrectionSuggestion,
    ): EditorCorrectionAcceptResult {
        requestGeneration.incrementAndGet()
        cancelInFlightScan()
        val lease = sessionCoordinator.currentInputLease() ?: return EditorCorrectionAcceptResult.Stale
        if (lease.targetId != identity.targetId || currentIdentity() != identity) {
            return EditorCorrectionAcceptResult.Stale
        }
        val before = withContext(Dispatchers.IO) { sessionCoordinator.queryTargetSnapshot(identity.targetId) }
            ?: return EditorCorrectionAcceptResult.Stale
        if (!snapshotMatchesLease(before, lease) || before.revision != suggestion.textRevision ||
            !validByteRange(before.text, suggestion.startByte, suggestion.endByte) ||
            rangeText(before.text, suggestion.startByte, suggestion.endByte) != suggestion.original
        ) {
            return EditorCorrectionAcceptResult.Stale
        }

        val response =
            withContext(Dispatchers.IO) {
                appServiceBridge.applyCorrection(
                    sessionId = lease.sessionId,
                    projectId = identity.projectId,
                    chapterId = identity.chapterId,
                    textRevision = suggestion.textRevision.toULong(),
                    startByte = suggestion.startByte.toUInt(),
                    endByte = suggestion.endByte.toUInt(),
                    original = suggestion.original,
                    replacement = suggestion.replacement,
                )
            }
        val dto = (response as? BridgeResult.Success)?.data ?: return EditorCorrectionAcceptResult.Rejected
        val edit = EditResult.fromDto(dto)
        if (!edit.isApplied()) return EditorCorrectionAcceptResult.Stale

        if (sessionCoordinator.currentInputLease() != lease || currentIdentity() != identity) {
            return EditorCorrectionAcceptResult.Stale
        }
        sessionCoordinator.applyProgrammaticReplace(
            EditorDocumentUpdate.ProgrammaticReplace(
                targetId = identity.targetId,
                commandId = edit.transactionId,
                revision = edit.newRevision,
                transactionId = edit.transactionId,
                selectionAnchorUtf8 = edit.newSelectionAnchor,
                selectionHeadUtf8 = edit.newSelectionHead,
                lease = lease,
                contentChanged = edit.displayPatches.isNotEmpty(),
                contentDelta = dto.contentDelta.toSessionDelta(),
            ),
        )
        val after = withContext(Dispatchers.IO) { sessionCoordinator.queryTargetSnapshot(identity.targetId) }
            ?: return EditorCorrectionAcceptResult.Rejected
        if (after.revision != edit.newRevision ||
            sessionCoordinator.currentInputLease() != lease ||
            currentIdentity() != identity
        ) {
            return EditorCorrectionAcceptResult.Stale
        }

        // The accepted replacement changed the body: cached coverage from the
        // previous revision no longer describes this text, so it is pending
        // again for the next periodic round or a manual check.
        scanWorkPending.set(true)
        val previous = baseStateFor(identity)
        _state.value = EditorCorrectionState(
            targetId = identity.targetId,
            projectId = identity.projectId,
            chapterId = identity.chapterId,
            sessionId = lease.sessionId,
            textRevision = after.revision,
            totalByteCount = after.text.toByteArray(Charsets.UTF_8).size,
            hasUncheckedSegments = true,
            hasPendingScanWork = true,
            lastScanCompletedAtMillis = previous.lastScanCompletedAtMillis,
            lastScanTrigger = previous.lastScanTrigger,
        )
        return EditorCorrectionAcceptResult.Applied(identity, before, after, edit)
    }

    suspend fun ignore(
        identity: EditorCorrectionIdentity,
        suggestion: EditorCorrectionSuggestion,
        scope: EditorCorrectionIgnoreScope,
    ): Boolean {
        requestGeneration.incrementAndGet()
        val lease = sessionCoordinator.currentInputLease() ?: return false
        if (lease.targetId != identity.targetId || currentIdentity() != identity) return false
        val snapshot =
            withContext(Dispatchers.IO) { sessionCoordinator.queryTargetSnapshot(identity.targetId) }
                ?: return false
        if (!snapshotMatchesLease(snapshot, lease) || snapshot.revision != suggestion.textRevision ||
            !validByteRange(snapshot.text, suggestion.startByte, suggestion.endByte) ||
            rangeText(snapshot.text, suggestion.startByte, suggestion.endByte) != suggestion.original
        ) {
            return false
        }
        val response =
            withContext(Dispatchers.IO) {
                appServiceBridge.ignoreCorrection(
                    identity.projectId,
                    identity.chapterId,
                    suggestion.ruleId,
                    suggestion.original,
                    scope.toDto(),
                    suggestion.textRevision.toULong(),
                    suggestion.startByte.toUInt(),
                    suggestion.endByte.toUInt(),
                )
            }
        val ignored = (response as? BridgeResult.Success)?.data == true
        if (ignored && sessionCoordinator.currentInputLease() == lease && currentIdentity() == identity) {
            val remaining =
                if (scope == EditorCorrectionIgnoreScope.RULE) {
                    _state.value.suggestions.filterNot { it.ruleId == suggestion.ruleId }
                } else {
                    _state.value.suggestions - suggestion
                }
            _state.value = _state.value.copy(suggestions = remaining)
        }
        return ignored
    }

    suspend fun addDictionaryEntry(identity: EditorCorrectionIdentity, entry: String): Boolean {
        if (currentIdentity() != identity || entry.isBlank()) return false
        requestGeneration.incrementAndGet()
        val added = withContext(Dispatchers.IO) {
            (appServiceBridge.addCorrectionDictionaryEntry(identity.projectId, entry) as? BridgeResult.Success)
                ?.data == true
        }
        if (added && currentIdentity() == identity && _state.value.projectId == identity.projectId) {
            _state.value = _state.value.copy(
                suggestions = _state.value.suggestions.filterNot { it.original == entry },
            )
        }
        if (added) markScanPending()
        return added
    }

    fun clear(targetId: String? = null) {
        if (disposed.get()) return
        if (targetId != null && _state.value.targetId != targetId) return
        requestGeneration.incrementAndGet()
        stopPeriodicScan(targetId)
        cancelInFlightScan()
        applyEpoch.incrementAndGet()
        scanWorkPending.set(false)
        _state.value = EditorCorrectionState()
    }

    /**
     * Final teardown when the owning ViewModel is destroyed. Unlike [clear],
     * this closes the ordered apply channel and cancels both standalone
     * scopes, so no coroutine or session reference survives the editor.
     */
    fun dispose() {
        if (!disposed.compareAndSet(false, true)) return
        applyEpoch.incrementAndGet()
        requestGeneration.incrementAndGet()
        stopPeriodicScan()
        cancelInFlightScan()
        scanWorkPending.set(false)
        applyChannel.close()
        applyScope.cancel()
        pendingScope.cancel()
        _state.value = EditorCorrectionState()
    }

    private fun snapshotMatchesLease(
        snapshot: com.xiwei.sujian.feature.editor.session.TargetSnapshot,
        lease: com.xiwei.sujian.feature.editor.session.EditorInputLease,
    ): Boolean =
        sessionCoordinator.sessionState.activeTargetId == lease.targetId &&
            sessionCoordinator.sessionState.sessionId == lease.sessionId &&
            sessionCoordinator.sessionState.revision == snapshot.revision

    /** 同一章节上一份状态（保留“最近检查”展示），身份不符时从空状态开始。 */
    private fun baseStateFor(identity: EditorCorrectionIdentity): EditorCorrectionState =
        _state.value.takeIf { it.matchesIdentity(identity) } ?: EditorCorrectionState()

    private fun markScanInProgress(
        identity: EditorCorrectionIdentity,
        lease: EditorInputLease,
    ) {
        val base = baseStateFor(identity)
        _state.value = base.copy(
            targetId = identity.targetId,
            projectId = identity.projectId,
            chapterId = identity.chapterId,
            sessionId = lease.sessionId,
            scanInProgress = true,
        )
    }

    private fun clearScanInProgress(identity: EditorCorrectionIdentity) {
        val current = _state.value
        if (current.matchesIdentity(identity) && current.scanInProgress) {
            _state.value = current.copy(scanInProgress = false)
        }
    }

    private fun syncPendingScanFlag() {
        if (disposed.get()) return
        val current = _state.value
        if (current.hasPendingScanWork != scanWorkPending.get()) {
            _state.value = current.copy(hasPendingScanWork = scanWorkPending.get())
        }
    }

    /**
     * Issue #1064：把一次 Core 快照发布成面板状态。restore（[trigger] 为 null）
     * 只更新缓存结果与进度，不算一次“检查完成”；扫描会同时记录最近检查时间
     * 与触发来源。
     */
    private fun publishSnapshot(
        identity: EditorCorrectionIdentity,
        lease: EditorInputLease,
        text: String,
        revision: Long,
        snapshot: uniffi.writer_core.CorrectionChapterSnapshotDto,
        trigger: EditorCorrectionScanTrigger?,
    ) {
        val totalBytes = text.toByteArray(Charsets.UTF_8).size
        val checkedBytes =
            snapshot.checkedRanges
                .sumOf { range ->
                    val start = range.startByte.toLong().coerceIn(0L, totalBytes.toLong())
                    val end = range.endByte.toLong().coerceIn(0L, totalBytes.toLong())
                    (end - start).coerceAtLeast(0L)
                }.coerceAtMost(totalBytes.toLong())
                .toInt()
        val previous = baseStateFor(identity)
        _state.value = EditorCorrectionState(
            targetId = identity.targetId,
            projectId = identity.projectId,
            chapterId = identity.chapterId,
            sessionId = lease.sessionId,
            textRevision = revision,
            suggestions = mappedSuggestions(text, revision, snapshot.suggestions),
            checkedByteCount = checkedBytes,
            totalByteCount = totalBytes,
            hasUncheckedSegments = snapshot.hasUncheckedSegments,
            hasPendingScanWork = scanWorkPending.get(),
            scanInProgress = false,
            lastScanCompletedAtMillis =
                if (trigger != null) System.currentTimeMillis() else previous.lastScanCompletedAtMillis,
            lastScanTrigger = trigger ?: previous.lastScanTrigger,
        )
    }

    private fun validByteRange(text: String, startByte: Int, endByte: Int): Boolean {
        val bytes = text.toByteArray(Charsets.UTF_8).size
        if (startByte < 0 || endByte <= startByte || endByte > bytes) return false
        val startUtf16 = TextOffsetUtils.utf16OffsetForUtf8Byte(text, startByte)
        val endUtf16 = TextOffsetUtils.utf16OffsetForUtf8Byte(text, endByte)
        return TextOffsetUtils.utf8OffsetForCharIndex(text, startUtf16) == startByte &&
            TextOffsetUtils.utf8OffsetForCharIndex(text, endUtf16) == endByte
    }

    private fun rangeText(text: String, startByte: Int, endByte: Int): String? {
        if (!validByteRange(text, startByte, endByte)) return null
        val start = TextOffsetUtils.utf16OffsetForUtf8Byte(text, startByte)
        val end = TextOffsetUtils.utf16OffsetForUtf8Byte(text, endByte)
        return text.substring(start, end)
    }

    private fun mappedSuggestions(
        text: String,
        revision: Long,
        suggestions: List<uniffi.writer_core.CorrectionSuggestionDto>,
    ): List<EditorCorrectionSuggestion> =
        suggestions.mapNotNull { suggestion ->
            val start = suggestion.startByte.toInt()
            val end = suggestion.endByte.toInt()
            if (suggestion.textRevision.toLong() != revision ||
                !validByteRange(text, start, end) ||
                rangeText(text, start, end) != suggestion.original
            ) {
                null
            } else {
                EditorCorrectionSuggestion(
                    startByte = start,
                    endByte = end,
                    original = suggestion.original,
                    replacement = suggestion.replacement,
                    source = suggestion.source,
                    ruleId = suggestion.ruleId,
                    severity = suggestion.severity,
                    textRevision = suggestion.textRevision.toLong(),
                )
            }
        }

    private fun EditorCorrectionState.matchesIdentity(identity: EditorCorrectionIdentity): Boolean =
        targetId == identity.targetId && projectId == identity.projectId && chapterId == identity.chapterId

    private fun EditorCorrectionIgnoreScope.toDto(): uniffi.writer_core.CorrectionIgnoreScopeDto =
        when (this) {
            EditorCorrectionIgnoreScope.ONCE -> uniffi.writer_core.CorrectionIgnoreScopeDto.ONCE
            EditorCorrectionIgnoreScope.CHAPTER -> uniffi.writer_core.CorrectionIgnoreScopeDto.CHAPTER
            EditorCorrectionIgnoreScope.PERMANENT -> uniffi.writer_core.CorrectionIgnoreScopeDto.PERMANENT
            EditorCorrectionIgnoreScope.RULE -> uniffi.writer_core.CorrectionIgnoreScopeDto.RULE
        }
}
