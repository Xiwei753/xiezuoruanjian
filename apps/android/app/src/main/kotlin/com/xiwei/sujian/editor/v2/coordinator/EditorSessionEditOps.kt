package com.xiwei.sujian.editor.v2.coordinator

//! # 编辑器会话编辑操作（从 EditorSessionCoordinator 拆分）

import android.util.Log

// #597 本地编辑应用需校验 lease 后原子更新 session/document/revision 三态，拆分会破坏一致性 — 待后续重构
@Suppress("CyclomaticComplexMethod", "CognitiveComplexMethod")
fun EditorSessionCoordinator.applyLocalEdit(update: EditorDocumentUpdate.LocalInput) {
    if (!isInputLeaseCurrent(update.lease, update.targetId)) {
        Log.w(EditorSessionCoordinator.TAG, "applyLocalEdit(${update.targetId}): stale input lease rejected")
        return
    }
    var pendingRecord: EditorSessionRecord? = null
    updateSessionState { previous ->
        val existing = store.record(update.targetId)
        val previousDoc = existing?.documentState ?: DocumentState()
        val sessionId = existing?.sessionId
            ?: previous.sessionId?.takeIf { previous.targetId == update.targetId }
            ?: 0UL
        val contentChanged = update.text != previousDoc.text
        val dirty = if (!contentChanged && update.operationKind == EditorOperationKind.SELECTION) {
            previousDoc.localDirty
        } else {
            contentChanged
        }
        pendingRecord = existing?.copy(
            documentState = previousDoc.copy(
                text = update.text,
                revision = update.revision,
                selectionAnchorUtf8 = if (update.selectionAnchorUtf8 >= 0) update.selectionAnchorUtf8 else previousDoc.selectionAnchorUtf8,
                selectionHeadUtf8 = if (update.selectionHeadUtf8 >= 0) update.selectionHeadUtf8 else previousDoc.selectionHeadUtf8,
                lastAppliedTransactionId = update.transactionId,
                localDirty = dirty,
            ),
        ) ?: EditorSessionRecord(
            targetId = update.targetId,
            documentState = DocumentState(
                text = update.text,
                revision = update.revision,
                selectionAnchorUtf8 = if (update.selectionAnchorUtf8 >= 0) update.selectionAnchorUtf8 else 0,
                selectionHeadUtf8 = if (update.selectionHeadUtf8 >= 0) update.selectionHeadUtf8 else 0,
                lastAppliedTransactionId = update.transactionId,
                localDirty = true,
            ),
        )
        EditorSessionState(
            targetId = update.targetId,
            sessionId = sessionId,
            text = update.text,
            revision = update.revision,
            selectionAnchorUtf8 = if (update.selectionAnchorUtf8 >= 0) update.selectionAnchorUtf8 else previousDoc.selectionAnchorUtf8,
            selectionHeadUtf8 = if (update.selectionHeadUtf8 >= 0) update.selectionHeadUtf8 else previousDoc.selectionHeadUtf8,
            lastAppliedTransactionId = update.transactionId,
            origin = EditorSessionOrigin.LOCAL_INPUT,
            bindingState = previous.bindingState,
            editingState = previous.editingState,
            activeTargetId = previous.activeTargetId,
            committedVersion = previousDoc.committedVersion,
            sessionBaseVersion = previousDoc.sessionBaseVersion,
            localDirty = dirty,
        )
    }
    pendingRecord?.let { store.put(it) }
}

