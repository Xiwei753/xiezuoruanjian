package com.xiwei.sujian.feature.editor.window

import android.content.Context
import com.xiwei.sujian.core.diagnostics.DiagnosticsEvents
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy
import com.xiwei.sujian.feature.editor.motion.TargetMotionConstraint
import com.xiwei.sujian.feature.editor.projection.ChapterPreviewState
import com.xiwei.sujian.feature.editor.session.AnimationPolicy
import com.xiwei.sujian.feature.editor.session.AuthoritativeEditorSnapshot
import com.xiwei.sujian.feature.editor.session.ChapterSavedSignal
import com.xiwei.sujian.feature.editor.session.EditorInputLease
import com.xiwei.sujian.feature.editor.session.EditorSessionCoordinator
import com.xiwei.sujian.feature.editor.session.EditorSessionHost
import com.xiwei.sujian.feature.editor.session.EditorSessionState
import com.xiwei.sujian.feature.editor.session.ExternalResetResult
import com.xiwei.sujian.feature.editor.projection.SessionCloseReason
import com.xiwei.sujian.feature.editor.session.SessionCommandPort
import com.xiwei.sujian.feature.editor.session.SessionResetSource
import com.xiwei.sujian.feature.editor.session.TargetCommand
import com.xiwei.sujian.feature.editor.session.TargetCommandResult
import com.xiwei.sujian.feature.editor.session.TargetDecorations
import com.xiwei.sujian.feature.editor.session.TargetSnapshot
import com.xiwei.sujian.feature.editor.session.TextEditorProfile
import com.xiwei.sujian.feature.editor.session.WindowBindingState
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow

/**
 * #641 评论1 第7节：窗口 facade — 只保留窗口级标识、排版诊断透传和
 * CompositionLocal 兼容。session 操作（target/session 命令、undo/redo、
 * save/close、外部正文事实同步）全部委托给 [sessionHost]（真正的 session owner）。
 *
 * #641：正文输入/排版/光标/换行/滚动由 [WritingEditorSurface] 的 state-based
 * [BasicTextField] 接管；本类不持有 View、FrameClock、InputConnection 或几何。
 */
