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
import com.xiwei.sujian.feature.editor.session.CoreVisualIntentEvent
import com.xiwei.sujian.feature.editor.session.ExternalContentDecision
import com.xiwei.sujian.feature.editor.session.SessionResetSource
import com.xiwei.sujian.feature.editor.session.WindowBindingState
import com.xiwei.sujian.feature.editor.session.applyExternalContentFact
import com.xiwei.sujian.feature.editor.session.consumePendingExternalFact
import com.xiwei.sujian.feature.editor.session.shouldApplyExternalContent
import com.xiwei.sujian.feature.editor.session.storePendingExternalFact
import com.xiwei.sujian.feature.editor.visual.ComposeEditorVisualState
import com.xiwei.sujian.feature.editor.visual.CursorVisualIntent
import com.xiwei.sujian.feature.editor.visual.EditorVisualIntent
import com.xiwei.sujian.feature.editor.visual.TextVisualKind
import com.xiwei.sujian.feature.editor.visual.VisualReplaceBounds
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

/** 生产动画链：设置状态 → Editor Host → 输入事务 → 动画协调器 → 真实 VSync 渲染。 */
@Composable
internal fun WritingPaneMotionPolicySync(
    coordinator: EditorWindowHost,
    settings: EditorSettingsState,
    chapterId: String,
) {
    LaunchedEffect(settings, chapterId) {
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
 * #641：收集 Core 视觉意图事件，映射为 [EditorVisualIntent] 喂给 [ComposeEditorVisualState]。
 * 按 target 过滤，避免其他 target 的视觉意图污染当前 overlay。
 *
 * #641 评论 问题3 + 评论 5457777142 问题4：收集 [EditorMotionPolicy] 的 effective 策略
 * 传给 [ComposeEditorVisualState.onVisualIntent]。
 */
@Composable
internal fun CollectVisualIntentEvents(
    viewModel: EditorViewModel,
    targetId: String,
    visualState: ComposeEditorVisualState,
    coordinator: EditorWindowHost,
) {
    val motionPolicy by coordinator.motionPolicyFlow.collectAsStateWithLifecycle()
    val currentMotionPolicy by rememberUpdatedState(motionPolicy)
    LaunchedEffect(viewModel, targetId) {
        viewModel.visualIntentEvents
            .collect { event ->
                if (event.targetId != targetId) return@collect
                val editorVisualIntent = mapCoreVisualIntentToEditorVisualIntent(event)
                visualState.onVisualIntent(editorVisualIntent, currentMotionPolicy)
            }
    }
}

/**
 * #641 评论 问题2：把 Core [VisualIntent]（UTF-8 byte ranges）转成 Compose [EditorVisualIntent]（UTF-16 ranges）。
 */
private fun mapCoreVisualIntentToEditorVisualIntent(event: CoreVisualIntentEvent): EditorVisualIntent {
    val textKind =
        when {
            event.visualIntent.isDelete() -> TextVisualKind.Delete
            event.visualIntent.isInsert() -> TextVisualKind.Insert
            event.visualIntent.isReplace() || event.visualIntent.isCompositionCommit() ||
                event.visualIntent.isCompositionUpdate() -> TextVisualKind.Move
            event.visualIntent.isCursorOnly() -> TextVisualKind.None
            event.visualIntent.isCompositionCancel() -> TextVisualKind.Delete
            else -> TextVisualKind.Move
        }

    val oldRanges =
        event.visualIntent.oldAffectedByteRanges.map { (start: Int, end: Int) ->
            TextOffsetUtils.utf16TextRangeForUtf8(event.oldText, start, end)
        }

    val newRanges =
        event.visualIntent.newAffectedByteRanges.map { (start: Int, end: Int) ->
            TextOffsetUtils.utf16TextRangeForUtf8(event.newText, start, end)
        }

    val coordinatedCursor = event.visualIntent.coordinatedCursor
    val cursor =
        if (coordinatedCursor.shouldAnimate) {
            val oldEndUtf16 =
                TextOffsetUtils.utf16OffsetForUtf8ByteOrNull(
                    text = event.oldText,
                    utf8ByteOffset = coordinatedCursor.oldByteOffset,
                )
            val newEndUtf16 =
                TextOffsetUtils.utf16OffsetForUtf8ByteOrNull(
                    text = event.newText,
                    utf8ByteOffset = coordinatedCursor.newByteOffset,
                )
            if (oldEndUtf16 != null && newEndUtf16 != null) {
                CursorVisualIntent(
                    oldEndUtf16 = oldEndUtf16,
                    newEndUtf16 = newEndUtf16,
                    animate = true,
                )
            } else {
                null
            }
        } else {
            null
        }

    val replaceBounds = computeVisualReplaceBounds(event.oldText, event.newText)

    return EditorVisualIntent(
        oldRanges = oldRanges,
        newRanges = newRanges,
        textKind = textKind,
        cursor = cursor,
        newTextLength = event.newText.length,
        expectedNewText = event.newText,
        replaceBounds = replaceBounds,
    )
}

/**
 * #641 评论 5458880786 问题2b：用 oldText/newText 做 code-point-safe diff 算 [VisualReplaceBounds] —
 * 共同前缀 + 共同后缀算出最小 replace 边界，offset 是 UTF-16。
 */
private fun computeVisualReplaceBounds(
    oldText: String,
    newText: String,
): VisualReplaceBounds {
    var oldStart = 0
    var newStart = 0
    while (oldStart < oldText.length && newStart < newText.length) {
        val oldCp = Character.codePointAt(oldText, oldStart)
        val newCp = Character.codePointAt(newText, newStart)
        if (oldCp != newCp) break
        oldStart += Character.charCount(oldCp)
        newStart += Character.charCount(newCp)
    }

    var oldEnd = oldText.length
    var newEnd = newText.length
    while (oldEnd > oldStart && newEnd > newStart) {
        val oldCp = Character.codePointBefore(oldText, oldEnd)
        val newCp = Character.codePointBefore(newText, newEnd)
        if (oldCp != newCp) break
        oldEnd -= Character.charCount(oldCp)
        newEnd -= Character.charCount(newCp)
    }

    return VisualReplaceBounds(
        oldStart = oldStart,
        oldEnd = oldEnd,
        newStart = newStart,
        newEnd = newEnd,
    )
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