fun EditorSessionCoordinator.applyUndoRestored(update: EditorDocumentUpdate.UndoRestored) {
    if (!isInputLeaseCurrent(update.lease, update.targetId)) {
        Log.w(EditorSessionCoordinator.TAG, "applyUndoRestored(${update.targetId}): stale input lease rejected")
        return
    }
    var pendingRecord: EditorSessionRecord? = null
    updateSessionState { previous ->
        val existing = store.record(update.targetId)
        val previousDoc = existing?.documentState ?: DocumentState()
        val sessionId = existing?.sessionId
            ?: previous.sessionId?.takeIf { previous.targetId == update.targetId }
            ?: 0UL
        val contentChanged = update.text != previousDoc.text
        val dirty = previousDoc.localDirty || contentChanged
        pendingRecord = existing?.copy(
            documentState = previousDoc.copy(
                text = update.text,
                revision = update.revision,
                selectionAnchorUtf8 = if (update.selectionAnchorUtf8 >= 0) update.selectionAnchorUtf8 else previousDoc.selectionAnchorUtf8,
                selectionHeadUtf8 = if (update.selectionHeadUtf8 >= 0) update.selectionHeadUtf8 else previousDoc.selectionHeadUtf8,
                lastAppliedTransactionId = update.transactionId,
                localDirty = dirty,
                ),
            ) ?: EditorSessionRecord(
                targetId = update.targetId,
                documentState = DocumentState(
                    text = update.text, revision = update.revision,
                    selectionAnchorUtf8 = update.selectionAnchorUtf8.coerceAtLeast(0),
                    selectionHeadUtf8 = update.selectionHeadUtf8.coerceAtLeast(0),
                    lastAppliedTransactionId = update.transactionId,
                    localDirty = true,
                ),
            )
        EditorSessionState(
            targetId = update.targetId,
            sessionId = sessionId,
            text = update.text,
            revision = update.revision,
            selectionAnchorUtf8 = if (update.selectionAnchorUtf8 >= 0) update.selectionAnchorUtf8 else previousDoc.selectionAnchorUtf8,
            selectionHeadUtf8 = if (update.selectionHeadUtf8 >= 0) update.selectionHeadUtf8 else previousDoc.selectionHeadUtf8,
            lastAppliedTransactionId = update.transactionId,
            origin = EditorSessionOrigin.UNDO_RESTORED,
            bindingState = previous.bindingState,
            editingState = previous.editingState,
            activeTargetId = previous.activeTargetId,
            committedVersion = previousDoc.committedVersion,
            sessionBaseVersion = previousDoc.sessionBaseVersion,
            localDirty = dirty,
        )
    }
    pendingRecord?.let { store.put(it) }
}

fun EditorSessionCoordinator.applyProgrammaticReplace(update: EditorDocumentUpdate.ProgrammaticReplace) {
    if (!isInputLeaseCurrent(update.lease, update.targetId)) {
        Log.w(EditorSessionCoordinator.TAG, "applyProgrammaticReplace(${update.targetId}): stale input lease rejected")
        return
    }
    var pendingRecord: EditorSessionRecord? = null
    updateSessionState { previous ->
        val existing = store.record(update.targetId)
        val previousDoc = existing?.documentState ?: DocumentState()
        val sessionId = existing?.sessionId
            ?: previous.sessionId?.takeIf { previous.targetId == update.targetId }
            ?: 0UL
        val contentChanged = update.text != previousDoc.text
        val dirty = previousDoc.localDirty || contentChanged
        pendingRecord = existing?.copy(
            documentState = previousDoc.copy(
                text = update.text,
                revision = update.revision,
                selectionAnchorUtf8 = if (update.selectionAnchorUtf8 >= 0) update.selectionAnchorUtf8 else previousDoc.selectionAnchorUtf8,
                selectionHeadUtf8 = if (update.selectionHeadUtf8 >= 0) update.selectionHeadUtf8 else previousDoc.selectionHeadUtf8,
                lastAppliedTransactionId = update.transactionId,
                localDirty = dirty,
                ),
            ) ?: EditorSessionRecord(
                targetId = update.targetId,
                documentState = DocumentState(
                    text = update.text, revision = update.revision,
                    selectionAnchorUtf8 = update.selectionAnchorUtf8.coerceAtLeast(0),
                    selectionHeadUtf8 = update.selectionHeadUtf8.coerceAtLeast(0),
                    lastAppliedTransactionId = update.transactionId,
                    localDirty = true,
                ),
            )
        EditorSessionState(
            targetId = update.targetId,
            sessionId = sessionId,
            text = update.text,
            revision = update.revision,
            selectionAnchorUtf8 = if (update.selectionAnchorUtf8 >= 0) update.selectionAnchorUtf8 else previousDoc.selectionAnchorUtf8,
            selectionHeadUtf8 = if (update.selectionHeadUtf8 >= 0) update.selectionHeadUtf8 else previousDoc.selectionHeadUtf8,
            lastAppliedTransactionId = update.transactionId,
            origin = EditorSessionOrigin.PROGRAMMATIC_REPLACE,
            bindingState = previous.bindingState,
            editingState = previous.editingState,
            activeTargetId = previous.activeTargetId,
            committedVersion = previousDoc.committedVersion,
            sessionBaseVersion = previousDoc.sessionBaseVersion,
            localDirty = dirty,
        )
    }
    pendingRecord?.let { store.put(it) }
}

