package com.xiwei.sujian.feature.editor.session

import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.editor.input.TextOffsetUtils
import com.xiwei.sujian.feature.editor.projection.EditResult
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.util.concurrent.atomic.AtomicLong

private const val CORRECTION_SCAN_DEBOUNCE_MS = 250L

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

data class EditorCorrectionState(
    val targetId: String? = null,
    val projectId: String? = null,
    val chapterId: String? = null,
    val sessionId: ULong? = null,
    val textRevision: Long? = null,
    val suggestions: List<EditorCorrectionSuggestion> = emptyList(),
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
    private var pendingScanJob: Job? = null

    private data class PendingScan(
        val identity: EditorCorrectionIdentity,
        val lease: EditorInputLease,
        val generation: Long,
        val textRevision: Long,
        val text: String,
    )

    /**
     * Apply the editor transaction's real display patches; never infer edits from the caret.
     *
     * [text] is the full committed text at [EditResult.newRevision], captured with the
     * transaction. The live kernel may already be at a newer revision when this queued
     * request reaches Core, so the revision must not be re-read here: Core applies the
     * patches against the captured text and advances its own segment index incrementally.
     */
    suspend fun checkEdits(
        identity: EditorCorrectionIdentity,
        edit: EditResult,
        text: String,
        lease: EditorInputLease,
    ) {
        // Use the lease captured by the edit transaction, never a lease sampled
        // when this queued coroutine happens to run. Reopening the chapter changes
        // the epoch (and usually the session id); replaying an old text/range pair
        // under the new session would poison the new chapter's incremental cache.
        if (lease.targetId != identity.targetId || currentIdentity() != identity) return
        if (sessionCoordinator.currentInputLease() != lease) return
        // Validate before advancing the generation. A stale queued request must
        // not invalidate the response of a newer request that is still in flight.
        val generation = requestGeneration.incrementAndGet()
        if (!_state.value.matchesIdentity(identity)) {
            _state.value = EditorCorrectionState(
                targetId = identity.targetId,
                projectId = identity.projectId,
                chapterId = identity.chapterId,
            )
        }
        _state.value = _state.value.copy(sessionId = lease.sessionId)

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
        if (edits.isEmpty()) {
            restoreChapterSnapshot(identity)
            return
        }

        val applied =
            withContext(Dispatchers.IO) {
                appServiceBridge.applyCorrectionEdits(
                    sessionId = lease.sessionId,
                    projectId = identity.projectId,
                    chapterId = identity.chapterId,
                    textRevision = edit.newRevision.toULong(),
                    baseRevision = edit.baseRevision.toULong(),
                    edits = edits,
                )
            }
        if (applied !is BridgeResult.Success) return

        // The model scan is debounced; rapid edits only apply cheap offset/dirty
        // updates and the latest text is scanned once after input settles.
        pendingScanJob?.cancel()
        val pending = PendingScan(identity, lease, generation, edit.newRevision, text)
        pendingScanJob =
            pendingScope.launch {
                delay(CORRECTION_SCAN_DEBOUNCE_MS)
                scanPending(pending)
            }
    }

    private suspend fun scanPending(pending: PendingScan) {
        if (pending.generation != requestGeneration.get() || currentIdentity() != pending.identity) return
        if (sessionCoordinator.currentInputLease() != pending.lease) return
        val response =
            withContext(Dispatchers.IO) {
                appServiceBridge.scanCorrectionPending(
                    sessionId = pending.lease.sessionId,
                    projectId = pending.identity.projectId,
                    chapterId = pending.identity.chapterId,
                    textRevision = pending.textRevision.toULong(),
                    text = pending.text,
                )
            }
        val snapshot = (response as? BridgeResult.Success)?.data ?: return
        val stillCurrent =
            withContext(Dispatchers.IO) {
                sessionCoordinator.currentInputLease() == pending.lease
            }
        if (!stillCurrent || pending.generation != requestGeneration.get() ||
            currentIdentity() != pending.identity
        ) {
            return
        }
        if (sessionCoordinator.sessionState.revision != pending.textRevision) return
        if (snapshot.textRevision.toLong() != pending.textRevision) return

        _state.value = EditorCorrectionState(
            targetId = pending.identity.targetId,
            projectId = pending.identity.projectId,
            chapterId = pending.identity.chapterId,
            sessionId = pending.lease.sessionId,
            textRevision = pending.textRevision,
            suggestions = mappedSuggestions(pending.text, pending.textRevision, snapshot.suggestions),
        )
    }

    suspend fun checkChapter(identity: EditorCorrectionIdentity) {
        pendingScanJob?.cancel()
        val lease = sessionCoordinator.currentInputLease() ?: return
        if (lease.targetId != identity.targetId || currentIdentity() != identity) return
        val generation = requestGeneration.incrementAndGet()
        if (!_state.value.matchesIdentity(identity)) {
            _state.value = EditorCorrectionState(
                targetId = identity.targetId,
                projectId = identity.projectId,
                chapterId = identity.chapterId,
            )
        }
        _state.value = _state.value.copy(sessionId = lease.sessionId)
        val before = withContext(Dispatchers.IO) { sessionCoordinator.queryTargetSnapshot(identity.targetId) } ?: return
        if (!snapshotMatchesLease(before, lease)) return

        val response =
            withContext(Dispatchers.IO) {
                appServiceBridge.checkCorrectionChapter(
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

        _state.value = EditorCorrectionState(
            targetId = identity.targetId,
            projectId = identity.projectId,
            chapterId = identity.chapterId,
            sessionId = lease.sessionId,
            textRevision = before.revision,
            suggestions = mappedSuggestions(before.text, before.revision, snapshot.suggestions),
        )
    }

    /** Restore Core's cached chapter suggestions without scanning; failures leave state untouched. */
    suspend fun restoreChapterSnapshot(identity: EditorCorrectionIdentity) {
        pendingScanJob?.cancel()
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

        _state.value = EditorCorrectionState(
            targetId = identity.targetId,
            projectId = identity.projectId,
            chapterId = identity.chapterId,
            sessionId = lease.sessionId,
            textRevision = before.revision,
            suggestions = mappedSuggestions(before.text, before.revision, snapshot.suggestions),
        )
    }

    suspend fun accept(
        identity: EditorCorrectionIdentity,
        suggestion: EditorCorrectionSuggestion,
    ): EditorCorrectionAcceptResult {
        requestGeneration.incrementAndGet()
        pendingScanJob?.cancel()
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

        _state.value = EditorCorrectionState(
            targetId = identity.targetId,
            projectId = identity.projectId,
            chapterId = identity.chapterId,
            sessionId = lease.sessionId,
            textRevision = after.revision,
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
        return added
    }

    fun clear(targetId: String? = null) {
        requestGeneration.incrementAndGet()
        pendingScanJob?.cancel()
        if (targetId == null || _state.value.targetId == targetId) _state.value = EditorCorrectionState()
    }

    private fun snapshotMatchesLease(
        snapshot: com.xiwei.sujian.feature.editor.session.TargetSnapshot,
        lease: com.xiwei.sujian.feature.editor.session.EditorInputLease,
    ): Boolean =
        sessionCoordinator.sessionState.activeTargetId == lease.targetId &&
            sessionCoordinator.sessionState.sessionId == lease.sessionId &&
            sessionCoordinator.sessionState.revision == snapshot.revision

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
