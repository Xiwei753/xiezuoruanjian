package com.xiwei.sujian.feature.editor.window

import android.content.Context
import com.xiwei.sujian.core.diagnostics.DiagnosticsEvents
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.feature.editor.interop.TextEditSessionBridge
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy
import com.xiwei.sujian.feature.editor.motion.TargetMotionConstraint
import com.xiwei.sujian.feature.editor.projection.ChapterPreviewState
import com.xiwei.sujian.feature.editor.projection.TextRange
import com.xiwei.sujian.feature.editor.session.AnimationPolicy
import com.xiwei.sujian.feature.editor.session.ChapterSavedSignal
import com.xiwei.sujian.feature.editor.session.EditorDocumentUpdate
import com.xiwei.sujian.feature.editor.session.EditorSessionCoordinator
import com.xiwei.sujian.feature.editor.session.EditorSessionState
import com.xiwei.sujian.feature.editor.session.ExternalResetResult
import com.xiwei.sujian.feature.editor.session.ProjectionSnapshot
import com.xiwei.sujian.feature.editor.session.SessionCloseReason
import com.xiwei.sujian.feature.editor.session.SessionCommandPort
import com.xiwei.sujian.feature.editor.session.SessionResetSource
import com.xiwei.sujian.feature.editor.session.TargetCommand
import com.xiwei.sujian.feature.editor.session.TargetCommandResult
import com.xiwei.sujian.feature.editor.session.TargetDecorations
import com.xiwei.sujian.feature.editor.session.TargetSnapshot
import com.xiwei.sujian.feature.editor.session.TextEditorProfile
import com.xiwei.sujian.feature.editor.session.WindowBindingState
import com.xiwei.sujian.feature.editor.session.applyUndoRestored
import com.xiwei.sujian.feature.editor.session.cancelActiveSession
import com.xiwei.sujian.feature.editor.session.closeTarget
import com.xiwei.sujian.feature.editor.session.commitActiveSession
import com.xiwei.sujian.feature.editor.session.detachWindowBinding
import com.xiwei.sujian.feature.editor.session.prepareSessionForEdit
import com.xiwei.sujian.feature.editor.session.releaseHost
import com.xiwei.sujian.feature.editor.session.resetPersistentSession
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow

/**
 * #641 评论2 第7节：Window/View 层拆干净 — [EditorWindowHost] 只保留 session 协调。
 *
 * 删除（#641）：
 * - `PresentationReadinessGate` / `presentationReady` / `presentationReadyGeneration` /
 *   `awaitPresentationReady()` / `registerPresentationReadyCallback()` /
 *   `invalidatePresentationReady()`
 * - `sharedEditorView` / `pendingViewBind` / `createWindowView()` / `attachView()` /
 *   `detachView()` / `updateView()`
 * - `WindowDisplayFrameClock` / View `InputConnection` 回调 / View 版 typography apply
 * - `EditorPresentationReady`
 *
 * 保留：
 * - [EditorSessionCoordinator] 转发（target/session 命令、undo/redo、save/close、
 *   外部正文事实同步）
 * - [motionPolicyFlow] 唯一动画策略流
 * - [applyEditorTypography] 窗口层唯一排版设置写入点（诊断 + 透传 Compose 层）
 *
 * #641：正文输入/排版/光标/换行/滚动现由 [WritingEditorSurface] 的 state-based
 * [BasicTextField] 接管；本类不再持有 View/FrameClock/几何，只做 session 协调。
 */
