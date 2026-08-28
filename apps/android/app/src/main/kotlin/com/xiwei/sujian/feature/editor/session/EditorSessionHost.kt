package com.xiwei.sujian.feature.editor.session

import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.feature.editor.interop.TextEditSessionBridge
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy
import com.xiwei.sujian.feature.editor.projection.ChapterPreviewState
import com.xiwei.sujian.feature.editor.projection.TextRange
import com.xiwei.sujian.feature.editor.window.EditableTextTarget
import com.xiwei.sujian.feature.editor.window.EditingState
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow

/**
 * #641 评论1 第7节：编辑会话宿主 — 真正的 session owner。
 *
 * 持有/转发 [EditorSessionCoordinator] 的 target/session 命令、undo/redo、
 * save/close、外部正文事实同步。不持有窗口对象、Compose 可变状态、View、
 * FrameClock 或 InputConnection — 那些属于窗口/显示层。
 *
 * [EditorWindowHost] 只保留必要窗口 facade（windowId、排版诊断透传、
 * CompositionLocal 兼容），session 操作全部委托给本类。
 *
 * Core session/save/undo/redo/dirty/autosave 语义全部经
 * [EditorSessionCoordinator] 保持唯一事实来源。
 */
class EditorSessionHost(
    val sessionCoordinator: EditorSessionCoordinator,
    private val appServiceBridge: AppServiceBridge,
) : SessionCommandPort {
    val sessionStateFlow: StateFlow<EditorSessionState> get() = sessionCoordinator.sessionStateFlow
    val targetDecorationsVersionFlow: StateFlow<Long> get() = sessionCoordinator.targetDecorationsVersionFlow
    val lastCommittedTextFlow: StateFlow<String?> get() = sessionCoordinator.lastCommittedTextFlow
    val motionPolicyFlow: StateFlow<EditorMotionPolicy> get() = sessionCoordinator.motionPolicyFlow
    val chapterSavedSignal: SharedFlow<ChapterSavedSignal> get() = sessionCoordinator.chapterSavedSignal

    val activeTargetId: String? get() = sessionCoordinator.activeTargetId
    val editingState: EditingState get() = sessionCoordinator.editingState
    val windowBindingState: WindowBindingState get() = sessionCoordinator.windowBindingState
    val targetDecorationsVersion: Long get() = sessionCoordinator.targetDecorationsVersion
    val lastCommittedText: String? get() = sessionCoordinator.lastCommittedText

    fun registerTarget(target: EditableTextTarget) {
        sessionCoordinator.registerTarget(target)
    }

    fun updateTargetSpec(
        targetId: String,
        profile: TextEditorProfile? = null,
    ) {
        sessionCoordinator.updateTargetSpec(targetId, profile = profile)
    }

    fun detachWindowBinding(
        windowId: String,
        targetId: String,
    ) {
        sessionCoordinator.detachWindowBinding(windowId, targetId)
    }

    fun closeTarget(
        targetId: String,
        reason: SessionCloseReason,
    ) {
        sessionCoordinator.closeTarget(targetId, reason)
    }

    fun getPersistentSessionId(targetId: String): ULong? = sessionCoordinator.getPersistentSessionId(targetId)

    fun getChapterPreviewState(targetId: String): ChapterPreviewState? {
        val snapshot = sessionCoordinator.queryTargetSnapshot(targetId) ?: return null
        val decorations = sessionCoordinator.getTargetDecorations(targetId)
        val searchHighlights =
            decorations?.searchHighlightsUtf8?.map {
                TextRange(it.first, it.second)
            } ?: emptyList()
        val selection =
            if (decorations != null &&
                decorations.selectionStartUtf8 >= 0 && decorations.selectionEndUtf8 >= 0
            ) {
                TextRange(decorations.selectionStartUtf8, decorations.selectionEndUtf8)
            } else {
                null
            }
        return ChapterPreviewState(
            text = snapshot.text,
            revision = snapshot.revision,
            selection = selection,
            searchHighlights = searchHighlights,
        )
    }

    fun applyMotionPolicy(policy: EditorMotionPolicy) {
        sessionCoordinator.applyMotionPolicy(policy)
    }

    /**
     * #625 评论项3：撤销 — 经 [TextEditSessionBridge] 调 Rust undo，
     * 结果经 [EditorSessionCoordinator.applyUndoRestored] 送入会话层 reducer。
     */
    fun performUndo() {
        val lease = sessionCoordinator.currentInputLease() ?: return
        val snapshot = sessionCoordinator.queryTargetSnapshot(lease.targetId) ?: return
        val bridge = TextEditSessionBridge(appServiceBridge, lease.sessionId)
        val result = bridge.undo(snapshot.revision) ?: return
        sessionCoordinator.applyUndoRestored(
            EditorDocumentUpdate.UndoRestored(
                targetId = lease.targetId,
                snapshotId = result.newRevision.toLong(),
                revision = result.newRevision.toLong(),
                transactionId = result.transactionId.toLong(),
                selectionAnchorUtf8 = result.newSelectionStart.toInt(),
                selectionHeadUtf8 = result.newSelectionEnd.toInt(),
                lease = lease,
                contentChanged = result.displayPatches.isNotEmpty(),
            ),
        )
    }

    /**
     * #625 评论项3：重做 — 同 [performUndo]。
     */
    fun performRedo() {
        val lease = sessionCoordinator.currentInputLease() ?: return
        val snapshot = sessionCoordinator.queryTargetSnapshot(lease.targetId) ?: return
        val bridge = TextEditSessionBridge(appServiceBridge, lease.sessionId)
        val result = bridge.redo(snapshot.revision) ?: return
        sessionCoordinator.applyUndoRestored(
            EditorDocumentUpdate.UndoRestored(
                targetId = lease.targetId,
                snapshotId = result.newRevision.toLong(),
                revision = result.newRevision.toLong(),
                transactionId = result.transactionId.toLong(),
                selectionAnchorUtf8 = result.newSelectionStart.toInt(),
                selectionHeadUtf8 = result.newSelectionEnd.toInt(),
                lease = lease,
                contentChanged = result.displayPatches.isNotEmpty(),
            ),
        )
    }

    /**
     * #592 二：开始编辑 — 预准备 Rust session，不附带 View 绑定。
     */
    fun beginEdit(
        targetId: String,
        initialSelection: Int? = null,
        windowId: String,
    ): Boolean {
        val currentActiveId = sessionCoordinator.activeTargetId
        if (currentActiveId != null && currentActiveId != targetId) {
            saveActiveTargetProjection(currentActiveId)
        }
        val bindInfo =
            sessionCoordinator.prepareSessionForEdit(
                targetId,
                "",
                initialSelection,
                windowId,
            ) ?: return false
        return bindInfo != null
    }

    fun commitActiveEdit(): Boolean {
        val targetId = activeTargetId ?: return false
        return sessionCoordinator.commitActiveSession(null)
    }

    fun cancelActiveEdit(): Boolean = sessionCoordinator.cancelActiveSession()

    fun resetPersistentSession(
        targetId: String,
        text: String,
        cursorUtf8: Int,
        source: SessionResetSource = SessionResetSource.EXTERNAL,
    ): ExternalResetResult = sessionCoordinator.resetPersistentSession(targetId, text, cursorUtf8, source)

    override fun queryTargetSnapshot(targetId: String): TargetSnapshot? =
        sessionCoordinator.queryTargetSnapshot(targetId)

    override fun applyTargetCommand(
        targetId: String,
        command: TargetCommand,
    ): TargetCommandResult = sessionCoordinator.executeTargetCommand(targetId, command)

    override fun setTargetDecorations(
        targetId: String,
        decorations: TargetDecorations,
    ) {
        sessionCoordinator.setTargetDecorations(targetId, decorations)
    }

    fun releaseHost() {
        sessionCoordinator.releaseHost()
    }

    fun saveProjectionSnapshot(
        targetId: String,
        snapshot: ProjectionSnapshot,
    ) {
        sessionCoordinator.saveProjectionSnapshot(targetId, snapshot)
    }

    private fun saveActiveTargetProjection(targetId: String) {
        sessionCoordinator.saveProjectionSnapshot(
            targetId,
            ProjectionSnapshot(viewportAnchor = null),
        )
    }
}
