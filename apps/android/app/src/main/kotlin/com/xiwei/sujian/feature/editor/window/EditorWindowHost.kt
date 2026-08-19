package com.xiwei.sujian.feature.editor.window

import android.content.Context
import android.graphics.Rect
import android.util.Log
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.feature.editor.interop.TextEditSessionBridge
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy
import com.xiwei.sujian.feature.editor.motion.TargetMotionConstraint
import com.xiwei.sujian.feature.editor.platform.SujianEditorView
import com.xiwei.sujian.feature.editor.projection.ChapterPreviewState
import com.xiwei.sujian.feature.editor.projection.TextRange
import com.xiwei.sujian.feature.editor.session.AnimationPolicy
import com.xiwei.sujian.feature.editor.session.ChapterSavedSignal
import com.xiwei.sujian.feature.editor.session.EditorDocumentUpdate
import com.xiwei.sujian.feature.editor.session.EditorInputLease
import com.xiwei.sujian.feature.editor.session.EditorSessionCoordinator
import com.xiwei.sujian.feature.editor.session.EditorSessionState
import com.xiwei.sujian.feature.editor.session.ExternalResetResult
import com.xiwei.sujian.feature.editor.session.ProjectionSnapshot
import com.xiwei.sujian.feature.editor.session.ViewportAnchor
import com.xiwei.sujian.feature.editor.session.SessionCloseReason
import com.xiwei.sujian.feature.editor.session.SessionCommandPort
import com.xiwei.sujian.feature.editor.session.SessionResetSource
import com.xiwei.sujian.feature.editor.session.TargetCommand
import com.xiwei.sujian.feature.editor.session.TargetCommandError
import com.xiwei.sujian.feature.editor.session.TargetCommandResult
import com.xiwei.sujian.feature.editor.session.TargetDecorations
import com.xiwei.sujian.feature.editor.session.TargetSnapshot
import com.xiwei.sujian.feature.editor.session.TextEditorProfile
import com.xiwei.sujian.feature.editor.session.WindowBindingState
import com.xiwei.sujian.feature.editor.session.applyLocalEdit
import com.xiwei.sujian.feature.editor.session.applyProgrammaticReplace
import com.xiwei.sujian.feature.editor.session.applyUndoRestored
import com.xiwei.sujian.feature.editor.session.cancelActiveSession
import com.xiwei.sujian.feature.editor.session.closeTarget
import com.xiwei.sujian.feature.editor.session.commitActiveSession
import com.xiwei.sujian.feature.editor.session.completeWindowAttach
import com.xiwei.sujian.feature.editor.session.detachWindowBinding
import com.xiwei.sujian.feature.editor.session.prepareSessionForEdit
import com.xiwei.sujian.feature.editor.session.releaseHost
import com.xiwei.sujian.feature.editor.session.resetPersistentSession
import com.xiwei.sujian.feature.editor.ui.theme.EditorThemeAdapter
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow

/**
 * #592 一/四：窗口层宿主 — 每个 Activity/窗口创建一份，持有全部窗口/渲染对象：
 * EditableTextTarget（含 Compose/ViewModel 回调、Rect、Transform、mutableState）、
 * SujianEditorView（唯一视觉运行时）、TextPaint、Android 动画时间源、
 * WindowDisplayFrameClock、IME 与 View 回调、onTargetContentChanged。
 *
 * #595 九：窗口层不再创建 TargetDisplayRuntime 第二套动画运行时 — 非活动预览只使用
 * 会话层 snapshot 派生的纯静态 [ChapterPreviewState]，活动编辑只有 SujianEditorView
 * 持有动画 runtime 和 Bitmap 资源。
 *
 * 会话层 [EditorSessionCoordinator] 只保存纯会话状态；窗口销毁时调用
 * [releaseWindow] 释放 View/FrameClock 但保留 Rust 会话，
 * 新窗口从 coordinator 读取 session snapshot 并重新附着。
 *
 * 不得持有自建 CoroutineScope、Context（除 application context）、仓库、同步 I/O。
 */