class EditorWindowHost(
    private val context: Context,
    val sessionCoordinator: EditorSessionCoordinator,
    private val appServiceBridge: AppServiceBridge,
) : SessionCommandPort {
    /** #592 二：窗口标识 — 同一窗口内的 Compose onDispose 用它调用 detachWindowBinding。 */
    val windowId: String = "window:${System.identityHashCode(this)}"

    // #595 三：窗口层只转发唯一 [sessionStateFlow]；activeTargetId / editingState /
    // windowBindingState 三个独立 stateIn 派生流已删除，Compose 消费者从同一个
    // EditorSessionState 快照读取，值 getter 供非 Compose 调用方读取当前值。

    val sessionStateFlow: StateFlow<EditorSessionState> get() = sessionCoordinator.sessionStateFlow
    val targetDecorationsVersionFlow: StateFlow<Long> get() = sessionCoordinator.targetDecorationsVersionFlow
    val lastCommittedTextFlow: StateFlow<String?> get() = sessionCoordinator.lastCommittedTextFlow

    /**
     * #595 三/七：唯一动画策略流 — 转发 [EditorSessionCoordinator.motionPolicyFlow]。
     * 窗口层不持有第二份动画状态；[applyMotionPolicy] 是唯一写入点。
     */
    val motionPolicyFlow: StateFlow<EditorMotionPolicy>
        get() = sessionCoordinator.motionPolicyFlow

    /**
     * #625 项6：章节保存成功信号 — 转发 [EditorSessionCoordinator.chapterSavedSignal]。
     * app 层收集该流后调用 refreshProjectSummaries()，使作品卡字数在保存后及时刷新，
     * 不再仅靠 RESUMED 生命周期。纯事件，不镜像业务数据。
     */
    val chapterSavedSignal: SharedFlow<ChapterSavedSignal>
        get() = sessionCoordinator.chapterSavedSignal

    val activeTargetId: String? get() = sessionCoordinator.activeTargetId
    val editingState: EditingState get() = sessionCoordinator.editingState
    val windowBindingState: WindowBindingState get() = sessionCoordinator.windowBindingState
    val targetDecorationsVersion: Long get() = sessionCoordinator.targetDecorationsVersion
    val lastCommittedText: String? get() = sessionCoordinator.lastCommittedText

    // ── Target management (pure metadata mirrored to session layer) ──

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

    /**
     * #592 三：业务级关闭 — 由 workspace 导航事件调用（返回章节列表、切换章节、
     * 删除章节）。与窗口解绑分开：关闭销毁 Rust session，解绑只解除窗口引用。
     */
    fun closeTarget(
        targetId: String,
        reason: SessionCloseReason,
    ) {
        sessionCoordinator.closeTarget(targetId, reason)
    }

    fun getPersistentSessionId(targetId: String): ULong? = sessionCoordinator.getPersistentSessionId(targetId)

    /**
     * #595 九：非活动章节预览的纯静态状态 — 不经 TargetDisplayRuntime，
     * 直接从会话层 snapshot 和装饰构建，不含动画引擎或 Bitmap 资源。
     */
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

    /**
     * #595 三/七：原子应用 [EditorMotionPolicy] — 唯一可写事实源。
     * 一次更新文字、光标、协同、时长和 reduce-motion；target profile 只作为约束
     * （SYSTEM_SUPPRESSED → forceStatic）在唯一计算点参与 effectivePolicy，
     * 不会成为第二个动画状态写入者。#641：不再同步推送到 View（View 已删除）。
     */
    fun applyMotionPolicy(policy: EditorMotionPolicy) {
        sessionCoordinator.applyMotionPolicy(policy)
    }

    /**
     * #624 评论3/4：排版设置持续应用 — 字号、行距、首行缩进（开关 + 字符宽度）。
     * #641：排版设置现在由 Compose 层 [WritingEditorSurface] 直接应用到
     * [BasicTextField]，本方法只做诊断记录 + 透传（窗口层唯一设置写入点）。
     */
    fun applyEditorTypography(
        fontSizeSp: Float,
        lineSpacingMultiplier: Float,
        autoIndentEnabled: Boolean,
        autoIndentWidth: Float,
    ) {
        DiagnosticsEvents.editorTypography(
            fontSizeSp = fontSizeSp,
            lineSpacing = lineSpacingMultiplier,
            firstLineIndent = autoIndentEnabled,
            indentChars = autoIndentWidth,
        )
    }

    /**
     * #624 评论5：导航离开正文前先立刻收 IME。
     * #641：IME 现由 Compose [BasicTextField] 管理；导航离开时正文 UI 离开
     * Composition，系统自动收起 IME。本方法保留为 no-op 兼容入口（顶栏调用）。
     */
    fun dismissImeForNavigation() {
        // IME 由 Compose BasicTextField 生命周期管理，离开 Composition 自动收起。
    }

    /**
     * #625 评论项3：撤销 — 通过 [TextEditSessionBridge] 调 Rust undo，结果经
     * [EditorSessionCoordinator.applyUndoRestored] 送入会话层 reducer
     * （dirty/保存/字数/动画同一份状态），不直接跨 UniFFI 调 undo/redo。
     * #641：旧路径 sharedEditorView.performUndo() 已随 View 成员删除；
     * 现在由窗口层直接桥接到 Core session，再经 Coordinator 业务链应用。
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
     * #625 评论项3：重做 — 同 [performUndo]，经 [TextEditSessionBridge.redo] +
     * [EditorSessionCoordinator.applyUndoRestored] 走现有 session 编辑链。
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

    // ── Edit operations (orchestrates session) ──

    /**
     * #592 二：开始编辑 — 通过 [EditorSessionCoordinator.prepareSessionForEdit]
     * 预准备 Rust session。#641：不再附带 View 绑定/typography（View 已删除，
     * Compose [WritingEditorSurface] 直接消费 session 状态）。
     */
    fun beginEdit(
        targetId: String,
        initialSelection: Int? = null,
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

    /**
     * 提交活动编辑 — 转发 [EditorSessionCoordinator.commitActiveSession]。
     */
    fun commitActiveEdit(): Boolean {
        val targetId = activeTargetId ?: return false
        return sessionCoordinator.commitActiveSession(null)
    }

    /**
     * 取消活动编辑 — 转发 [EditorSessionCoordinator.cancelActiveSession]。
     */
    fun cancelActiveEdit(): Boolean {
        return sessionCoordinator.cancelActiveSession()
    }

    /**
     * 重置持久会话 — 转发 [EditorSessionCoordinator.resetPersistentSession]。
     */
    fun resetPersistentSession(
        targetId: String,
        text: String,
        cursorUtf8: Int,
        source: SessionResetSource = SessionResetSource.EXTERNAL,
    ): ExternalResetResult {
        return sessionCoordinator.resetPersistentSession(targetId, text, cursorUtf8, source)
    }

    // ── SessionCommandPort (projection for inactive targets) ──

    override fun queryTargetSnapshot(targetId: String): TargetSnapshot? =
        sessionCoordinator.queryTargetSnapshot(targetId)

    override fun applyTargetCommand(
        targetId: String,
        command: TargetCommand,
    ): TargetCommandResult {
        return sessionCoordinator.executeTargetCommand(targetId, command)
    }

    override fun setTargetDecorations(
        targetId: String,
        decorations: TargetDecorations,
    ) {
        sessionCoordinator.setTargetDecorations(targetId, decorations)
    }

    // ── Window lifecycle ──

    /**
     * #592 二：窗口销毁时完整释放，但保留 Rust 会话。
     * 窗口状态由会话层窗口绑定状态机统一维护，新窗口创建后通过
     * [beginEdit] 的 attach 路径自动附着旧 session。
     */
    fun releaseWindow() {
        val activeId = activeTargetId
        if (activeId != null) {
            saveActiveTargetProjection(activeId)
            sessionCoordinator.detachWindowBinding(windowId, activeId)
        }
    }

    /**
     * Activity 永久结束 — 释放窗口和全部会话。
     */
    fun releaseHost() {
        sessionCoordinator.releaseHost()
    }

    // ── Private helpers ──

    /**
     * 根据 target profile 的 animationPolicy 计算有效动画约束。
     * 纯函数，供绑定和测试使用。
     */
    companion object {
        private const val TAG = "EditorWindowHost"

        fun constraintFor(profile: TextEditorProfile?): TargetMotionConstraint {
            if (profile == null) return TargetMotionConstraint()
            return when (profile.animationPolicy) {
                AnimationPolicy.SYSTEM_SUPPRESSED -> TargetMotionConstraint(forceStatic = true)
                AnimationPolicy.ENABLED -> TargetMotionConstraint()
                AnimationPolicy.INHERIT_GLOBAL -> TargetMotionConstraint()
            }
        }
    }

    /**
     * 把窗口滚动位置保存为会话层纯数据快照。
     * #641：滚动位置现由 Compose [BasicTextField] 的 ScrollState 持有；
     * 这里只保存空的投影快照（viewportAnchor=null），保持 session 层接口完整。
     */
    private fun saveActiveTargetProjection(targetId: String) {
        sessionCoordinator.saveProjectionSnapshot(
            targetId,
            ProjectionSnapshot(
                viewportAnchor = null,
            ),
        )
    }
}

/**
 * #624 评论3/4：排版设置 — 字号、行距、首行缩进（开关 + 字符宽度）。
 * #641：排版设置现在由 Compose 层 [WritingEditorSurface] 直接应用到 [BasicTextField]，
 * [EditorWindowHost.applyEditorTypography] 仍作为窗口层唯一设置写入点（诊断 + 透传）。
 * data class 以便 == 幂等比较（切章不重排）。
 */
data class EditorTypography(
    val fontSizeSp: Float,
    val lineSpacingMultiplier: Float,
    val autoIndentEnabled: Boolean,
    val autoIndentWidth: Float,
)
