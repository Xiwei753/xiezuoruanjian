package com.xiwei.sujian.feature.editor.ui

import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.xiwei.sujian.feature.editor.input.TextOffsetUtils
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy
import com.xiwei.sujian.feature.editor.presentation.EditorSettingsState
import com.xiwei.sujian.feature.editor.presentation.EditorViewModel
import com.xiwei.sujian.feature.editor.presentation.applyExternalContentToUi
import com.xiwei.sujian.feature.editor.presentation.isCurrentChapter
import com.xiwei.sujian.feature.editor.presentation.notifySyncMergeConflict
import com.xiwei.sujian.feature.editor.presentation.reloadSettings
import com.xiwei.sujian.feature.editor.presentation.shouldConsumePendingAfterFact
import com.xiwei.sujian.feature.editor.session.CoreEditFactEvent
import com.xiwei.sujian.feature.editor.session.ExternalContentDecision
import com.xiwei.sujian.feature.editor.session.SessionResetSource
import com.xiwei.sujian.feature.editor.session.WindowBindingState
import com.xiwei.sujian.feature.editor.session.applyExternalContentFact
import com.xiwei.sujian.feature.editor.session.consumePendingExternalFact
import com.xiwei.sujian.feature.editor.session.shouldApplyExternalContent
import com.xiwei.sujian.feature.editor.session.storePendingExternalFact
import com.xiwei.sujian.feature.editor.visual.ComposeEditorVisualState
import com.xiwei.sujian.feature.editor.visual.EditorEditFact
import com.xiwei.sujian.feature.editor.visual.mapCoreEditFactToEditorEditFact
import com.xiwei.sujian.feature.editor.window.EditorWindowHost
import kotlinx.coroutines.flow.filter

// ── 外部文档事实 ──────────────────────────────────────────────

/**
 * #595 一/二：外部文档事实（RepositoryLoaded/SyncMerged）— 调用方已通过
 * shouldApplyExternalContent 确认版本更新与本地 dirty 状态，此处只执行
 * Core reset 和 UI 同步，不再重复构造事件做检查。
 */
@Composable
internal fun WritingPaneExternalContentFlow(
    viewModel: EditorViewModel,
    coordinator: EditorWindowHost,
    targetId: String,
    currentUiState: com.xiwei.sujian.feature.editor.presentation.EditorUiState,
) {
    val currentViewModel by rememberUpdatedState(viewModel)
    val currentCoordinator by rememberUpdatedState(coordinator)
    val latestUiState by rememberUpdatedState(currentUiState)

    LaunchedEffect(targetId) {
        currentViewModel.documentUpdates(targetId).collect { fact ->
            if (latestUiState.loading) return@collect
            handleExternalDocumentFact(currentCoordinator, currentViewModel, targetId, fact)
        }
    }
}

/**
 * #595 一/二：外部文档事实决策执行 — 调用方已通过 shouldApplyExternalContent
 * 确认版本更新与本地 dirty 状态，此处只执行 Core reset 和 UI 同步。
 * #624 评论13 第4项：suspend — 与 [EditorViewModel.applyExternalContentToUi]
 * （await calculateWordCount）同一调用链；本来就在 LaunchedEffect collect 里调用。
 */
internal suspend fun handleExternalDocumentFact(
    coordinator: EditorWindowHost,
    viewModel: EditorViewModel,
    targetId: String,
    fact: com.xiwei.sujian.feature.editor.session.TargetDocumentFact,
) {
    when (val decision = coordinator.sessionCoordinator.shouldApplyExternalContent(fact)) {
        ExternalContentDecision.Apply ->
            applyExternalDocumentFact(coordinator, viewModel, targetId, fact)
        ExternalContentDecision.IgnoreSameContent -> {
            coordinator.sessionCoordinator.applyExternalContentFact(fact)
            coordinator.sessionCoordinator.consumePendingExternalFact(targetId)
        }
        ExternalContentDecision.IgnoreDirtyConflict -> {
            coordinator.sessionCoordinator.storePendingExternalFact(targetId, fact)
            if (fact.origin == com.xiwei.sujian.feature.editor.session.DocumentFactOrigin.SYNC_MERGED) {
                viewModel.notifySyncMergeConflict()
            }
        }
        ExternalContentDecision.IgnoreReplay,
        ExternalContentDecision.IgnoreOlder,
        -> consumePendingForReapplyIfApplicable(coordinator, targetId, decision, fact)
        ExternalContentDecision.IgnoreEmptyVersion -> {
        }
        ExternalContentDecision.IgnoreUncomparableConflict -> {
            coordinator.sessionCoordinator.storePendingExternalFact(targetId, fact)
            if (fact.origin == com.xiwei.sujian.feature.editor.session.DocumentFactOrigin.SYNC_MERGED) {
                viewModel.notifySyncMergeConflict()
            }
        }
    }
}