class EditorWindowHost(
    @Suppress("unused") private val context: Context,
    val sessionCoordinator: EditorSessionCoordinator,
    private val appServiceBridge: AppServiceBridge,
) : SessionCommandPort {
    /** #641：真正的 session owner — target/session 命令、undo/redo、save/close、外部事实同步。 */
    val sessionHost: EditorSessionHost = EditorSessionHost(sessionCoordinator, appServiceBridge)

    /** #592 二：窗口标识 — 同一窗口内的 Compose onDispose 用它调用 detachWindowBinding。 */
    val windowId: String = "window:${System.identityHashCode(this)}"

    val sessionStateFlow: StateFlow<EditorSessionState> get() = sessionHost.sessionStateFlow
    val targetDecorationsVersionFlow: StateFlow<Long> get() = sessionHost.targetDecorationsVersionFlow
    val lastCommittedTextFlow: StateFlow<String?> get() = sessionHost.lastCommittedTextFlow
    val motionPolicyFlow: StateFlow<EditorMotionPolicy> get() = sessionHost.motionPolicyFlow
    val chapterSavedSignal: SharedFlow<ChapterSavedSignal> get() = sessionHost.chapterSavedSignal

    /**
     * #641 评论 问题7c：转发 [EditorSessionHost.authoritativeEditorSnapshots] —
     * UI 层（WritingPaneEditorContent）通过 EditorWindowHost 收集 undo/redo 后的权威正文。
     */
    val authoritativeEditorSnapshots: SharedFlow<AuthoritativeEditorSnapshot>
        get() = sessionHost.authoritativeEditorSnapshots

    val activeTargetId: String? get() = sessionHost.activeTargetId
    val editingState: EditingState get() = sessionHost.editingState
    val windowBindingState: WindowBindingState get() = sessionHost.windowBindingState
    val targetDecorationsVersion: Long get() = sessionHost.targetDecorationsVersion
    val lastCommittedText: String? get() = sessionHost.lastCommittedText

    fun registerTarget(target: EditableTextTarget) = sessionHost.registerTarget(target)

    fun updateTargetSpec(
        targetId: String,
        profile: TextEditorProfile? = null,
    ) = sessionHost.updateTargetSpec(targetId, profile = profile)

    fun detachWindowBinding(
        windowId: String,
        targetId: String,
    ) = sessionHost.detachWindowBinding(windowId, targetId)

    /**
     * #592 三：业务级关闭 — 由 workspace 导航事件调用。委托给 [sessionHost]。
     */
    fun closeTarget(
        targetId: String,
        reason: SessionCloseReason,
    ) = sessionHost.closeTarget(targetId, reason)

    fun getPersistentSessionId(targetId: String): ULong? = sessionHost.getPersistentSessionId(targetId)

    fun getChapterPreviewState(targetId: String): ChapterPreviewState? = sessionHost.getChapterPreviewState(targetId)

    /**
     * #641 评论 5457777142 问题7：暴露 [TargetDecorations] 给 UI 层 —
     * 供 [com.xiwei.sujian.feature.editor.ui.WritingPaneEditorContent] 读取搜索高亮。
     */
    fun getTargetDecorations(targetId: String): TargetDecorations? =
        sessionHost.sessionCoordinator.getTargetDecorations(targetId)

    fun applyMotionPolicy(policy: EditorMotionPolicy) = sessionHost.applyMotionPolicy(policy)

    /**
     * #624 评论3/4：排版设置持续应用 — 窗口层唯一设置写入点（诊断 + 透传 Compose 层）。
     * #641：排版由 Compose [BasicTextField] 直接应用，本方法只做诊断记录。
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
     * #624 评论5：导航离开正文前收 IME。
     * #641：IME 由 Compose [BasicTextField] 生命周期管理，离开 Composition 自动收起。
     */
    fun dismissImeForNavigation() {
        // IME 由 Compose BasicTextField 生命周期管理，离开 Composition 自动收起。
    }

    /** #625 评论项3：撤销 — 委托给 [sessionHost]。 */
    fun performUndo() = sessionHost.performUndo()

    /** #625 评论项3：重做 — 委托给 [sessionHost]。 */
    fun performRedo() = sessionHost.performRedo()

    /** #592 二：开始编辑 — 委托给 [sessionHost] 预准备 Rust session。 */
    fun beginEdit(
        targetId: String,
        initialSelection: Int? = null,
    ): Boolean = sessionHost.beginEdit(targetId, initialSelection, windowId)

    /** 提交活动编辑 — 委托给 [sessionHost]。 */
    fun commitActiveEdit(): Boolean = sessionHost.commitActiveEdit()

    /** 取消活动编辑 — 委托给 [sessionHost]。 */
    fun cancelActiveEdit(): Boolean = sessionHost.cancelActiveEdit()

    /** 重置持久会话 — 委托给 [sessionHost]。 */
    fun resetPersistentSession(
        targetId: String,
        text: String,
        cursorUtf8: Int,
        source: SessionResetSource = SessionResetSource.EXTERNAL,
    ): ExternalResetResult = sessionHost.resetPersistentSession(targetId, text, cursorUtf8, source)

    override fun queryTargetSnapshot(targetId: String): TargetSnapshot? = sessionHost.queryTargetSnapshot(targetId)

    override fun applyTargetCommand(
        targetId: String,
        command: TargetCommand,
    ): TargetCommandResult = sessionHost.applyTargetCommand(targetId, command)

    override fun setTargetDecorations(
        targetId: String,
        decorations: TargetDecorations,
    ) = sessionHost.setTargetDecorations(targetId, decorations)

    /**
     * #592 二：窗口销毁时完整释放，但保留 Rust 会话。
     *
     * #644 评论 5462826712 第5节：删除 ProjectionSnapshot(viewportAnchor = null) 写入。
     * release 只负责 detach；真实 viewport 已由 Compose surface 在 dispose 时保存。
     */
    fun releaseWindow() {
        val activeId = activeTargetId
        if (activeId != null) {
            sessionHost.detachWindowBinding(windowId, activeId)
        }
    }

    /** Activity 永久结束 — 释放窗口和全部会话。 */
    fun releaseHost() = sessionHost.releaseHost()

    /**
     * #644 评论 5462826712 第1节：Compose Surface 附着 —
     * 窗口层用自己的 windowId 完成 binding；UI 不知道 sessionId。
     */
    fun attachSurface(targetId: String): EditorInputLease? =
        sessionHost.attachSurface(windowId, targetId)

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
}

/**
 * #624 评论3/4：排版设置 — 字号、行距、首行缩进（开关 + 字符宽度）。
 * data class 以便 == 幂等比较（切章不重排）。
 */
data class EditorTypography(
    val fontSizeSp: Float,
    val lineSpacingMultiplier: Float,
    val autoIndentEnabled: Boolean,
    val autoIndentWidth: Float,
)
