package com.xiwei.sujian.editor.v2.coordinator

import android.content.Context
import android.graphics.Rect
import com.xiwei.sujian.data.AppServiceBridge
import com.xiwei.sujian.data.BridgeResult
import com.xiwei.sujian.editor.v2.host.SujianEditorView
import com.xiwei.sujian.editor.v2.host.TextEditSessionBridge
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import com.xiwei.sujian.editor.v2.projection.TargetDisplayRuntime
import com.xiwei.sujian.editor.v2.host.EditorAttachmentState
import com.xiwei.sujian.editor.v2.host.EditorFrameSnapshot
import com.xiwei.sujian.editor.v2.host.attachmentStateFromBinding
import com.xiwei.sujian.editor.v2.motion.EditorMotionPolicy
import kotlinx.coroutines.flow.StateFlow

/**
 * #592 一/四：窗口层宿主 — 每个 Activity/窗口创建一份，持有全部窗口/渲染对象：
 * EditableTextTarget（含 Compose/ViewModel 回调、Rect、Transform、mutableState）、
 * TargetDisplayRuntime、TextPaint、Android 动画时间源、WindowDisplayFrameClock、
 * IME 与 View 回调、onTargetContentChanged。
 *
 * 会话层 [EditorSessionCoordinator] 只保存纯会话状态；窗口销毁时调用
 * [releaseWindow] 释放 View/FrameClock/投影运行时但保留 Rust 会话，
 * 新窗口从 coordinator 读取 session snapshot 并重新附着。
 *
 * 不得持有自建 CoroutineScope、Context（除 application context）、仓库、同步 I/O。
 */