/**
 * #595 一/二：Apply 分支执行 — Core reset + 会话事实提交 + bridge 同步 + ViewModel 同步。
 * #641 评论1 第2节：composition 活跃时不得覆盖 TextFieldState。
 */
internal suspend fun applyExternalDocumentFact(
    coordinator: EditorWindowHost,
    viewModel: EditorViewModel,
    targetId: String,
    fact: com.xiwei.sujian.feature.editor.session.TargetDocumentFact,
) {
    if (viewModel.isBridgeComposing(targetId)) {
        coordinator.sessionCoordinator.storePendingExternalFact(targetId, fact)
        if (fact.origin == com.xiwei.sujian.feature.editor.session.DocumentFactOrigin.SYNC_MERGED) {
            viewModel.notifySyncMergeConflict()
        }
        return
    }

    val resetResult =
        coordinator.resetPersistentSession(
            targetId,
            fact.text,
            fact.text.toByteArray(Charsets.UTF_8).size,
            SessionResetSource.EXTERNAL,
        )
    if (resetResult is com.xiwei.sujian.feature.editor.session.ExternalResetResult.Success &&
        coordinator.activeTargetId != targetId
    ) {
        coordinator.beginEdit(targetId)
    }
    if (resetResult !is com.xiwei.sujian.feature.editor.session.ExternalResetResult.Success) return
    coordinator.sessionCoordinator.applyExternalContentFact(fact)
    coordinator.sessionCoordinator.consumePendingExternalFact(targetId)
    val snapshot = coordinator.queryTargetSnapshot(targetId)
    if (snapshot != null) {
        viewModel.applyAuthoritativeToBridge(
            targetId,
            fact.text,
            snapshot.selectionAnchorUtf8,
            snapshot.selectionHeadUtf8,
        )
    }
    if (fact.origin == com.xiwei.sujian.feature.editor.session.DocumentFactOrigin.SYNC_MERGED) {
        viewModel.applyExternalContentToUi(targetId, fact.text, fact.sourceVersion.contentHash)
    }
}

/**
 * #624 评论17 问题5：reapply fact 的 IgnoreReplay/IgnoreOlder 消费 pending。
 */
private fun consumePendingForReapplyIfApplicable(
    coordinator: EditorWindowHost,
    targetId: String,
    decision: ExternalContentDecision,
    fact: com.xiwei.sujian.feature.editor.session.TargetDocumentFact,
) {
    if (shouldConsumePendingAfterFact(decision, fact.isReapply)) {
        coordinator.sessionCoordinator.consumePendingExternalFact(targetId)
    }
}

// ── 编辑器附着 ────────────────────────────────────────────────

/** 编辑器附着所需的可观察状态（正文/会话/章节身份）。 */
internal data class EditorAttachInputs(
    val uiState: com.xiwei.sujian.feature.editor.presentation.EditorUiState,
    val sessionState: com.xiwei.sujian.feature.editor.session.EditorSessionState,
    val chapter: ChapterRef,
)

/** 编辑器附着（beginEdit）。 */
@Composable
internal fun WritingPaneEditorAttachSync(
    currentViewModel: EditorViewModel,
    coordinator: EditorWindowHost,
    targetId: String,
    inputs: EditorAttachInputs,
) {
    WritingPaneEditorAttach(currentViewModel, coordinator, targetId, inputs)
}

/**
 * #644 评论 5462826712 第3节：编辑器附着决策 — 纯函数。
 */
sealed interface EditorAttachAction {
    data object BeginEdit : EditorAttachAction

    data object Hold : EditorAttachAction
}