class EditorWindowHost(
    private val context: Context,
    val sessionCoordinator: EditorSessionCoordinator,
    private val appServiceBridge: AppServiceBridge,
    private val animationTimeSource: com.xiwei.sujian.feature.editor.visual.AnimationTimeSource,
    private val transactionIdSource: com.xiwei.sujian.feature.editor.visual.TransactionIdSource,
    frameClock: WindowDisplayFrameClock? = null,
) : SessionCommandPort {
    /** #592 二：窗口标识 — 同一窗口内的 Compose onDispose 用它调用 detachWindowBinding。 */
    val windowId: String = "window:${System.identityHashCode(this)}"
    private fun setEditorScreen(view: android.view.View) {
        val holder = androidx.metrics.performance.PerformanceMetricsState.getHolderForHierarchy(view)
        holder?.state?.putState("screen", "Editor")
    }

    private fun setEditorInteraction(interaction: String) {
        sharedEditorView?.let { view ->
            val holder = androidx.metrics.performance.PerformanceMetricsState.getHolderForHierarchy(view)
            holder?.state?.putSingleFrameState("interaction", interaction)
        }
    }


    private val targets = mutableMapOf<String, EditableTextTarget>()

    private var sharedEditorView: SujianEditorView? = null
    val windowFrameClock: WindowDisplayFrameClock = frameClock ?: WindowDisplayFrameClock()

    // #595 三：pendingViewBind — beginEdit 在 AndroidView.factory 之前执行时，
    // View 尚未由 Compose 创建。将 session 绑定参数暂存，等 attachView（factory 内调用）
    // 在 Compose 提供的 Context 创建的 View 上执行绑定。避免 beginEdit 用 Host context
    // 创建的 View 与 AndroidView.factory 用 Compose context 创建的 View 不一致。
    private var pendingViewBind: PendingViewBind? = null

    private data class PendingViewBind(
        val targetId: String,
        val sessionId: ULong,
        val bridge: TextEditSessionBridge,
        val profile: TextEditorProfile,
        val snapshot: TargetSnapshot?,
        val typography: EditorTypography?,
    )

    // #595 一：窗口坐标追踪（activeTargetGeometry/activeTargetTransform）已随根壳
    // 覆盖层 AnimatedTextEditorSlot 一并删除。正文编辑器现在由 WritingEditorSurface
    // 在正文 Box 内直接持有 AndroidView，使用局部坐标，不再需要窗口级几何缓存。

    // ── Delegated session-level state ──
    // #595 三：窗口层只转发唯一 [sessionStateFlow]；activeTargetId / editingState /
    // windowBindingState 三个独立 stateIn 派生流已删除，Compose 消费者从同一个
    // EditorSessionState 快照读取，值 getter 供非 Compose 调用方读取当前值。

    val sessionStateFlow: StateFlow<EditorSessionState> get() = sessionCoordinator.sessionStateFlow
    val targetDecorationsVersionFlow: StateFlow<Long> get() = sessionCoordinator.targetDecorationsVersionFlow
    val lastCommittedTextFlow: StateFlow<String?> get() = sessionCoordinator.lastCommittedTextFlow

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

    /** #592 四：窗口层回调 — 非活动 target 命令执行后通知业务层。 */
    var onTargetContentChanged: ((targetId: String, newText: String) -> Unit)? = null

    // ── Target management (window-owned objects, pure metadata mirrored to session layer) ──

    fun registerTarget(target: EditableTextTarget) {
        targets[target.targetId] = target
        sessionCoordinator.registerTarget(target)
    }

    fun updateTargetSpec(
        targetId: String,
        onTextChanged: ((String) -> Unit)? = null,
        onCommit: ((String) -> Unit)? = null,
        onCancel: (() -> Unit)? = null,
        onEditingStateChanged: ((EditingState) -> Unit)? = null,
        profile: TextEditorProfile? = null,
    ) {
        val target = targets[targetId] ?: return
        onTextChanged?.let { target.onTextChanged = it }
        onCommit?.let { target.onCommit = it }
        onCancel?.let { target.onCancel = it }
        onEditingStateChanged?.let { target.onEditingStateChanged = it }
        profile?.let { target.updateProfile(it) }
        sessionCoordinator.updateTargetSpec(targetId, profile = profile)
    }

    fun detachWindowBinding(
        windowId: String,
        targetId: String,
    ) {
        val isPersistent = sessionCoordinator.isTargetPersistent(targetId)
        if (isPersistent) {
            saveActiveTargetProjection(targetId)
        }
        targets.remove(targetId)
        // #595 三：解绑必须清除 pendingViewBind — 否则下一个窗口的 View 会用
        // 已关闭/已释放的 session bridge 执行绑定。
        pendingViewBind = null
        sessionCoordinator.detachWindowBinding(windowId, targetId)
    }

    /**
     * #592 三：业务级关闭 — 由 workspace 导航事件调用（返回章节列表、切换章节、
     * 删除章节）。与窗口解绑分开：关闭销毁 Rust session，解绑只解除窗口引用。
     * 若当前 View 仍绑定该会话，先解除绑定避免 IME 输入命中已关闭的 session。
     */
    fun closeTarget(
        targetId: String,
        reason: SessionCloseReason,
    ) {
        if (activeTargetId == targetId) {
            sharedEditorView?.unbindSession("target_close")
        }
        targets.remove(targetId)
        // #595 三：业务关闭同样清除 pendingViewBind，防止 attachView 绑定已关闭的 session。
        pendingViewBind = null
        sessionCoordinator.closeTarget(targetId, reason)
    }

    fun updateTargetGeometry(
        targetId: String,
        geometry: Rect,
    ) {
        targets[targetId]?.updateGeometry(geometry)
    }

    fun updateTargetTransform(
        targetId: String,
        transform: Transform2D,
    ) {
        targets[targetId]?.updateTransform(transform)
    }

    fun updateTargetText(
        targetId: String,
        text: String,
    ) {
        // #595 四：窗口层只更新 target 对象（Compose 侧正文来源）；
        // 会话层正文唯一事实源是 sessionStateFlow（applyLocalEdit 已更新），
        // 不再存在 targetTexts 第二份正文缓存。
        targets[targetId]?.updateText(text)
    }

    fun getTargetGeometry(targetId: String): Rect? = targets[targetId]?.currentGeometry

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
     * #595 三/四/七：原子应用 [EditorMotionPolicy] — 唯一可写事实源。
     * 一次更新文字、光标、协同、时长和 reduce-motion；target profile 只作为约束
     * （SYSTEM_SUPPRESSED → forceStatic）在唯一计算点参与 effectivePolicy，
     * 不会成为第二个动画状态写入者。同步推送到活动 View。
     */
    fun applyMotionPolicy(policy: EditorMotionPolicy) {
        sessionCoordinator.applyMotionPolicy(policy)
        sharedEditorView?.let { view ->
            applyPolicyToView(view, activeTargetId)
            setEditorInteraction("motion_policy_change")
        }
    }

    /**
     * #595 四：唯一有效策略计算点 — globalPolicy.apply(profileConstraint).effective()。
     * profile 的 animationPolicy（SYSTEM_SUPPRESSED → forceStatic）只在此作为约束。
     */
    private fun effectivePolicyFor(targetId: String?): EditorMotionPolicy {
        val global = sessionCoordinator.getMotionPolicy()
        val constraint = targetId?.let { profileConstraintFor(it) } ?: TargetMotionConstraint()
        return constraint.apply(global).effective()
    }

    private fun profileConstraintFor(targetId: String): TargetMotionConstraint {
        val profile = targets[targetId]?.profile ?: return TargetMotionConstraint()
        return constraintFor(profile)
    }

    private fun applyPolicyToView(
        view: SujianEditorView,
        targetId: String?,
    ) {
        val effective = effectivePolicyFor(targetId)
        view.setTypingAnimationEnabled(effective.textEnabled, effective.textDurationMillis)
        view.setSmoothCursorEnabled(effective.cursorEnabled, effective.cursorDurationMillis)
        view.setCoordinatedAnimationEnabled(effective.coordinated)
        view.setReduceMotion(effective.reduceMotion)
        view.setKernelAnimationEnabled(effective.textEnabled || effective.cursorEnabled)
    }

    /**
     * #595 三：动画策略 StateFlow — 只读可观察的单一事实源（不可变）。
     * UI 生命周期感知地收集该值，[applyMotionPolicy] 原子更新全部字段。
     */
    val motionPolicyFlow: kotlinx.coroutines.flow.StateFlow<EditorMotionPolicy>
        get() = sessionCoordinator.motionPolicyFlow

    /**
     * #624 评论3/4：排版设置持续应用到当前共享 [SujianEditorView] — 字号、行距、
     * 首行缩进（开关 + 字符宽度）。设置变化后当前正文立即重排，不重建编辑 session。
     * 与 [applyMotionPolicy] 同为窗口层唯一设置写入点。
     *
     * 最近一次排版设置缓存在窗口层（与 motion policy 存于会话层同源模式）；
     * 首次/换绑由 [performViewBind] 使用 [PendingViewBind.typography] 显式写入，
     * 运行时改字号/行距由本方法更新当前活动 View 并刷新 lastTypography。
     */
    fun applyEditorTypography(
        fontSizeSp: Float,
        lineSpacingMultiplier: Float,
        autoIndentEnabled: Boolean,
        autoIndentWidth: Float,
    ) {
        lastTypography =
            EditorTypography(
                fontSizeSp = fontSizeSp,
                lineSpacingMultiplier = lineSpacingMultiplier,
                autoIndentEnabled = autoIndentEnabled,
                autoIndentWidth = autoIndentWidth,
            )
        sharedEditorView?.let { view -> applyTypographyToView(view, lastTypography!!) }
        setEditorInteraction("typography_change")
    }

    private fun applyTypographyToView(
        view: SujianEditorView,
        typography: EditorTypography,
    ) {
        // #624 评论3：一个原子入口一次更新 TextPaint / runtime config，
        // 最后只推进一次布局 — 不再连续调用三个各自 updateLayoutConfig 的 setter。
        view.applyLayoutConfig(
            typography.fontSizeSp,
            typography.lineSpacingMultiplier,
            typography.autoIndentEnabled,
            typography.autoIndentWidth,
        )
    }

    /**
     * #624 评论5：导航离开正文前先立刻收 IME — 只转发给当前真实 shared editor view。
     */
    fun dismissImeForNavigation() {
        sharedEditorView?.dismissImeForNavigation()
    }

    /**
     * #625 评论项3：撤销 — 只转发给当前真实 [sharedEditorView]。
     *
     * 继续走现有 View → Pipeline → session 编辑链（dirty/保存/字数/动画同一份状态），
     * 不重建 TextEditSessionBridge，不直接跨 UniFFI 调 undo/redo。
     */
    fun performUndo() {
        sharedEditorView?.performUndo()
    }

    /**
     * #625 评论项3：重做 — 只转发给当前真实 [sharedEditorView]。
     *
     * 同 [performUndo]，继续走现有 View → Pipeline → session 编辑链。
     */
    fun performRedo() {
        sharedEditorView?.performRedo()
    }

    private var lastTypography: EditorTypography? = null

    // ── Edit operations (orchestrates session + window) ──

    fun beginEdit(
        targetId: String,
        initialSelection: Int? = null,
        typography: EditorTypography? = null,
    ): Boolean {
        // #592 三：业务已关闭（closeTarget）或未注册的 target 拒绝重新绑定 —
        // 防止导航离开正文的过渡期间 beginEdit 重新触发并复活已关闭的 session。
        if (targets[targetId] == null) return false
        val currentActiveId = sessionCoordinator.activeTargetId
        if (currentActiveId != null && currentActiveId != targetId) {
            // 重绑定到不同 target：先把旧活动目标的滚动/视口状态存入会话层纯数据快照。
            saveActiveTargetProjection(currentActiveId)
        }
        clearActiveCallbacks()
        // 重绑定到不同 target：通知旧 target 回调 REBINDING（窗口层回调归属）。
        val oldActiveId = sessionCoordinator.activeTargetId
        if (oldActiveId != null && oldActiveId != targetId) {
            targets[oldActiveId]?.onEditingStateChanged?.invoke(EditingState.REBINDING)
        }
        val target = targets[targetId] ?: return false
        // #595 四：新建 session 的初始正文来自窗口层 target（Compose 唯一正文来源），
        // 会话层不再维护 targetTexts 第二份正文缓存。
        val bindInfo =
            sessionCoordinator.prepareSessionForEdit(
                targetId, target.currentText, initialSelection, windowId,
            ) ?: return false

        target.onEditingStateChanged?.invoke(EditingState.BINDING)

        val bridge = TextEditSessionBridge(appServiceBridge, bindInfo.sessionId)
        val pending =
            PendingViewBind(
                targetId = targetId,
                sessionId = bindInfo.sessionId,
                bridge = bridge,
                profile = bindInfo.profile,
                snapshot = bindInfo.snapshot,
                typography = typography ?: lastTypography,
            )

        // #595 三：如果 AndroidView.factory 已创建 View（重新绑定场景），直接在现有
        // View 上执行 session 绑定并进入 Attached；否则只存 pendingViewBind，
        // 状态保持 Attaching（prepareSessionForEdit 已设置），等 attachView 在
        // Compose 提供的 Context 创建的 View 上完成真实绑定后才 completeWindowAttach。
        // 禁止在 View 尚未创建/绑定时提前进入 Attached。
        val view = sharedEditorView
        if (view != null) {
            if (performViewBind(view, pending, target) &&
                sessionCoordinator.completeWindowAttach(windowId, targetId, bindInfo.sessionId)
            ) {
                // #624 评论17 问题2：只有 completeWindowAttach==true（binding 仍是本次
                // Attaching）才通知 EDITING。旧 View 晚到直接丢弃。
                // #630 C块：completeWindowAttach 只表达 session/view 绑定完成，
                // 不自动激活输入。activateInput 只从明确用户手势（如 handleTap）进入。
                target.onEditingStateChanged?.invoke(EditingState.EDITING)
            } else {
                // 绑定失败或 binding 已不属于本次 attach：回到 Detached/Idle，
                // 不能留下没有 View 绑定的 Attached 状态。
                pendingViewBind = null
                sessionCoordinator.detachWindowBinding(windowId, targetId)
                return false
            }
        } else {
            pendingViewBind = pending
        }

        return true
    }

    /**
     * #595 四：根据 target profile 的 animationPolicy 计算有效动画约束。
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

    // #595 三：在 View 上执行 session 绑定 — 由 beginEdit（View 已存在）或
    // attachView（AndroidView.factory 刚创建 View）调用。返回 false 表示绑定失败
    // （无真实 snapshot），调用方必须回到 Detached/Idle，
    // 不得进入没有真实 View 绑定的 Attached。
    // #592 一：复用既有持久 session 时执行 attachSnapshot（不调用
    // textEditSessionLoadText，Rust revision/Undo/Redo/composition 保持）；
    // #595 二：新建 session 同样 attachSnapshot（createSession 已把初始正文装入
    // kernel，是唯一一次 Core 命令）。
    // #595 三：snapshot 缺失（create 后读不到）视为绑定失败 — 禁止回退到
    // bindSession/loadText 对同一 session 执行第二次 Core loadText。
    private fun performViewBind(
        view: SujianEditorView,
        pending: PendingViewBind,
        target: EditableTextTarget,
    ): Boolean {
        val snapshot =
            pending.snapshot
                ?: run {
                    Log.w(
                        TAG,
                        "performViewBind(${pending.targetId}): no real kernel snapshot — refusing bind " +
                            "to avoid a second Core loadText on the same session",
                    )
                    return false
                }
        // #595 二：窗口绑定成功时签发输入 lease — 之后每次 onLocalEdit/
        // onExternalEdit/onContentChanged 提交都携带；章节切换提交/业务关闭/
        // 窗口解绑会使 epoch 失效，旧 View 晚到一帧的输入被会话层拒绝。
        val lease = sessionCoordinator.currentInputLease()
        if (lease == null) {
            Log.w(TAG, "performViewBind(${pending.targetId}): no active input lease — refusing bind")
            return false
        }
        // #630 评论 5326175206 项3: 先配置 TextPaint/lineSpacing/indent，再 attach snapshot，
        // 只生成一次正确排版。不要先 attach 默认排版再 applyLayoutConfig 二次重排。
        pending.typography?.let { applyTypographyToView(view, it) }
        view.attachSession(
            sessionBridge = pending.bridge,
            profile = pending.profile,
            text = snapshot.text,
            revision = snapshot.revision,
            cursorUtf8 = snapshot.cursorUtf8,
            selStartUtf8 = snapshot.selectionAnchorUtf8,
            selEndUtf8 = snapshot.selectionHeadUtf8,
        )
        restoreProjectionScroll(view, pending.targetId)

        // #595 七: 活动编辑时 SujianEditorView 是唯一的 FrameClock listener。
        // 非活动预览只使用会话层 snapshot 派生的纯静态 ChapterPreviewState。

        installContentCallback(view, target, lease)
        installCommitRequestedCallback(view)
        installCancelRequestedCallback(view)
        // #595 四：绑定完成后应用 target 约束后的有效动画策略 — 全局策略 + profile 约束
        // 在唯一计算点（effectivePolicyFor）合成，一次传入 View/引擎/Rust kernel。
        applyPolicyToView(view, pending.targetId)
        return true
    }

    /**
     * #592 三：重新附着窗口后恢复投影保存的滚动位置（配置变化/返回重进时）。
     * 会话层保存的是纯数据 scrollX/scrollY，这里应用到当前窗口的 View。
     */
    private fun restoreProjectionScroll(
        view: SujianEditorView,
        targetId: String,
    ) {
        val snapshot = sessionCoordinator.getProjectionSnapshot(targetId) ?: return
        val anchor = snapshot.viewportAnchor
        if (anchor != null) {
            // 优先使用逻辑锚点恢复：从文本偏移 + 行内像素推导滚动位置
            val pipeline = view.getEditorPipeline()
            val line = pipeline.getLayoutLineForOffset(anchor.textOffsetUtf16)
            val lineTop = pipeline.getLayoutLineTop(line).toFloat()
            // 锚点位置的主水平坐标 = 行首 + 行内偏移，直接用它作为 scrollX
            val anchorPrimaryHorizontal = pipeline.getLayoutPrimaryHorizontal(anchor.textOffsetUtf16)
            val scrollX = anchorPrimaryHorizontal
            val scrollY = lineTop
            // 写入 interaction 用于 JankStats
            setEditorInteraction("viewport_restore")
            view.setScrollPosition(scrollX, scrollY)
        } else {
            // 兼容旧快照：回退到绝对像素
            view.setScrollPosition(snapshot.scrollX, snapshot.scrollY)
        }
    }

    fun commitActiveEdit(): Boolean {
        clearActiveCallbacks()
        val targetId = activeTargetId ?: return false
        val isPersistent = sessionCoordinator.isTargetPersistent(targetId)
        val view = sharedEditorView
        var finalText: String? = null
        if (view != null) {
            finalText = view.getText()
            if (isPersistent) {
                view.softResetForPersistentCommit()
            } else {
                view.unbindSession("commit")
            }
        }
        val target = targets[targetId]
        return sessionCoordinator.commitActiveSession(finalText).also { success ->
            if (success) {
                target?.onEditingStateChanged?.invoke(EditingState.IDLE)
            }
        }
    }

    fun cancelActiveEdit(): Boolean {
        clearActiveCallbacks()
        sharedEditorView?.unbindSession("cancel")
        val targetId = activeTargetId
        val target = targetId?.let { targets[it] }
        return sessionCoordinator.cancelActiveSession().also { success ->
            if (success) {
                target?.onEditingStateChanged?.invoke(EditingState.IDLE)
            }
        }
    }

    fun resetPersistentSession(
        targetId: String,
        text: String,
        cursorUtf8: Int,
        source: SessionResetSource = SessionResetSource.EXTERNAL,
    ): ExternalResetResult {
        // #595 二/五：Core 侧只允许一次 reset 命令（textEditSessionReset / createSession）。
        // 完成后 View 不得再次 loadText（那是第二次 Core 命令，会 revision+1、
        // 重复清空 Undo/Redo/composition）— 只从真实 snapshot attach 到本地镜像。
        // #595 五：返回可提交事务结果 — reset 失败时返回 Failed，调用方不得推进 UI/会话事实。
        val result = sessionCoordinator.resetPersistentSession(targetId, text, cursorUtf8, source)
        if (result is ExternalResetResult.Success && targetId == activeTargetId) {
            val view = sharedEditorView
            if (view != null) {
                attachSnapshotToView(targetId, view)
            }
        }
        // 非活动预览直接读取会话层 snapshot（getChapterPreviewState），无需重建投影运行时。
        return result
    }

    /**
     * #595 二：把真实 Rust snapshot（reset 后的 text/revision/cursor/selection）
     * 只装入 Android mirror/layout — 不调用 textEditSessionLoadText。
     * 使用 attachSnapshotSameSession：同一 session 不解除绑定，
     * 不清回调、不隐藏 IME、不丢焦点（外部替换可能发生在输入过程中）。
     */
    private fun attachSnapshotToView(
        targetId: String,
        view: SujianEditorView,
    ) {
        val snapshot = sessionCoordinator.queryTargetSnapshot(targetId) ?: return
        val sessionId = sessionCoordinator.getPersistentSessionId(targetId) ?: return
        val profile = targets[targetId]?.profile ?: TextEditorProfile()
        val bridge = TextEditSessionBridge(appServiceBridge, sessionId)
        view.attachSnapshotSameSession(
            sessionBridge = bridge,
            profile = profile,
            text = snapshot.text,
            revision = snapshot.revision,
            cursorUtf8 = snapshot.cursorUtf8,
            selStartUtf8 = snapshot.selectionAnchorUtf8,
            selEndUtf8 = snapshot.selectionHeadUtf8,
        )
        applyPolicyToView(view, targetId)
    }

    // ── SessionCommandPort (view pipeline when active, projection for inactive) ──

    override fun queryTargetSnapshot(targetId: String): TargetSnapshot? =
        sessionCoordinator.queryTargetSnapshot(
            targetId,
        )

    override fun applyTargetCommand(
        targetId: String,
        command: TargetCommand,
    ): TargetCommandResult {
        if (targetId == activeTargetId) {
            val view = sharedEditorView
            if (view != null) {
                val snapshotBefore =
                    sessionCoordinator.queryTargetSnapshot(targetId)
                        ?: return TargetCommandResult.Failed(TargetCommandError.SNAPSHOT_UNAVAILABLE)
                val commandPort = view.getPipeline()
                when (command) {
                    is TargetCommand.Replace -> {
                        // #595 四：程序化替换命令显式携带 PROGRAMMATIC 来源 —
                        // 输出天然携带来源，不再依赖 View 侧可变标记。
                        val pipelineOutput =
                            commandPort.replaceRangeTyped(
                                command.byteStart,
                                command.byteEndExclusive,
                                command.replacementText,
                                command.originalText,
                                uniffi.writer_core.EditorTransactionCauseDto.PROGRAMMATIC,
                                source = com.xiwei.sujian.feature.editor.platform.EditorEditSource.PROGRAMMATIC,
                            )
                        view.handlePipelineOutput(pipelineOutput)
                    }
                    is TargetCommand.ReplaceAll -> {
                        view.replaceAll(command.searchText, command.replacementText)
                    }
                    is TargetCommand.SetSelection -> {
                        val pipelineOutput = commandPort.setSelectionTyped(command.anchorUtf8, command.headUtf8)
                        view.handlePipelineOutput(pipelineOutput)
                    }
                }
                val snapshotAfter =
                    sessionCoordinator.queryTargetSnapshot(targetId)
                        ?: return TargetCommandResult.Failed(TargetCommandError.SNAPSHOT_UNAVAILABLE)
                targets[targetId]?.updateText(snapshotAfter.text)
                return TargetCommandResult.Success(snapshotAfter)
            }
        }
        // 非活动目标：只执行 bridge 命令，预览直接读取会话层 snapshot（getChapterPreviewState）
        val result = sessionCoordinator.executeTargetCommand(targetId, command)
        if (result is TargetCommandResult.Success && targetId != activeTargetId) {
            onTargetContentChanged?.invoke(targetId, result.snapshot.text)
        }
        return result
    }

    override fun setTargetDecorations(
        targetId: String,
        decorations: TargetDecorations,
    ) {
        sessionCoordinator.setTargetDecorations(targetId, decorations)
        if (targetId == activeTargetId) {
            val view = sharedEditorView
            if (view != null) {
                if (decorations.searchHighlightsUtf8.isNotEmpty()) {
                    view.setSearchHighlights(decorations.searchHighlightsUtf8)
                } else {
                    view.clearSearchHighlights()
                }
                if (decorations.selectionStartUtf8 >= 0 && decorations.selectionEndUtf8 >= 0) {
                    view.setSelectionRange(decorations.selectionStartUtf8, decorations.selectionEndUtf8)
                }
            }
        }
    }

    // ── View management ──

    /**
     * #595 三：在 AndroidView.factory 中用传入的 Context 创建 View —
     * 不返回宿主提前创建、长期缓存的 View。Compose 官方模型要求 factory 创建 View。
     */
    fun createWindowView(context: Context): SujianEditorView {
        return SujianEditorView(
            context,
            animationTimeSource = animationTimeSource,
            transactionIdSource = transactionIdSource,
        ).also {
                view ->
            view.setFrameClock(windowFrameClock)
            // #630 C块：createWindowView 不再 applyPolicyToView — 真实 target 还没完成 bind，
            // 真正的策略只在 performViewBind() 根据 pending.targetId 应用一次。
            // 后续全局设置变化只走 applyMotionPolicy() 更新活动 View。
            // #630 评论 5327560790: 删除 lastTypography 预写 — 首次 bind 已有显式
            // pending.typography（performViewBind 先写排版参数再 attach snapshot），
            // createWindowView 不应再抢先给空 View 套一次缓存排版。
            sharedEditorView = view
            setEditorScreen(view)
        }
    }

    /**
     * #595 三：AndroidView.factory 创建 View 后立即附着到窗口 —
     * 设置 sharedEditorView 引用，并处理 beginEdit 留下的 pending session 绑定。
     *
     * 只有 performViewBind 成功后（真实 View 已绑定 session）才 completeWindowAttach：
     * Attached 必须表示屏幕上的 View 已绑定；绑定失败回到 Detached/Idle。
     */
    fun attachView(
        windowId: String,
        targetId: String,
        view: SujianEditorView,
    ) {
        sharedEditorView = view
        // #595 三：AndroidView.factory 创建 View 后，检查 beginEdit 留下的 pending
        // session 绑定。在 Compose 提供的 Context 创建的 View 上执行绑定，
        // 确保 session 绑定在显示的 View 上而非被丢弃的临时 View 上。
        val pending = pendingViewBind
        if (pending != null && pending.targetId == targetId) {
            pendingViewBind = null
            val target = targets[targetId]
            if (target != null) {
                if (performViewBind(view, pending, target) &&
                    sessionCoordinator.completeWindowAttach(windowId, targetId, pending.sessionId)
                ) {
                    // #624 评论17 问题2：只有 completeWindowAttach==true 才通知 EDITING。
                    // 旧 View 晚到直接丢弃，不得表现成绑定成功。
                    // #630 C块：completeWindowAttach 只表达 session/view 绑定完成，
                    // 不自动激活输入。activateInput 只从明确用户手势（如 handleTap）进入。
                    target.onEditingStateChanged?.invoke(EditingState.EDITING)
                } else {
                    // 绑定失败或 binding 已不属于本次 attach：回到 Detached/Idle，
                    // 不留下没有 View 绑定的 Attached 状态。
                    sessionCoordinator.detachWindowBinding(windowId, targetId)
                }
            } else {
                // target 已被业务关闭（closeTarget）— 丢弃 pending，不绑定。
                sessionCoordinator.detachWindowBinding(windowId, targetId)
            }
        }
    }

    /**
     * #595 三：AndroidView.onRelease — View 已离开 Composition 且不会再被 Compose 使用。
     * 解除双向引用、InputConnection、FrameClock 和 callback，不能只设置 GONE。
     */
    fun detachView(
        windowId: String,
        targetId: String,
        view: SujianEditorView,
    ) {
        // #623 评论 2：只允许当前 sharedEditorView === view 的 View 执行解绑，
        // 旧 View 的晚到 onRelease 不能动新 View。
        if (sharedEditorView !== view) return
        // 解绑前保存当前滚动投影。
        saveActiveTargetProjection(targetId)
        clearActiveCallbacks()
        view.unbindSession("compose_release")
        view.setFrameClock(null)
        // 调用会话层的窗口解绑，把当前绑定明确推进到 Detached。
        // 持久正文 session 只解除窗口绑定，不关闭 Rust session。
        sessionCoordinator.detachWindowBinding(windowId, targetId)
        sharedEditorView = null
    }

    /**
     * #595 三：AndroidView.update — 应用主题和几何更新到 View。
     */
    fun updateView(
        view: SujianEditorView,
        themeColors: com.xiwei.sujian.feature.editor.ui.theme.EditorThemeColors,
    ) {
        EditorThemeAdapter.applyToView(view, themeColors)
        view.visibility = android.view.View.VISIBLE
        if (view.width > 0 && view.height > 0) {
            updateHostGeometry(view.width.toFloat(), view.height.toFloat())
        }
    }

    fun updateHostGeometry(
        width: Float,
        height: Float,
    ) {
        sharedEditorView?.updateHostGeometry(width, height)
    }

    // ── Window lifecycle ──

    /**
     * #592 二：窗口销毁时完整释放 View/FrameClock/投影运行时，但保留 Rust 会话。
     * 窗口状态由会话层窗口绑定状态机统一维护，新窗口创建后通过
     * [beginEdit] 的 attach 路径自动附着旧 session。
     */
    fun releaseWindow() {
        clearActiveCallbacks()
        val activeId = activeTargetId
        if (activeId != null) {
            // #592 三：窗口销毁前把滚动/选区/装饰等状态存入会话层纯数据快照
            saveActiveTargetProjection(activeId)
        }
        sharedEditorView?.let { view ->
            view.unbindSession("config_change")
            view.setFrameClock(null)
            view.release()
        }
        sharedEditorView = null
        // #623 评论5：窗口销毁必须把会话层绑定明确推进到 Detached —
        // 否则新窗口（新 windowId）看到残留的 Attached/Attaching 会误判
        // "已附着"而跳过 beginEdit，新建的 AndroidView 没有 pendingViewBind
        // 可消费，得到 isSessionBound=false 的编辑器 View。
        // detachWindowBinding 内部校验 windowId+targetId：若绑定已属于更新的
        // 窗口/目标则 no-op；持久 Rust session 只解除窗口绑定，不关闭。
        // 与 detachView 一致清除 pendingViewBind，避免残留的 session 绑定参数
        // 被后续复用。
        pendingViewBind = null
        if (activeId != null) {
            sessionCoordinator.detachWindowBinding(windowId, activeId)
        }
        windowFrameClock.release()
    }

    /**
     * Activity 永久结束 — 释放窗口和全部会话。
     */
    fun releaseHost() {
        clearActiveCallbacks()
        if (activeTargetId != null) {
            sharedEditorView?.unbindSession("release")
        }
        sharedEditorView?.let { view ->
            view.setFrameClock(null)
            view.release()
        }
        sharedEditorView = null
        windowFrameClock.release()
        sessionCoordinator.releaseHost()
    }

    // ── Private helpers ──

    private fun installContentCallback(
        view: SujianEditorView,
        target: EditableTextTarget,
        lease: EditorInputLease,
    ) {
        // #624 评论9：onContentChanged 已删除 — 热路径不再传整章 String。
        // #595 一/二：类型化本地编辑回调 — 先更新会话层唯一 SessionState（revision/transactionId），
        // 再通知 ViewModel 保存。ViewModel 不再靠字符串比较猜测来源，
        // WritingPane 收集 sessionStateFlow 发现 revision 已应用，不触发 reset。
        view.onLocalEdit = localEdit@{ event ->
            // lease 校验 — 章节切换提交后旧 View 晚到事件不能进入新章节会话。
            if (!sessionCoordinator.isInputLeaseCurrent(lease, target.targetId)) return@localEdit
            sessionCoordinator.applyLocalEdit(
                EditorDocumentUpdate.LocalInput(
                    targetId = target.targetId,
                    revision = event.revision,
                    transactionId = event.transactionId,
                    operationKind = event.operationKind,
                    selectionAnchorUtf8 = event.selectionAnchorUtf8,
                    selectionHeadUtf8 = event.selectionHeadUtf8,
                    lease = lease,
                    contentChanged = event.contentChanged,
                    contentDelta = event.contentDelta,
                ),
            )
            // #624 评论9：ViewModel 增量处理（保存调度/统计/字数）— 不传整章 String。
            target.onEditorApplied?.invoke(event)
        }
        // #595 二/四：类型化外部编辑回调 — 撤销/恢复/程序化替换走类型化事件，
        // 来源由 PipelineOutput 天然携带，由 Coordinator 的 apply* 统一处理。
        view.onExternalEdit = externalEdit@{ event ->
            if (!sessionCoordinator.isInputLeaseCurrent(lease, target.targetId)) return@externalEdit
            when (event.source) {
                com.xiwei.sujian.feature.editor.platform.EditorEditSource.UNDO,
                com.xiwei.sujian.feature.editor.platform.EditorEditSource.REDO,
                -> {
                    sessionCoordinator.applyUndoRestored(
                        EditorDocumentUpdate.UndoRestored(
                            targetId = target.targetId,
                            snapshotId = event.transactionId,
                            revision = event.revision,
                            transactionId = event.transactionId,
                            selectionAnchorUtf8 = event.selectionAnchorUtf8,
                            selectionHeadUtf8 = event.selectionHeadUtf8,
                            lease = lease,
                            contentChanged = event.contentChanged,
                            contentDelta = event.contentDelta,
                        ),
                    )
                }
                com.xiwei.sujian.feature.editor.platform.EditorEditSource.PROGRAMMATIC -> {
                    sessionCoordinator.applyProgrammaticReplace(
                        EditorDocumentUpdate.ProgrammaticReplace(
                            targetId = target.targetId,
                            commandId = event.transactionId,
                            revision = event.revision,
                            transactionId = event.transactionId,
                            selectionAnchorUtf8 = event.selectionAnchorUtf8,
                            selectionHeadUtf8 = event.selectionHeadUtf8,
                            lease = lease,
                            contentChanged = event.contentChanged,
                            contentDelta = event.contentDelta,
                        ),
                    )
                }
                com.xiwei.sujian.feature.editor.platform.EditorEditSource.NORMAL -> {
                    // NORMAL 不应走 onExternalEdit，回退到 applyLocalEdit
                    sessionCoordinator.applyLocalEdit(
                        EditorDocumentUpdate.LocalInput(
                            targetId = target.targetId,
                            revision = event.revision,
                            transactionId = event.transactionId,
                            operationKind = event.operationKind,
                            selectionAnchorUtf8 = event.selectionAnchorUtf8,
                            selectionHeadUtf8 = event.selectionHeadUtf8,
                            lease = lease,
                            contentChanged = event.contentChanged,
                            contentDelta = event.contentDelta,
                        ),
                    )
                }
            }
            target.onEditorApplied?.invoke(event)
        }
        view.onSearchHighlightsCleared = {
            sessionCoordinator.setTargetDecorations(target.targetId, TargetDecorations())
        }
    }

    private fun installCommitRequestedCallback(view: SujianEditorView) {
        view.onCommitRequested = {
            commitActiveEdit()
        }
    }

    private fun installCancelRequestedCallback(view: SujianEditorView) {
        view.onCancelRequested = {
            cancelActiveEdit()
        }
    }

    private fun clearActiveCallbacks() {
        sharedEditorView?.let { view ->
            view.onLocalEdit = null
            view.onExternalEdit = null
            view.onCommitRequested = null
            view.onCancelRequested = null
            view.onSearchHighlightsCleared = null
        }
    }

    /**
     * #592 三：把窗口滚动位置保存为会话层纯数据快照。
     * 不依赖 target 对象存在（窗口销毁时序晚于 Compose onDispose），
     * 只依赖 View。
     */
    private fun saveActiveTargetProjection(targetId: String) {
        val view = sharedEditorView ?: return
        val pipeline = view.getEditorPipeline()
        val scrollX = view.getScrollXPos()
        val scrollY = view.getScrollYPos()
        // 计算视口锚点：从当前滚动位置推导逻辑锚点（文本偏移 + 行内像素）
        val anchorLine = pipeline.getLayoutLineForVertical(scrollY.toInt())
        val anchorOffset = pipeline.getLayoutOffsetForHorizontal(anchorLine, scrollX)
        // 行首偏移 = 该行起始位置的偏移量
        val lineStartOffset = pipeline.getLayoutOffsetForHorizontal(anchorLine, 0f)
        // 行首水平位置
        val lineLeft = pipeline.getLayoutPrimaryHorizontal(lineStartOffset)
        val offsetWithinLinePx = (scrollX - lineLeft).coerceAtLeast(0f)
        val viewportAnchor = ViewportAnchor(
            textOffsetUtf16 = anchorOffset,
            offsetWithinLinePx = offsetWithinLinePx.toInt(),
        )
        // 写入 interaction 用于 JankStats
        setEditorInteraction("viewport_save")
        sessionCoordinator.saveProjectionSnapshot(
            targetId,
            ProjectionSnapshot(
                scrollX = scrollX,
                scrollY = scrollY,
                viewportAnchor = viewportAnchor,
            ),
        )
    }
}

/**
 * #630 评论 5326175206 项3: 排版快照 — 首次正文 attach 时必须显式携带，
 * 不再靠两个 LaunchedEffect 竞态猜测谁先到。
 */
data class EditorTypography(
    val fontSizeSp: Float,
    val lineSpacingMultiplier: Float,
    val autoIndentEnabled: Boolean,
    val autoIndentWidth: Float,
)