class EditorWindowHost(
    private val context: Context,
    val sessionCoordinator: EditorSessionCoordinator,
    private val appServiceBridge: AppServiceBridge,
    private val animationTimeSource: com.xiwei.sujian.editor.v2.visual.AnimationTimeSource,
    private val transactionIdSource: com.xiwei.sujian.editor.v2.visual.TransactionIdSource,
    frameClock: WindowDisplayFrameClock? = null,
) : SessionCommandPort {

    /** #592 二：窗口标识 — 同一窗口内的 Compose onDispose 用它调用 detachWindowBinding。 */
    val windowId: String = "window:${System.identityHashCode(this)}"

    private val targets = mutableMapOf<String, EditableTextTarget>()
    private val targetProjections = mutableMapOf<String, TargetDisplayRuntime>()

    private var sharedEditorView: SujianEditorView? = null
    val windowFrameClock: WindowDisplayFrameClock = frameClock ?: WindowDisplayFrameClock()

    // #595 一：窗口坐标追踪（activeTargetGeometry/activeTargetTransform）已随根壳
    // 覆盖层 AnimatedTextEditorSlot 一并删除。正文编辑器现在由 WritingEditorSurface
    // 在正文 Box 内直接持有 AndroidView，使用局部坐标，不再需要窗口级几何缓存。

    // ── Delegated session-level state ──
    // #595 二：会话层用 StateFlow 暴露，窗口层转发给 Compose 消费者用 collectAsState()。
    // 值 getter 供非 Compose 调用方读取当前值。

    val activeTargetIdFlow: StateFlow<String?> get() = sessionCoordinator.activeTargetIdFlow
    val editingStateFlow: StateFlow<EditingState> get() = sessionCoordinator.editingStateFlow
    val windowBindingStateFlow: StateFlow<WindowBindingState> get() = sessionCoordinator.windowBindingStateFlow
    val targetDecorationsVersionFlow: StateFlow<Long> get() = sessionCoordinator.targetDecorationsVersionFlow
    val lastCommittedTextFlow: StateFlow<String?> get() = sessionCoordinator.lastCommittedTextFlow

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
        currentText: String? = null
    ) {
        val target = targets[targetId] ?: return
        onTextChanged?.let { target.onTextChanged = it }
        onCommit?.let { target.onCommit = it }
        onCancel?.let { target.onCancel = it }
        onEditingStateChanged?.let { target.onEditingStateChanged = it }
        profile?.let { target.updateProfile(it) }
        currentText?.let { target.updateText(it) }
        sessionCoordinator.updateTargetSpec(targetId, profile = profile, currentText = currentText)
    }

    fun detachWindowBinding(windowId: String, targetId: String) {
        val isPersistent = sessionCoordinator.isTargetPersistent(targetId)
        if (isPersistent) {
            saveActiveTargetProjection(targetId)
            targetProjections.remove(targetId)?.release()
        }
        targets.remove(targetId)
        sessionCoordinator.detachWindowBinding(windowId, targetId)
    }

    /**
     * #592 三：业务级关闭 — 由 workspace 导航事件调用（返回章节列表、切换章节、
     * 删除章节）。与窗口解绑分开：关闭销毁 Rust session，解绑只解除窗口引用。
     * 若当前 View 仍绑定该会话，先解除绑定避免 IME 输入命中已关闭的 session。
     */
    fun closeTarget(targetId: String, reason: SessionCloseReason) {
        if (activeTargetId == targetId) {
            sharedEditorView?.let { view ->
                view.unbindSession("target_close")
            }
        }
        targetProjections.remove(targetId)?.release()
        targets.remove(targetId)
        sessionCoordinator.closeTarget(targetId, reason)
    }

    fun updateTargetGeometry(targetId: String, geometry: Rect) {
        targets[targetId]?.updateGeometry(geometry)
    }

    fun updateTargetTransform(targetId: String, transform: Transform2D) {
        targets[targetId]?.updateTransform(transform)
    }

    fun updateTargetText(targetId: String, text: String) {
        targets[targetId]?.updateText(text)
        sessionCoordinator.updateTargetText(targetId, text)
    }

    fun getTargetGeometry(targetId: String): Rect? = targets[targetId]?.currentGeometry

    fun getTargetText(targetId: String): String? = sessionCoordinator.getTargetText(targetId)

    fun getPersistentSessionId(targetId: String): ULong? = sessionCoordinator.getPersistentSessionId(targetId)

    fun getTargetProjection(targetId: String): TargetDisplayRuntime? = targetProjections[targetId]

    fun getEditorAnimationSettings(): EditorAnimationSettings = sessionCoordinator.getEditorAnimationSettings()

    fun setEditorAnimationSettings(settings: EditorAnimationSettings) {
        sessionCoordinator.setEditorAnimationSettings(settings)
        sharedEditorView?.let { view ->
            view.setTypingAnimationEnabled(settings.typingAnimationEnabled, settings.typingAnimationDurationMs)
            view.setSmoothCursorEnabled(settings.smoothCursorEnabled, settings.smoothCursorDurationMs)
            view.setCoordinatedAnimationEnabled(settings.coordinated)
            view.setReduceMotion(settings.reduceMotion)
            // #595 四: kernel animation_enabled = text OR cursor，使 Rust
            // CoordinatedCursor.should_animate 在仅关闭文字动画时仍正确上报光标移动。
            view.setKernelAnimationEnabled(settings.typingAnimationEnabled || settings.smoothCursorEnabled)
        }
    }

    /**
     * #595 三：原子应用 [EditorMotionPolicy] — 一次更新文字、光标、协同、时长和 reduce-motion。
     */
    fun applyMotionPolicy(policy: EditorMotionPolicy) {
        val effective = policy.effective()
        setEditorAnimationSettings(EditorAnimationSettings.fromMotionPolicy(effective))
    }

    /**
     * #595 三：当前动画策略 — 从规范设置派生的单一可观察事实源（不可变）。
     * UI 收集该值向下传递，[applyMotionPolicy] 原子更新。
     */
    val motionPolicy: EditorMotionPolicy
        get() = getEditorAnimationSettings().toMotionPolicy()

    /**
     * #595 三：动画策略 StateFlow — 只读可观察的单一事实源（不可变）。
     * UI 生命周期感知地收集该值，[applyMotionPolicy] 原子更新全部字段。
     */
    val motionPolicyFlow: kotlinx.coroutines.flow.StateFlow<EditorMotionPolicy>
        get() = sessionCoordinator.motionPolicyFlow

    /**
     * #595 六：窗口附着状态 — 从规范 [WindowBindingState] 派生，不引入并行状态机。
     * 叠加窗口级暂停标志（来自 [SujianEditorView.isAnimationPaused]）。
     */
    val attachmentState: EditorAttachmentState
        get() {
            val view = sharedEditorView
            val paused = view != null && view.isAnimationPaused()
            val frame = if (paused) currentFrameSnapshot() else null
            val projection = activeTargetId?.let { sessionCoordinator.getProjectionSnapshot(it) }
            return attachmentStateFromBinding(windowBindingState, paused, frame, projection)
        }

    private fun currentFrameSnapshot(): EditorFrameSnapshot? {
        val view = sharedEditorView ?: return null
        if (view.width <= 0 || view.height <= 0) return null
        return EditorFrameSnapshot(
            scrollX = view.getScrollXPos(),
            scrollY = view.getScrollYPos(),
            viewportWidth = view.width,
            viewportHeight = view.height,
            hasActiveAnimation = view.needsFrame(),
        )
    }

    // ── Edit operations (orchestrates session + window) ──

    fun beginEdit(targetId: String, initialSelection: Int? = null): Boolean {
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
        val bindInfo = sessionCoordinator.prepareSessionForEdit(targetId, initialSelection, windowId) ?: return false

        val target = targets[targetId] ?: return false
        target.onEditingStateChanged?.invoke(EditingState.BINDING)

        val view = getOrCreateEditorView()
        val bridge = TextEditSessionBridge(appServiceBridge, bindInfo.sessionId)
        // #592 一：复用既有持久 session 时执行 attachSnapshot（不调用
        // textEditSessionLoadText，Rust revision/Undo/Redo/composition 保持）；
        // 新建 session 或外部内容重置才走 bindSession/loadText。
        val snapshot = bindInfo.snapshot
        if (snapshot != null) {
            view.attachSession(
                sessionBridge = bridge,
                profile = bindInfo.profile,
                text = snapshot.text,
                revision = snapshot.revision,
                cursorUtf8 = snapshot.cursorUtf8,
                selStartUtf8 = snapshot.selectionAnchorUtf8,
                selEndUtf8 = snapshot.selectionHeadUtf8,
            )
            restoreProjectionScroll(view, targetId)
            rebuildProjectionFromSnapshot(targetId, snapshot)
        } else {
            view.bindSession(bridge, bindInfo.profile, bindInfo.text, bindInfo.selection)
            rebuildProjectionFromSnapshot(targetId, null)
        }

        // #595 七: 活动编辑时 SujianEditorView 是唯一的 FrameClock listener。
        // 投影运行时不接到 FrameClock — 它只在非活动预览（ReadonlyChapterPreview）时
        // 静态绘制，避免与 SujianEditorView 的 pipeline runtime 形成双驱动。
        // 投影的 mirror 在 detach/rebind 时从 session snapshot 重建。

        installContentCallback(view, target)
        installCommitRequestedCallback(view)
        installCancelRequestedCallback(view)

        sessionCoordinator.completeWindowAttach(windowId, targetId, bindInfo.sessionId)
        target.onEditingStateChanged?.invoke(EditingState.EDITING)

        view.post {
            view.requestFocus()
            val imm = context.getSystemService(android.content.Context.INPUT_METHOD_SERVICE) as? android.view.inputmethod.InputMethodManager
            imm?.showSoftInput(view, 0)
        }

        return true
    }

    /**
     * #592 三：重新附着窗口后恢复投影保存的滚动位置（配置变化/返回重进时）。
     * 会话层保存的是纯数据 scrollX/scrollY，这里应用到当前窗口的 View。
     */
    private fun restoreProjectionScroll(view: SujianEditorView, targetId: String) {
        val snapshot = sessionCoordinator.getProjectionSnapshot(targetId) ?: return
        view.setScrollPosition(snapshot.scrollX, snapshot.scrollY)
    }

    /**
     * #592 一/三：从真实 snapshot（或新建 session 的初值）重建窗口层投影运行时。
     * 投影属于窗口层；会话层只保存纯数据快照，跨配置变化由这里重建。
     */
    private fun rebuildProjectionFromSnapshot(targetId: String, snapshot: TargetSnapshot?) {
        val target = targets[targetId] ?: return
        val projection = getOrCreateProjection(targetId, target)
        if (snapshot != null) {
            projection.updateFromSnapshot(snapshot.text, snapshot.cursorUtf8, snapshot.revision)
        }
        val decorations = sessionCoordinator.getTargetDecorations(targetId)
        if (decorations != null) {
            applyDecorationsToProjection(projection, decorations)
        }
        val pure = sessionCoordinator.getProjectionSnapshot(targetId)
        if (pure != null) {
            projection.setScrollPosition(pure.scrollX, pure.scrollY)
            if (pure.viewportWidth > 0f) projection.setWidth(pure.viewportWidth)
            if (pure.viewportHeight > 0f) projection.setViewportSize(pure.viewportWidth.toInt(), pure.viewportHeight.toInt())
            if (pure.fontSizePx > 0f) projection.setFontSize(pure.fontSizePx)
            if (pure.lineSpacingMultiplier > 0f) projection.setLineSpacingMultiplier(pure.lineSpacingMultiplier)
            if (pure.themeColors != null) {
                projection.setThemeColors(
                    pure.themeColors.text, pure.themeColors.cursor, pure.themeColors.selection,
                    pure.themeColors.composing, pure.themeColors.background, pure.themeColors.searchHighlight
                )
            }
        }
    }

    private fun applyDecorationsToProjection(projection: TargetDisplayRuntime, decorations: TargetDecorations) {
        if (decorations.searchHighlightsUtf8.isEmpty() &&
            decorations.selectionStartUtf8 < 0 && decorations.selectionEndUtf8 < 0) {
            projection.clearDecorations()
        } else {
            projection.setSearchHighlights(decorations.searchHighlightsUtf8)
            if (decorations.selectionStartUtf8 >= 0 && decorations.selectionEndUtf8 >= 0) {
                projection.setSelection(decorations.selectionStartUtf8, decorations.selectionEndUtf8)
            }
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
        sharedEditorView?.let { view ->
            view.unbindSession("cancel")
        }
        val targetId = activeTargetId
        val target = targetId?.let { targets[it] }
        return sessionCoordinator.cancelActiveSession().also { success ->
            if (success) {
                target?.onEditingStateChanged?.invoke(EditingState.IDLE)
            }
        }
    }

    fun resetPersistentSession(targetId: String, text: String, cursorUtf8: Int, source: SessionResetSource = SessionResetSource.EXTERNAL) {
        sessionCoordinator.resetPersistentSession(targetId, text, cursorUtf8, source)
        if (targetId == activeTargetId) {
            val view = sharedEditorView
            if (view != null) {
                view.loadText(text, cursorUtf8)
            }
        }
        // 重建投影，保持窗口层预览与 Rust 状态一致
        val snapshot = sessionCoordinator.queryTargetSnapshot(targetId)
        if (snapshot != null) {
            rebuildProjectionFromSnapshot(targetId, snapshot)
        } else {
            val target = targets[targetId]
            if (target != null && sessionCoordinator.getPersistentSessionId(targetId) != null) {
                val projection = getOrCreateProjection(targetId, target)
                projection.updateFromSnapshot(text, cursorUtf8, projection.getRevision())
            }
        }
    }

    // ── SessionCommandPort (view pipeline when active, projection for inactive) ──

    override fun queryTargetSnapshot(targetId: String): TargetSnapshot? = sessionCoordinator.queryTargetSnapshot(targetId)

    override fun applyTargetCommand(targetId: String, command: TargetCommand): TargetCommandResult {
        if (targetId == activeTargetId) {
            val view = sharedEditorView
            if (view != null) {
                val snapshotBefore = sessionCoordinator.queryTargetSnapshot(targetId)
                    ?: return TargetCommandResult.Failed(TargetCommandError.SNAPSHOT_UNAVAILABLE)
                val commandPort = view.getPipeline()
                when (command) {
                    is TargetCommand.Replace -> {
                        val pipelineOutput = commandPort.replaceRangeTyped(
                            command.byteStart, command.byteEndExclusive,
                            command.replacementText, command.originalText,
                            uniffi.writer_core.EditorTransactionCauseDto.PROGRAMMATIC
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
                val snapshotAfter = sessionCoordinator.queryTargetSnapshot(targetId)
                    ?: return TargetCommandResult.Failed(TargetCommandError.SNAPSHOT_UNAVAILABLE)
                sessionCoordinator.updateTargetText(targetId, snapshotAfter.text)
                return TargetCommandResult.Success(snapshotAfter)
            }
        }
        // 非活动目标：先执行 bridge 命令，再把结果应用到窗口层投影
        val result = sessionCoordinator.executeTargetCommand(targetId, command)
        if (result is TargetCommandResult.Success) {
            val target = targets[targetId]
            if (target != null) {
                val projection = getOrCreateProjection(targetId, target)
                projection.updateFromSnapshot(
                    result.snapshot.text,
                    result.snapshot.cursorUtf8,
                    result.snapshot.revision,
                )
                val decorations = sessionCoordinator.getTargetDecorations(targetId)
                if (decorations != null) {
                    applyDecorationsToProjection(projection, decorations)
                }
            }
            if (targetId != activeTargetId) {
                onTargetContentChanged?.invoke(targetId, result.snapshot.text)
            }
        }
        return result
    }

    override fun setTargetDecorations(targetId: String, decorations: TargetDecorations) {
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
        val projection = targetProjections[targetId]
        if (projection != null) {
            applyDecorationsToProjection(projection, decorations)
        }
    }

    // ── View management ──

    fun getSharedEditorView(): SujianEditorView? = sharedEditorView

    fun obtainSharedEditorView(): SujianEditorView = getOrCreateEditorView()

    fun setSharedEditorView(view: SujianEditorView) {
        sharedEditorView = view
    }

    fun updateHostGeometry(width: Float, height: Float) {
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
        // #592 三：窗口销毁前解除投影 FrameClock 绑定，避免已释放时钟继续驱动投影。
        targetProjections.values.forEach { it.setFrameClock(null) }
        sharedEditorView?.let { view ->
            view.unbindSession("config_change")
            view.setFrameClock(null)
            view.release()
        }
        sharedEditorView = null
        targetProjections.values.forEach { it.release() }
        targetProjections.clear()
        windowFrameClock.release()
    }

    /**
     * Activity 永久结束 — 释放窗口和全部会话。
     */
    fun releaseHost() {
        clearActiveCallbacks()
        if (activeTargetId != null) {
            sharedEditorView?.let { view ->
                view.unbindSession("release")
            }
        }
        sharedEditorView?.let { view ->
            view.setFrameClock(null)
            view.release()
        }
        sharedEditorView = null
        targetProjections.values.forEach { it.setFrameClock(null) }
        targetProjections.values.forEach { it.release() }
        targetProjections.clear()
        windowFrameClock.release()
        sessionCoordinator.releaseHost()
    }

    // ── Private helpers ──

    private fun installContentCallback(view: SujianEditorView, target: EditableTextTarget) {
        view.onContentChanged = { newText ->
            target.onTextChanged?.invoke(newText)
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
            view.onContentChanged = null
            view.onCommitRequested = null
            view.onCancelRequested = null
            view.onSearchHighlightsCleared = null
        }
    }

    /**
     * #592 三：把窗口滚动/视口/字体/主题/装饰保存为会话层纯数据快照。
     * 不依赖 target 对象存在（窗口销毁时序晚于 Compose onDispose），
     * 只依赖 View 和投影运行时。
     */
    private fun saveActiveTargetProjection(targetId: String) {
        val view = sharedEditorView ?: return
        val themeColors = view.getPipelineThemeColors()
        val themeSnapshot = if (themeColors != null) {
            ThemeColorsSnapshot(
                themeColors.text, themeColors.cursor, themeColors.selection,
                themeColors.composing, themeColors.background, themeColors.searchHighlight
            )
        } else null
        sessionCoordinator.saveProjectionSnapshot(
            targetId,
            ProjectionSnapshot(
                scrollX = view.getScrollXPos(),
                scrollY = view.getScrollYPos(),
                viewportWidth = view.width.toFloat(),
                viewportHeight = view.height.toFloat(),
                fontSizePx = view.getPipelineTextPaintSize(),
                lineSpacingMultiplier = view.getPipelineLineSpacingMultiplier(),
                themeColors = themeSnapshot,
            ),
        )
    }

    private fun getOrCreateProjection(targetId: String, target: EditableTextTarget): TargetDisplayRuntime {
        return targetProjections.getOrPut(targetId) {
            val mirror = DisplayTextMirror()
            val textPaint = android.text.TextPaint().apply {
                textSize = target.profile.fontSizePx.coerceAtLeast(1f)
                isAntiAlias = true
            }
            TargetDisplayRuntime(mirror, textPaint, animationTimeSource, transactionIdSource)
        }
    }

    private fun getOrCreateEditorView(): SujianEditorView {
        return sharedEditorView ?: SujianEditorView(context, animationTimeSource = animationTimeSource, transactionIdSource = transactionIdSource).also {
            it.setFrameClock(windowFrameClock)
            val settings = sessionCoordinator.getEditorAnimationSettings()
            it.setTypingAnimationEnabled(settings.typingAnimationEnabled, settings.typingAnimationDurationMs)
            it.setSmoothCursorEnabled(settings.smoothCursorEnabled, settings.smoothCursorDurationMs)
            it.setCoordinatedAnimationEnabled(settings.coordinated)
            it.setReduceMotion(settings.reduceMotion)
            it.setKernelAnimationEnabled(settings.typingAnimationEnabled || settings.smoothCursorEnabled)
            sharedEditorView = it
        }
    }
}
