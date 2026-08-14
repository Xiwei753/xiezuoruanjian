package com.xiwei.sujian.feature.editor.session

// ! # 编辑器会话编辑操作（从 EditorSessionCoordinator 拆分）

import android.util.Log

// #597：本地输入/撤销恢复/程序化替换三类编辑事件共用同一 reducer 骨架
// （lease 校验 → sessionId/dirty 计算 → documentState 更新 → SessionState 构建），
// 差异只有 dirty 规则与来源标记；收敛到 applyLocalUpdate 后各入口只保留类型。
// #624 评论9：热路径不传整章 String — dirty 判定用 [EditorDocumentUpdate.contentChanged]
// 替代旧 `update.text != previousDoc.text` 字符串比较。

/**
 * #597：本地编辑事件统一 reducer — 校验 lease 后原子更新
 * session/document/revision 三态（updateSessionState 单点写入 + pendingRecord
 * 落 store），与拆分前行为完全一致。
 */
private fun EditorSessionCoordinator.applyLocalUpdate(
    update: EditorDocumentUpdate,
    origin: EditorSessionOrigin,
    dirtyRule: (EditorDocumentUpdate, DocumentState) -> Boolean,
) {
    if (!isInputLeaseCurrent(update.lease, update.targetId)) {
        Log.w(EditorSessionCoordinator.TAG, "applyLocalUpdate(${update.targetId}): stale input lease rejected")
        return
    }
    var pendingRecord: EditorSessionRecord? = null
    updateSessionState { previous ->
        val existing = store.record(update.targetId)
        val previousDoc = existing?.documentState ?: DocumentState()
        val sessionId =
            existing?.sessionId
                ?: previous.sessionId?.takeIf { previous.targetId == update.targetId }
                ?: 0UL
        val dirty = dirtyRule(update, previousDoc)
        pendingRecord =
            existing?.copy(
                documentState =
                    previousDoc.copy(
                        revision = update.revision,
                        selectionAnchorUtf8 =
                            if (update.selectionAnchorUtf8 >= 0) {
                                update.selectionAnchorUtf8
                            } else {
                                previousDoc.selectionAnchorUtf8
                            },
                        selectionHeadUtf8 =
                            if (update.selectionHeadUtf8 >= 0) {
                                update.selectionHeadUtf8
                            } else {
                                previousDoc.selectionHeadUtf8
                            },
                        lastAppliedTransactionId = update.transactionId,
                        localDirty = dirty,
                    ),
            ) ?: EditorSessionRecord(
                targetId = update.targetId,
                documentState =
                    DocumentState(
                        revision = update.revision,
                        selectionAnchorUtf8 = update.selectionAnchorUtf8.coerceAtLeast(0),
                        selectionHeadUtf8 = update.selectionHeadUtf8.coerceAtLeast(0),
                        lastAppliedTransactionId = update.transactionId,
                        localDirty = true,
                    ),
            )
        EditorSessionState(
            targetId = update.targetId,
            sessionId = sessionId,
            revision = update.revision,
            selectionAnchorUtf8 =
                if (update.selectionAnchorUtf8 >= 0) {
                    update.selectionAnchorUtf8
                } else {
                    previousDoc.selectionAnchorUtf8
                },
            selectionHeadUtf8 =
                if (update.selectionHeadUtf8 >= 0) {
                    update.selectionHeadUtf8
                } else {
                    previousDoc.selectionHeadUtf8
                },
            lastAppliedTransactionId = update.transactionId,
            origin = origin,
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

/**
 * #624 评论9：本地输入 dirty 规则 — 用 [EditorDocumentUpdate.contentChanged]
 * 替代旧 `update.text != previousDoc.text` 字符串比较。
 * selection-only 且 !contentChanged 时保留原 dirty，否则 contentChanged。
 */
private fun localInputDirty(
    update: EditorDocumentUpdate,
    previousDoc: DocumentState,
): Boolean {
    val contentChanged = update.contentChanged
    return if (!contentChanged &&
        update is EditorDocumentUpdate.LocalInput &&
        update.operationKind == EditorOperationKind.SELECTION
    ) {
        previousDoc.localDirty
    } else {
        contentChanged
    }
}

fun EditorSessionCoordinator.applyLocalEdit(update: EditorDocumentUpdate.LocalInput) {
    applyLocalUpdate(update, EditorSessionOrigin.LOCAL_INPUT, ::localInputDirty)
}

fun EditorSessionCoordinator.applyUndoRestored(update: EditorDocumentUpdate.UndoRestored) {
    applyLocalUpdate(update, EditorSessionOrigin.UNDO_RESTORED) { update, previousDoc ->
        previousDoc.localDirty || update.contentChanged
    }
}

fun EditorSessionCoordinator.applyProgrammaticReplace(update: EditorDocumentUpdate.ProgrammaticReplace) {
    applyLocalUpdate(update, EditorSessionOrigin.PROGRAMMATIC_REPLACE) { update, previousDoc ->
        previousDoc.localDirty || update.contentChanged
    }
}