fun editorAttachDecision(
    bindingState: WindowBindingState,
    windowId: String,
    targetId: String,
): EditorAttachAction =
    when (bindingState) {
        is WindowBindingState.Attached ->
            if (bindingState.windowId == windowId && bindingState.targetId == targetId) {
                EditorAttachAction.Hold
            } else {
                EditorAttachAction.BeginEdit
            }
        is WindowBindingState.Attaching ->
            if (bindingState.windowId == windowId && bindingState.targetId == targetId) {
                EditorAttachAction.Hold
            } else {
                EditorAttachAction.BeginEdit
            }
        is WindowBindingState.Detached -> EditorAttachAction.BeginEdit
        WindowBindingState.Idle -> EditorAttachAction.BeginEdit
        is WindowBindingState.Committing -> EditorAttachAction.Hold
        is WindowBindingState.Cancelling -> EditorAttachAction.Hold
        is WindowBindingState.Detaching -> EditorAttachAction.Hold
    }

/**
 * #644 评论 5462826712 第3节：编辑器附着 — 用 [editorAttachDecision] 纯函数决策。
 */
@Composable
private fun WritingPaneEditorAttach(
    currentViewModel: EditorViewModel,
    coordinator: EditorWindowHost,
    targetId: String,
    inputs: EditorAttachInputs,
) {
    LaunchedEffect(targetId, inputs.uiState.loading, inputs.uiState.settingsReady, inputs.sessionState.bindingState) {
        if (!currentViewModel.isCurrentChapter(
                inputs.chapter.projectId,
                inputs.chapter.volumeId,
                inputs.chapter.chapterId,
            )
        ) {
            return@LaunchedEffect
        }
        val binding = inputs.sessionState.bindingState
        when (editorAttachDecision(binding, coordinator.windowId, targetId)) {
            EditorAttachAction.BeginEdit -> {
                if (shouldBeginEditForEditorAttach(inputs.uiState.loading, inputs.uiState.settingsReady)) {
                    coordinator.beginEdit(targetId)
                }
            }
            EditorAttachAction.Hold -> {
                // do nothing - attach is confirmed only by WritingEditorSurface.onSurfaceReady()
            }
        }
    }
}

/**
 * #630 评论 5327560790: BeginEdit 门槛 — 纯函数。
 */
internal fun shouldBeginEditForEditorAttach(
    loading: Boolean,
    settingsReady: Boolean,
): Boolean = !loading && settingsReady

/**
 * #630 评论 5327560790: 从持久化 [EditorSettingsState] 构造首帧 [EditorTypography] — 纯函数。
 */
internal fun editorTypographyFromSettings(
    settings: EditorSettingsState,
): com.xiwei.sujian.feature.editor.window.EditorTypography =
    com.xiwei.sujian.feature.editor.window.EditorTypography(
        fontSizeSp = settings.fontSize,
        lineSpacingMultiplier = settings.lineSpacingMultiplier,
        autoIndentEnabled = settings.autoIndentEnabled,
        autoIndentWidth = settings.autoIndentWidth,
    )

// ── 动画/排版/设置同步 ────────────────────────────────────────

/**
 * 生产动画链：设置状态 → EditorSessionCoordinator.motionPolicyFlow 唯一写入口。
 *
 * Issue #732 评论 5763493968 第1节：LaunchedEffect 只 key 动画相关字段，不再 key 整个 settings，
 * 也不需要 chapterId — policy 是进程级动画事实，不随章节切换重置。
 * 真正绑定到 visualState 的工作由 [BindMotionPolicyToVisualState] 完成，
 * 那里只 collect 一次 motionPolicyFlow 并交给 visualState.updateMotionPolicy。
 */
@Composable
internal fun WritingPaneMotionPolicySync(
    coordinator: EditorWindowHost,
    settings: EditorSettingsState,
) {
    LaunchedEffect(
        settings.typingAnimationEnabled,
        settings.typingAnimationDurationMs,
        settings.smoothCursorEnabled,
        settings.smoothCursorDurationMs,
        settings.coordinatedTextCursorAnimationEnabled,
        settings.reduceMotion,
    ) {
        coordinator.applyMotionPolicy(
            EditorMotionPolicy(
                textEnabled = settings.typingAnimationEnabled,
                textDurationMillis = settings.typingAnimationDurationMs,
                cursorEnabled = settings.smoothCursorEnabled,
                cursorDurationMillis = settings.smoothCursorDurationMs,
                coordinated = settings.coordinatedTextCursorAnimationEnabled,
                reduceMotion = settings.reduceMotion,
            ),
        )
    }
}

