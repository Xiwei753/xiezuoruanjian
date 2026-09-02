package com.xiwei.sujian.feature.editor.window

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.xiwei.sujian.feature.editor.session.CommitPolicy
import com.xiwei.sujian.feature.editor.session.TextEditorProfile

/**
 * Registration descriptor for an editable text region within the window.
 *
 * Per #541: each target represents a UI/business object (project title, chapter body,
 * search query, starmap node, etc.) that can be activated for editing via
 * [EditorWindowHost.beginEdit]. The [targetId] is a namespaced string
 * (e.g. "chapter-title:{chapterId}") that identifies the business object; it is
 * separate from the Rust TextEditSessionId which represents an editing transaction.
 *
 * [isPersistent] determines the session lifecycle: persistent targets (chapter body)
 * keep their session across edits; draft targets (project title) close on every commit.
 * [commitPolicy] controls when the coordinator commits text to the domain model.
 *
 * #644 评论 5467821839 第5节：删除 currentGeometry/currentTransform/currentText/
 * updateGeometry/updateTransform/updateText/onTextChanged/Transform2D。
 * 正文持久化由 state-based BasicTextField + TextFieldState 接管，不再由 target 持有第三份正文。
 */
class EditableTextTarget(
    val targetId: String,
    profile: TextEditorProfile = TextEditorProfile(),
    isPersistent: Boolean = false,
    commitPolicy: CommitPolicy =
        if (isPersistent) CommitPolicy.COMMIT_ON_EVERY_CHANGE else CommitPolicy.COMMIT_ON_CONFIRM,
) {
    var profile: TextEditorProfile by mutableStateOf(profile)
        private set
    var isPersistent: Boolean by mutableStateOf(isPersistent)
        private set
    var commitPolicy: CommitPolicy by mutableStateOf(commitPolicy)
        private set
    var onCommit: ((String) -> Unit)? = null
    var onCancel: (() -> Unit)? = null
    var onEditingStateChanged: ((EditingState) -> Unit)? = null

    /**
     * #624 评论9：轻量编辑应用回调 — 热路径不传整章 String，只传 [EditorAppliedEvent]。
     * 由 [EditorWindowHost.installContentCallback] 在 onLocalEdit/onExternalEdit
     * 完成会话层 reducer 后调用，ViewModel 据此做保存调度/统计/字数增量。
     */
    var onEditorApplied: ((com.xiwei.sujian.feature.editor.session.EditorAppliedEvent) -> Unit)? = null

    fun updateProfile(newProfile: TextEditorProfile) {
        profile = newProfile
    }

    fun updatePersistent(persistent: Boolean) {
        isPersistent = persistent
    }

    fun updateCommitPolicy(newCommitPolicy: CommitPolicy) {
        commitPolicy = newCommitPolicy
    }
}

/**
 * Lifecycle states of the shared editing host within the coordinator.
 *
 * Per #541: the editing host transitions through these states as the
 * coordinator binds, rebinds, commits, or cancels editing targets. #641 后正文
 * 由 state-based [BasicTextField]([TextFieldState]) 渲染，不再有传统 View
 * InputConnection；只有 EDITING 产生活动输入，其余状态准备或拆除 session。
 *
 * State transitions:
 * - IDLE → BINDING (beginEdit called)
 * - BINDING → EDITING (session created and host bound)
 * - EDITING → COMMITTING / CANCELLING (user or programmatic action)
 * - EDITING → REBINDING (beginEdit called for a different target while editing)
 * - COMMITTING / CANCELLING / REBINDING → IDLE (action completed)
 * - IDLE → RELEASED (releaseHost called — terminal, host cannot be reused)
 */
enum class EditingState {
    IDLE,
    BINDING,
    EDITING,
    COMMITTING,
    CANCELLING,
    REBINDING,
    RELEASED,
}