/**
 * Issue #732 评论 5763493968 第1节：把 [EditorSessionCoordinator.motionPolicyFlow] 绑定到
 * [ComposeEditorVisualState] — 只 collect 一次 flow，把新 policy 交给
 * [ComposeEditorVisualState.updateMotionPolicy]。
 *
 * 这里不再自己 `withFrameNanos` — policy 切换、patch 消费、motion 创建全部由
 * [EditorTextFieldDrawLayer] 的单一帧循环统一处理（第3节）。
 */
@Composable
internal fun BindMotionPolicyToVisualState(
    coordinator: EditorWindowHost,
    visualState: ComposeEditorVisualState,
) {
    val policy by coordinator.motionPolicyFlow.collectAsStateWithLifecycle()
    val currentPolicy by rememberUpdatedState(policy)
    LaunchedEffect(currentPolicy) {
        visualState.updateMotionPolicy(currentPolicy)
    }
}

/**
 * 排版设置链：字号/行距/首行缩进设置 → Editor Host → 当前共享编辑器 View。
 * #632 评论 5378239827 项3: 去掉 chapterId key — applyEditorTypography 已幂等。 */
@Composable
internal fun WritingPaneTypographySync(
    coordinator: EditorWindowHost,
    settings: EditorSettingsState,
) {
    LaunchedEffect(
        settings.fontSize,
        settings.lineSpacingMultiplier,
        settings.autoIndentEnabled,
        settings.autoIndentWidth,
    ) {
        coordinator.applyEditorTypography(
            fontSizeSp = settings.fontSize,
            lineSpacingMultiplier = settings.lineSpacingMultiplier,
            autoIndentEnabled = settings.autoIndentEnabled,
            autoIndentWidth = settings.autoIndentWidth,
        )
    }
}

/** 设置变更通过 CoreSettingsEvents.editorSettingsChanged SharedFlow 推送，
 * ON_RESUME 兜底处理进程恢复场景。 */
@Composable
internal fun WritingPaneSettingsReload(
    viewModel: EditorViewModel,
    targetId: String,
) {
    val lifecycleOwner = LocalLifecycleOwner.current
    DisposableEffect(targetId, lifecycleOwner) {
        val observer =
            androidx.lifecycle.LifecycleEventObserver { _, event ->
                if (event == androidx.lifecycle.Lifecycle.Event.ON_RESUME) {
                    viewModel.reloadSettings()
                }
            }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose {
            lifecycleOwner.lifecycle.removeObserver(observer)
        }
    }
    LaunchedEffect(targetId) {
        com.xiwei.sujian.feature.settings.data.CoreSettingsEvents.editorSettingsChanged.collect {
            viewModel.reloadSettings()
        }
    }
}

// ── 章节切换 ──────────────────────────────────────────────────

/** 章节引用 — 当前 pane 的章节身份（用于切换同步与附着判断）。 */
internal data class ChapterRef(
    val projectId: String,
    val volumeId: String,
    val chapterId: String,
    val title: String,
)

/**
 * 章节切换收口：深链/恢复路径（currentSession 不是本 pane 章节）走 switchChapter 事务。
 * #624 评论14 第2项：failedSwitchTarget 已删除 — switchLoadAndPrepare 不提前发布 B，
 * isCurrentChapter 守卫已足够；onChapterSwitchFailed 回调保留用于保存/加载失败回滚导航。
 */
@Composable
internal fun rememberChapterSwitchSync(
    viewModel: EditorViewModel,
    chapter: ChapterRef,
    targetId: String,
    onChapterSwitchFailed: (
        (oldProjectId: String, oldVolumeId: String?, oldChapterId: String?, oldChapterTitle: String) -> Unit
    )?,
) {
    var lastProjectId by remember { mutableStateOf("") }
    var lastVolumeId by remember { mutableStateOf("") }
    var lastChapterId by remember { mutableStateOf("") }
    var lastChapterTitle by remember { mutableStateOf("") }
    val currentViewModel by rememberUpdatedState(viewModel)

    LaunchedEffect(chapter.projectId, chapter.volumeId, chapter.chapterId) {
        val sameChapter =
            lastChapterId.isNotEmpty() &&
                lastProjectId == chapter.projectId &&
                lastVolumeId == chapter.volumeId &&
                lastChapterId == chapter.chapterId
        if (!sameChapter) {
            if (!currentViewModel.isCurrentChapter(chapter.projectId, chapter.volumeId, chapter.chapterId)) {
                when (
                    val result =
                        viewModel.switchChapter(
                            chapter.projectId,
                            chapter.volumeId,
                            chapter.chapterId,
                            chapter.title,
                        )
                ) {
                    is com.xiwei.sujian.feature.editor.presentation.ChapterSwitchResult.Success -> {
                    }
                    is com.xiwei.sujian.feature.editor.presentation.ChapterSwitchResult.SaveFailed,
                    is com.xiwei.sujian.feature.editor.presentation.ChapterSwitchResult.LoadFailed,
                    -> {
                        onChapterSwitchFailed?.invoke(
                            lastProjectId.takeIf { it.isNotEmpty() } ?: chapter.projectId,
                            lastVolumeId.takeIf { it.isNotEmpty() },
                            lastChapterId.takeIf { it.isNotEmpty() },
                            lastChapterTitle,
                        )
                    }
                    com.xiwei.sujian.feature.editor.presentation.ChapterSwitchResult.Stale -> {
                    }
                }
            }
            lastProjectId = chapter.projectId
            lastVolumeId = chapter.volumeId
            lastChapterId = chapter.chapterId
            lastChapterTitle = chapter.title
        }
    }
}

// ── 视觉意图收集 ──────────────────────────────────────────────

/**
 * Issue #735 评论 5771063665：收集 Core 编辑事实事件，映射为 [EditorEditFact] 喂给 [ComposeEditorVisualState]。
 *
 * Core 已不再返回视觉意图。Android 从 [CoreEditFactEvent] 的 cause/operationKind/offsetMap
 * 推导动画策略。
 *
 * Issue #735 评论 5773604666 问题1：删除本地输入分流 —
 * 所有正文编辑（包括 TYPING/TYPING_COMMIT/IME_COMPOSITION/PASTE/DELETE）都从同一个
 * [EditorEditFact] 入口进平台 motion，不再跳过 Core visual path。
 * 目标链路：平台输入 -> EditorCommand -> EditorKernel -> EditorEditResult -> 平台 layout
 * -> 平台 motion -> 平台 render。
 */
@Composable
internal fun CollectEditFactEvents(
    viewModel: EditorViewModel,
    targetId: String,
    visualState: ComposeEditorVisualState,
    coordinator: EditorWindowHost,
) {
    LaunchedEffect(viewModel, targetId) {
        viewModel.editFactEvents
            .collect { event ->
                if (event.targetId != targetId) return@collect
                // Issue #735 评论 5773604666 问题1：所有 cause 都走同一个 EditorEditFact 入口 —
                // 不再用 isLocalInputCause() 跳过本地输入 cause，统一送进 ComposeVisualFrameCoordinator。
                val editFact = mapCoreEditFactToEditorEditFact(event)
                visualState.onEditFact(editFact)
            }
    }
}

// ── 权威快照收集 ──────────────────────────────────────────────

/**
 * #641 评论 问题7d + 评论 5457777142 问题5 + 评论 5458283021 问题4：
 * 收集 undo/redo 后的权威编辑器快照，把正文写回 bridge。
 *
 * #641 评论 5458283021 问题4：单一串行 collector —
 * 不再用两个独立 collector 竞争。pending authoritative 事实收进 bridge，
 * 由同一个 `snapshotFlow { EditorInputSnapshot }.collect(bridge::onInputSnapshot)` 决定顺序。
 */
@Composable
@Suppress("CognitiveComplexMethod")
internal fun CollectAuthoritativeEditorSnapshots(
    coordinator: EditorWindowHost,
    viewModel: EditorViewModel,
    targetId: String,
    bridge: com.xiwei.sujian.feature.editor.input.EditorTextFieldStateBridge,
) {
    LaunchedEffect(targetId) {
        coordinator.authoritativeEditorSnapshots
            .filter { it.targetId == targetId }
            .collect { snapshot ->
                if (bridge.state.composition == null) {
                    if (snapshot.text != bridge.mirroredText) {
                        viewModel.applyAuthoritativeToBridge(
                            snapshot.targetId,
                            snapshot.text,
                            snapshot.selectionAnchorUtf8,
                            snapshot.selectionHeadUtf8,
                        )
                    }
                } else {
                    val utf16Selection =
                        TextOffsetUtils.utf16TextRangeForUtf8(
                            snapshot.text,
                            snapshot.selectionAnchorUtf8,
                            snapshot.selectionHeadUtf8,
                        )
                    bridge.storePendingAuthoritative(snapshot.text, utf16Selection)
                }
            }
    }
}
