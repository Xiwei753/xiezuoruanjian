package com.xiwei.sujian.feature.editor.ui

import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import com.xiwei.sujian.app.di.LocalSujianAppDependencies
import com.xiwei.sujian.feature.editor.input.TextOffsetUtils
import com.xiwei.sujian.feature.editor.presentation.EditorSettingsState
import com.xiwei.sujian.feature.editor.presentation.EditorViewModel
import com.xiwei.sujian.feature.editor.presentation.applyExternalContentToUi
import com.xiwei.sujian.feature.editor.presentation.confirmEditorAttached
import com.xiwei.sujian.feature.editor.presentation.isCurrentChapter
import com.xiwei.sujian.feature.editor.presentation.notifySyncMergeConflict
import com.xiwei.sujian.feature.editor.presentation.onEditorApplied
import com.xiwei.sujian.feature.editor.presentation.reloadSettings
import com.xiwei.sujian.feature.editor.presentation.shouldConsumePendingAfterFact
import com.xiwei.sujian.feature.editor.session.CoreVisualIntentEvent
import com.xiwei.sujian.feature.editor.session.ExternalContentDecision
import com.xiwei.sujian.feature.editor.session.SessionResetSource
import com.xiwei.sujian.feature.editor.session.TextEditorProfile
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
import com.xiwei.sujian.feature.editor.window.EditableTextTarget
import kotlinx.coroutines.flow.filter

/**
 * 正文编辑窗格 — 「正文」一级内容（#624 评论17 第2部分 Route 层）。
 *
 * Route 层负责收集状态/事务，把纯展示值传给 [WritingPaneLayout]；
 * 通过 editorContent slot 注入 WritingEditorSurface。
 *
 * 会话生命周期：
 * - DisposableEffect 注册/注销 target
 * - LaunchedEffect(chapterId) 处理章节切换收口（closeTarget 旧章节）
 * - LaunchedEffect(targetId, uiState.loading, sessionState.bindingState) 附着编辑器
 * - LaunchedEffect(targetId) 收集版本化文档事实（Repository 加载 / 同步合并）
 *
 * #595 一：输入窗口防护 — 只有 ViewModel 当前已提交章节（isCurrentChapter）
 * 才显示编辑器；切换事务提交后、导航落地前，旧 pane 不显示 View、
 * 不安装输入回调，旧章节最后一次输入不可能写进新章节。
 *
 * #641：正文 UI 由 state-based [BasicTextField] 接管 —
 * 编辑器只在 WorkspaceLocation.Editor 真正存在，离开 Composition 自动收起 IME。
 */
@Composable
@Suppress("LongParameterList")
fun WritingPane(
    projectId: String,
    volumeId: String,
    chapterId: String,
    chapterTitle: String,
    modifier: Modifier = Modifier,
    /**
     * #595 一：章节切换事务失败回调（保存失败/加载失败）。
     * 参数是回滚目标（旧章节）；旧章节不存在时传 null（应清除章节选择/回到章节树）。
     * 由工作区宿主把工作区导航回滚到 activeChapterKey，不能只把 loading 改回 false。
     */
    onChapterSwitchFailed: (
        (oldProjectId: String, oldVolumeId: String?, oldChapterId: String?, oldChapterTitle: String) -> Unit
    )? = null,
) {
    val context = LocalContext.current
    val deps = LocalSujianAppDependencies.current
    val coordinator =
        LocalEditorWindowHost.current
            ?: throw IllegalStateException(
                "WritingPane requires an EditorWindowHost in the CompositionLocal. " +
                    "Ensure the host Activity or Fragment provides one via CompositionLocalProvider.",
            )
    val viewModel = rememberWritingPaneViewModel(context, deps, coordinator)

    val targetId =
        remember(projectId, volumeId, chapterId) {
            "chapter-body:$projectId:$volumeId:$chapterId"
        }

    val uiState by viewModel.uiState.collectAsStateWithLifecycle()

    // 生产动画链：设置状态 → Editor Host → 输入事务 → 动画协调器 → 真实 VSync 渲染。
    WritingPaneMotionPolicySync(coordinator, uiState.settings, chapterId)
    // #624 评论3/4：字号/行距/首行缩进 → Editor Host → 当前共享 View 立即重排。
    // #632 评论 5378239827 项3: 去掉 chapterId key — applyEditorTypography 已幂等，
    // 切章不应用重排排版设置；LaunchedEffect 只依赖排版参数本身。
    WritingPaneTypographySync(coordinator, uiState.settings)
    WritingPaneSettingsReload(viewModel, targetId)

    // #595 一：章节切换收口 — 宿主已用 requestOpenChapter 预提交章节
    // （保存/加载/session 预准备成功后才导航）；pane 只负责旧 target 的窗口解绑
    // （DisposableEffect onDispose 的 detachWindowBinding）。
    // #595 一：章节切换不业务关闭旧章节的持久 session — 快速连续点击章节时
    // 原章节的 Undo/Redo 历史保留（persistent session 留在 store 的 Detached
    // 状态）；返回章节列表/作品列表时由 ProjectWorkspaceScreen 以
    // WORKSPACE_NAVIGATION 关闭。
    // 深链/恢复路径（currentSession 不是本 pane 章节）仍走 switchChapter 事务。
    val chapter = ChapterRef(projectId, volumeId, chapterId, chapterTitle)
    // #624 评论14 第2项：failedSwitchTarget 已删除 — switchLoadAndPrepare 不提前发布 B，
    // WritingPaneEditorAttach 的 isCurrentChapter 守卫已足够阻止提前 beginEdit。
    // onChapterSwitchFailed 回调仍需保留（保存/加载失败回滚导航）。
    rememberChapterSwitchSync(
        viewModel,
        chapter,
        targetId,
        onChapterSwitchFailed,
    )

    val sessionState by coordinator.sessionStateFlow.collectAsStateWithLifecycle()
    val currentUiState by rememberUpdatedState(uiState)
    val currentViewModel by rememberUpdatedState(viewModel)

    val target =
        rememberWritingPaneTarget(
            currentViewModel = currentViewModel,
            coordinator = coordinator,
            targetId = targetId,
        )

    // #595 二：按 target 分区的最新文档事实流（带 replay）— 新 collector 立即
    // 拿到当前文档事实，同 sourceVersion 重放由 reducer 幂等忽略。
    WritingPaneExternalContentFlow(
        viewModel,
        coordinator,
        targetId,
        currentUiState,
    )

    WritingPaneEditorAttachSync(
        currentViewModel = currentViewModel,
        coordinator = coordinator,
        targetId = targetId,
        inputs = EditorAttachInputs(uiState, sessionState, chapter),
    )

    // #630 R12 首帧竞态：isActivePane 计算一次，复用于 shouldShowEditorForWritingPane
    // 和 WritingEditorSurface — 同一帧内 showEditor 门控与 Surface 渲染决策用同一个布尔值，
    // 避免 commitPreparedSession 提交新章节后 activeTargetId 仍为 null 的一帧里
    // settingsReady=true 把 Surface 放进 Composition 但 isCurrentChapter 尚未落地。
    val isActivePane = currentViewModel.isCurrentChapter(projectId, volumeId, chapterId)
    WritingPaneLayout(
        modifier = modifier,
        uiState = uiState,
        // #630 R12：showEditor 纳入 settingsReady 门控 —
        // 设置未加载完时继续显示现有 loading UI，不提前进入正文 Surface。
        // 即使 settingsReady=true 到 beginEdit/Attaching 间隔仍隔一帧，
        // #640 A.3：AndroidView 已始终组合（EditorSurfaceMode.EditorHost），
        // 用 View.INVISIBLE 控制可见性。
        showEditor =
            shouldShowEditorForWritingPane(
                loading = uiState.loading,
                settingsReady = uiState.settingsReady,
                isCurrentChapter = isActivePane,
            ),
    ) { editorModifier ->
        WritingPaneEditorContent(
            coordinator = coordinator,
            viewModel = currentViewModel,
            targetId = targetId,
            isActivePane = isActivePane,
            sessionState = sessionState,
            uiState = uiState,
            modifier = editorModifier,
        )
    }
}

/**
 * #641：正文内容渲染 — 活动 target → [WritingEditorSurface]([BasicTextField])；
 * 非活动 → 只读预览。从 [WritingPane] 提取以控制函数长度。
 *
 * #644 评论 5462826712 第2/4/5/6节：viewportState 管滚动/视口，visualState 按 target 隔离，
 * DisposableEffect 保存真实 anchor，onSurfaceReady 完成 attach。
 */
@Composable
@Suppress("LongParameterList")
private fun WritingPaneEditorContent(
    coordinator: com.xiwei.sujian.feature.editor.window.EditorWindowHost,
    viewModel: EditorViewModel,
    targetId: String,
    isActivePane: Boolean,
    sessionState: com.xiwei.sujian.feature.editor.session.EditorSessionState,
    uiState: com.xiwei.sujian.feature.editor.presentation.EditorUiState,
    modifier: Modifier,
) {
    @Suppress("UNUSED_EXPRESSION")
    (coordinator.targetDecorationsVersionFlow.collectAsStateWithLifecycle().value)
    val surfaceMode = editorSurfaceMode(sessionState.bindingState, coordinator.windowId, targetId, isActivePane)
    val inputEnabled by viewModel.inputEnabled.collectAsStateWithLifecycle()
    when (surfaceMode) {
        EditorSurfaceMode.EditorHost -> {
            val bridge = viewModel.bridgeForTarget(targetId, uiState.content)

            // #644 评论 5462826712 第4节：viewportState 管滚动/视口
            val projection = coordinator.sessionCoordinator.getProjectionSnapshot(targetId)
            val viewportState = com.xiwei.sujian.feature.editor.layout.rememberEditorViewportState(
                targetId = targetId,
                initialAnchor = projection?.viewportAnchor,
            )

            // #644 评论 5462826712 第6节：visualState 按 target 隔离
            val visualState = remember(targetId) { ComposeEditorVisualState() }

            // #644 评论 5462826712 第5节：退出当前 target 时保存真实 anchor
            DisposableEffect(targetId, viewportState) {
                onDispose {
                    val anchor = viewportState.snapshotAnchor()
                    coordinator.sessionCoordinator.saveProjectionSnapshot(
                        targetId,
                        com.xiwei.sujian.feature.editor.session.ProjectionSnapshot(anchor),
                    )
                }
            }

            // #641 评论1 第2节：观察 TextFieldState 快照，提交已完成的文本变化给 Core。
            androidx.compose.runtime.LaunchedEffect(bridge) {
                androidx.compose.runtime.snapshotFlow {
                    com.xiwei.sujian.feature.editor.input.EditorInputSnapshot(
                        text = bridge.state.text.toString(),
                        selection = bridge.state.selection,
                        composition = bridge.state.composition,
                    )
                }.collect(bridge::onInputSnapshot)
            }

            // #641：undo/redo 后 session 正文变化时把权威正文写回 bridge（UTF-8→UTF-16）。
            val lastCommittedText by coordinator.lastCommittedTextFlow.collectAsStateWithLifecycle()
            androidx.compose.runtime.LaunchedEffect(targetId, lastCommittedText) {
                val committed = lastCommittedText
                if (committed != null &&
                    committed != bridge.mirroredText &&
                    bridge.state.composition == null
                ) {
                    val snapshot = coordinator.queryTargetSnapshot(targetId)
                    if (snapshot != null && snapshot.text != bridge.mirroredText) {
                        viewModel.applyAuthoritativeToBridge(
                            targetId,
                            snapshot.text,
                            snapshot.selectionAnchorUtf8,
                            snapshot.selectionHeadUtf8,
                        )
                    }
                }
            }

            CollectAuthoritativeEditorSnapshots(
                coordinator = coordinator,
                viewModel = viewModel,
                targetId = targetId,
                bridge = bridge,
            )

            CollectVisualIntentEvents(
                viewModel = viewModel,
                targetId = targetId,
                visualState = visualState,
                coordinator = coordinator,
            )

            val decorations = coordinator.getTargetDecorations(targetId)
            val authoritativeText = bridge.mirroredText
            val searchHighlightsUtf16 =
                decorations?.searchHighlightsUtf8?.map { (start, end) ->
                    val r =
                        com.xiwei.sujian.feature.editor.input.TextOffsetUtils
                            .utf16TextRangeForUtf8(authoritativeText, start, end)
                    com.xiwei.sujian.feature.editor.projection.TextRange(r.start, r.end)
                } ?: emptyList()

            // #644 评论 5462826712 第2节：onSurfaceReady 回调 — attach 成功才算输入 surface ready
            WritingEditorSurface(
                bridge = bridge,
                visualState = visualState,
                viewportState = viewportState,
                textStyle = rememberEditorTextStyle(uiState.settings),
                textColor = androidx.compose.material3.MaterialTheme.colorScheme.onSurface,
                cursorColor = androidx.compose.material3.MaterialTheme.colorScheme.primary,
                inputEnabled = inputEnabled,
                onSurfaceReady = {
                    val lease = coordinator.attachSurface(targetId) ?: return@WritingEditorSurface false
                    viewModel.confirmEditorAttached(targetId, lease)
                    true
                },
                searchHighlights = searchHighlightsUtf16,
                modifier = modifier,
            )
        }
        EditorSurfaceMode.Preview -> {
            val previewState = coordinator.getChapterPreviewState(targetId)
            if (previewState != null && previewState.text.isNotEmpty()) {
                com.xiwei.sujian.feature.editor.ui.ReadonlyChapterPreview(previewState = previewState)
            }
        }
    }
}

/**
 * #641 评论 问题6d：编辑器 TextStyle 构造 — 字号/行距/首行缩进写进 Compose paragraph/text style。
 *
 * 首行缩进 = autoIndentWidth * fontSize（autoIndentWidth 语义是"字符宽度"）。
 * 提取以降低 [WritingPaneEditorContent] 行数与认知复杂度。
 */
@Composable
private fun rememberEditorTextStyle(settings: EditorSettingsState): androidx.compose.ui.text.TextStyle {
    val fontSizeSp =
        androidx.compose.ui.unit.TextUnit(
            settings.fontSize,
            androidx.compose.ui.unit.TextUnitType.Sp,
        )
    val lineHeightSp =
        androidx.compose.ui.unit.TextUnit(
            settings.fontSize * settings.lineSpacingMultiplier,
            androidx.compose.ui.unit.TextUnitType.Sp,
        )
    // 首行缩进 — autoIndentEnabled=false 时不缩进。
    val textIndent =
        if (settings.autoIndentEnabled) {
            androidx.compose.ui.text.style.TextIndent(
                firstLine =
                    androidx.compose.ui.unit.TextUnit(
                        settings.autoIndentWidth * settings.fontSize,
                        androidx.compose.ui.unit.TextUnitType.Sp,
                    ),
                restLine =
                    androidx.compose.ui.unit.TextUnit(
                        0f,
                        androidx.compose.ui.unit.TextUnitType.Sp,
                    ),
            )
        } else {
            null
        }
    return androidx.compose.ui.text.TextStyle(
        fontSize = fontSizeSp,
        lineHeight = lineHeightSp,
        textIndent = textIndent,
    )
}

/**
 * target 创建与输入回调绑定（输入回调经 rememberUpdatedState 始终指向最新 VM）。
 *
 * #644 评论 5462826712 第8节：去掉 content 参数。target 注册只做：
 * 身份/profile/persistent/onEditorApplied/commit/cancel，不再 updateText(content)。
 */
@Composable
private fun rememberWritingPaneTarget(
    currentViewModel: EditorViewModel,
    coordinator: com.xiwei.sujian.feature.editor.window.EditorWindowHost,
    targetId: String,
): EditableTextTarget {
    val target =
        remember(targetId) {
            EditableTextTarget(targetId = targetId)
        }
    target.onEditorApplied = { event ->
        currentViewModel.onEditorApplied(event)
    }
    target.onCommit = { }
    target.onCancel = {}
    target.updateProfile(TextEditorProfile.DocumentBody)
    target.updatePersistent(true)

    DisposableEffect(targetId) {
        coordinator.registerTarget(target)
        onDispose {
            coordinator.detachWindowBinding(coordinator.windowId, targetId)
        }
    }
    return target
}

/** 编辑器附着所需的可观察状态（正文/会话/章节身份）。 */
private data class EditorAttachInputs(
    val uiState: com.xiwei.sujian.feature.editor.presentation.EditorUiState,
    val sessionState: com.xiwei.sujian.feature.editor.session.EditorSessionState,
    val chapter: ChapterRef,
)

/** 编辑器附着（beginEdit / confirmEditorAttached）。
 * #624 评论9：WritingPaneTargetTextSync 已删除 — sessionState.text 已不存在，
 * 不再比较 `sessionState.text == uiState.content`；本地输入正文同步由
 * onEditorApplied 增量处理，target.updateText(content) 只在 attach 冷路径执行。 */
@Composable
private fun WritingPaneEditorAttachSync(
    currentViewModel: EditorViewModel,
    coordinator: com.xiwei.sujian.feature.editor.window.EditorWindowHost,
    targetId: String,
    inputs: EditorAttachInputs,
) {
    WritingPaneEditorAttach(currentViewModel, coordinator, targetId, inputs)
}

/**
 * #644 评论 5462826712 第3节：编辑器附着决策 — 纯函数，只保留 BeginEdit 和 Hold。
 *
 * - [EditorAttachAction.BeginEdit]：Idle/Detached/Attaching 或 Attached 属于别的 window/target；
 * - [EditorAttachAction.Hold]：当前 window + target 已经 Attaching/Attached，或 Committing/Cancelling/Detaching。
 *
 * 删除 Confirm 和"等待 AndroidView factory"的 Wait 语义。
 * Attached 后解除冻结已经由 surface-ready 回调和 lease 完成。
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
 * #630 评论 5329388516: WritingPane showEditor 门槛 — 纯函数。
 *
 * 只有 loading = false 且 settingsReady = true 且当前章节匹配（isCurrentChapter）
 * 才进入正文 Surface；持久化设置没读完时继续显示现有 loading UI，
 * 不提前进入正文 Surface（不会先被 ReadonlyChapterPreview 顶一帧）。
 */
internal fun shouldShowEditorForWritingPane(
    loading: Boolean,
    settingsReady: Boolean,
    isCurrentChapter: Boolean,
): Boolean = !loading && settingsReady && isCurrentChapter

/**
 * #630 评论 5327560790: BeginEdit 门槛 — 纯函数。
 *
 * 只有 loading = false 且 settingsReady = true 才允许构造 [EditorTypography] 并 beginEdit，
 * 不用"默认值恰好存在"冒充已加载完成。
 */
internal fun shouldBeginEditForEditorAttach(
    loading: Boolean,
    settingsReady: Boolean,
): Boolean = !loading && settingsReady

/**
 * #630 评论 5327560790: 从持久化 [EditorSettingsState] 构造首帧 [EditorTypography] — 纯函数。
 *
 * 首帧排版参数直接来自持久化权威设置，不再靠默认值或 ON_RESUME 后二次重排。
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

/**
 * #644 评论 5462826712 第3节：编辑器附着 — 用 [editorAttachDecision] 纯函数决策。
 *
 * 只保留 BeginEdit 和 Hold 两类动作。
 * Attached 后解除冻结已经由 surface-ready 回调和 lease 完成，
 * 不要再由另一个 LaunchedEffect 第二次 confirm。
 */
@Composable
private fun WritingPaneEditorAttach(
    currentViewModel: EditorViewModel,
    coordinator: com.xiwei.sujian.feature.editor.window.EditorWindowHost,
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
                // 当前 window + target 已经 Attaching/Attached，或 Committing/Cancelling/Detaching。
            }
        }
    }
}

/** #595 一：显式 Factory 注入进程级容器依赖 + 会话层协调器 — 不再退回
 * ProjectRepository(getApplication()) 创建第二份容器。 */
@Composable
private fun rememberWritingPaneViewModel(
    context: android.content.Context,
    deps: com.xiwei.sujian.app.di.SujianAppDependencies,
    coordinator: com.xiwei.sujian.feature.editor.window.EditorWindowHost,
): EditorViewModel {
    val viewModel: EditorViewModel =
        viewModel(
            factory =
                EditorViewModel.Factory(
                    context.applicationContext as android.app.Application,
                    deps,
                    coordinator.sessionCoordinator,
                ),
        )
    LaunchedEffect(Unit) {
        viewModel.initialize(
            deps.projectRepository,
            deps.settingsRepository,
            deps.syncRepository,
            deps.syncStatusRepository,
            coordinator.sessionCoordinator,
            appServiceBridge = deps.appServiceBridge,
        )
    }
    return viewModel
}

/** 生产动画链：设置状态 → Editor Host → 输入事务 → 动画协调器 → 真实 VSync 渲染。 */
@Composable
private fun WritingPaneMotionPolicySync(
    coordinator: com.xiwei.sujian.feature.editor.window.EditorWindowHost,
    settings: EditorSettingsState,
    chapterId: String,
) {
    LaunchedEffect(settings, chapterId) {
        // #595 三: 走 applyMotionPolicy 原子应用文字、光标、协同、时长和 reduce-motion。
        coordinator.applyMotionPolicy(
            com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy(
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

/** 排版设置链：字号/行距/首行缩进设置 → Editor Host → 当前共享编辑器 View。
 * 设置变化后当前正文立即重排，不重建编辑 session（#624 评论3）。
 *
 * #632 评论 5378239827 项3: 去掉 chapterId key — applyEditorTypography 已幂等
 * （EditorTypography data class == 比较），切章不重排排版设置；
 * LaunchedEffect 只依赖排版参数本身。 */
@Composable
private fun WritingPaneTypographySync(
    coordinator: com.xiwei.sujian.feature.editor.window.EditorWindowHost,
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
private fun WritingPaneSettingsReload(
    viewModel: EditorViewModel,
    targetId: String,
) {
    val lifecycleOwner = androidx.lifecycle.compose.LocalLifecycleOwner.current
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

/**
 * 章节切换收口：深链/恢复路径（currentSession 不是本 pane 章节）走 switchChapter 事务。
 * #624 评论14 第2项：failedSwitchTarget 已删除 — switchLoadAndPrepare 不提前发布 B，
 * isCurrentChapter 守卫已足够；onChapterSwitchFailed 回调保留用于保存/加载失败回滚导航。
 */
@Composable
private fun rememberChapterSwitchSync(
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
                        // #595 一：只有保存+加载+session 预准备都成功，旧章节才
                        // 由事务提交（commitPreparedSession 保留其持久 session）；
                        // 窗口解绑由 DisposableEffect onDispose 完成。
                    }
                    is com.xiwei.sujian.feature.editor.presentation.ChapterSwitchResult.SaveFailed,
                    is com.xiwei.sujian.feature.editor.presentation.ChapterSwitchResult.LoadFailed,
                    -> {
                        // #595 一：保存/加载失败 → 回滚工作区选择到旧章节（activeChapterKey）。
                        onChapterSwitchFailed?.invoke(
                            lastProjectId.takeIf { it.isNotEmpty() } ?: chapter.projectId,
                            lastVolumeId.takeIf { it.isNotEmpty() },
                            lastChapterId.takeIf { it.isNotEmpty() },
                            lastChapterTitle,
                        )
                    }
                    com.xiwei.sujian.feature.editor.presentation.ChapterSwitchResult.Stale -> {
                        // #595 一：请求已过期 — 更新的请求正在完成切换，本请求不再动作。
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

/** 章节引用 — 当前 pane 的章节身份（用于切换同步与附着判断）。 */
private data class ChapterRef(val projectId: String, val volumeId: String, val chapterId: String, val title: String)

/**
 * #595 一/二：外部文档事实（RepositoryLoaded/SyncMerged）— 调用方已通过
 * shouldApplyExternalContent 确认版本更新与本地 dirty 状态，此处只执行
 * Core reset 和 UI 同步，不再重复构造事件做检查。
 */
@Composable
private fun WritingPaneExternalContentFlow(
    viewModel: EditorViewModel,
    coordinator: com.xiwei.sujian.feature.editor.window.EditorWindowHost,
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
 * #624 评论13 第4项：suspend — 与 [com.xiwei.sujian.feature.editor.presentation.EditorViewModel.applyExternalContentToUi]
 * （await calculateWordCount）同一调用链；本来就在 LaunchedEffect collect 里调用。
 */
private suspend fun handleExternalDocumentFact(
    coordinator: com.xiwei.sujian.feature.editor.window.EditorWindowHost,
    viewModel: EditorViewModel,
    targetId: String,
    fact: com.xiwei.sujian.feature.editor.session.TargetDocumentFact,
) {
    when (val decision = coordinator.sessionCoordinator.shouldApplyExternalContent(fact)) {
        ExternalContentDecision.Apply ->
            applyExternalDocumentFact(coordinator, viewModel, targetId, fact)
        ExternalContentDecision.IgnoreSameContent -> {
            // 正文已一致 — 只记录版本事实（幂等）。
            coordinator.sessionCoordinator.applyExternalContentFact(fact)
            // #624 评论17 问题3：IgnoreSameContent 提交版本后清除未解决事实。
            coordinator.sessionCoordinator.consumePendingExternalFact(targetId)
        }
        ExternalContentDecision.IgnoreDirtyConflict -> {
            // #624 评论17 问题3/5：保存未解决事实到 pendingExternal（只存
            // sourceVersion + origin，不缓存 fact.text），不得只发 UI 错误后丢掉。
            // 本地保存清 dirty 后据 sourceVersion/origin 重新从 Repository 读最新
            // 正文/hash 走 shouldApplyExternalContent，不拿缓存的旧正文覆盖。
            coordinator.sessionCoordinator.storePendingExternalFact(targetId, fact)
            if (fact.origin == com.xiwei.sujian.feature.editor.session.DocumentFactOrigin.SYNC_MERGED) {
                viewModel.notifySyncMergeConflict()
            }
        }
        ExternalContentDecision.IgnoreReplay,
        ExternalContentDecision.IgnoreOlder,
        -> consumePendingForReapplyIfApplicable(coordinator, targetId, decision, fact)
        ExternalContentDecision.IgnoreEmptyVersion -> {
            // 无版本锚点 — 忽略，不消费 pending。
        }
        ExternalContentDecision.IgnoreUncomparableConflict -> {
            // #624 评论17 问题3/5：保存未解决事实到 pendingExternal（只存
            // sourceVersion + origin，不缓存 fact.text）。
            // #595 五：不同版本但不可比较（无共同 revision 锚点/父链）—
            // 不得盲目覆盖；进入重新读取/(三方合并/冲突路径（类型化通知）。
            coordinator.sessionCoordinator.storePendingExternalFact(targetId, fact)
            if (fact.origin == com.xiwei.sujian.feature.editor.session.DocumentFactOrigin.SYNC_MERGED) {
                viewModel.notifySyncMergeConflict()
            }
        }
    }
}

/**
 * #595 一/二：Apply 分支执行 — Core reset + 会话事实提交 + bridge 同步 + ViewModel 同步。
 * 从 [handleExternalDocumentFact] 提取以控制 Cognitive Complexity。
 *
 * #641 评论1 第2节：composition 活跃时不得覆盖 TextFieldState。
 * 先检测 ViewModel-owned bridge 的 composition；composition 活跃时只调用既有
 * [storePendingExternalFact]/冲突通知并返回，不 reset、不覆盖 TextFieldState；
 * composition 结束后的 snapshot/既有链再应用 pending。
 */
private suspend fun applyExternalDocumentFact(
    coordinator: com.xiwei.sujian.feature.editor.window.EditorWindowHost,
    viewModel: EditorViewModel,
    targetId: String,
    fact: com.xiwei.sujian.feature.editor.session.TargetDocumentFact,
) {
    // #641：composition 活跃时暂存外部事实，等 composition 结束后再应用。
    // 不得在 IME 正在编辑 buffer 时 reset Core 并覆盖 TextFieldState。
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
    // #595 五：仅 Core reset 成功才一次性提交会话事实与 UI —
    // reset 失败时保持旧正文与旧版本，不得推进任何状态（旧实现
    // 无条件推进导致 Rust session/SessionStore/ViewModel 三份分裂）。
    coordinator.sessionCoordinator.applyExternalContentFact(fact)
    // #624 评论17 问题3：真正 Apply 提交版本后清除未解决事实。
    coordinator.sessionCoordinator.consumePendingExternalFact(targetId)
    // #641 评论1 第2节：外部权威正文写回 bridge state（UTF-8→UTF-16）。
    // composition 已由上方守卫处理，此处 bridge 一定不在 composing 态。
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
        // #595 三：同步合并同时更新 ViewModel 正文/hash/保存状态/字数 —
        // 磁盘、Rust session、ViewModel 三方保持一致。
        viewModel.applyExternalContentToUi(targetId, fact.text, fact.sourceVersion.contentHash)
    }
}

/**
 * #624 评论17 问题5：reapply fact 的 IgnoreReplay/IgnoreOlder 消费 pending
 * （外部状态已对齐/本地更新，冲突已解决）。正常 fact 不消费（可能消费无关 pending）。
 * 提取为独立函数以控制 [handleExternalDocumentFact] 的 Cognitive Complexity。
 */
private fun consumePendingForReapplyIfApplicable(
    coordinator: com.xiwei.sujian.feature.editor.window.EditorWindowHost,
    targetId: String,
    decision: ExternalContentDecision,
    fact: com.xiwei.sujian.feature.editor.session.TargetDocumentFact,
) {
    if (shouldConsumePendingAfterFact(decision, fact.isReapply)) {
        coordinator.sessionCoordinator.consumePendingExternalFact(targetId)
    }
}

/**
 * #641 评论1 第2节：[EditorTextFieldStateBridge] 现由 [EditorViewModel] 拥有
 * （[EditorViewModel.bridgeForTarget]），按 targetId 生命周期稳定存活，
 * 不每次重组重建。初始 selection 来自 Core/session UTF-8 → UTF-16。
 * commitToCore lambda 内统一 UTF-16→UTF-8 / UTF-8→UTF-16 偏移转换。
 *
 * #641 评论 问题2：把 Core [VisualIntent]（UTF-8 byte ranges）转成 Compose [EditorVisualIntent]（UTF-16 ranges）。
 *
 * 映射规则：
 * - textKind：Insert/Delete/Move/None（根据 Core visualIntent 的 operationKind）；
 * - cursor：只要 Core 的 [CoordinatedCursor.shouldAnimate] 为 true，
 *   就构造 [CursorVisualIntent]（animate = true），不管 textKind 是什么；
 * - oldRanges：从 Core oldAffectedByteRanges 转成 UTF-16（用完整 oldText）；
 * - newRanges：从 Core newAffectedByteRanges 转成 UTF-16（用完整 newText）。
 *
 * 注意：Core 的 visual intent 已经区分了 old/new affected ranges。
 * 传入完整 oldText 和 newText 确保 CJK/emoji/多行上的 byte→UTF-16 换算正确。
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

    // oldRanges：从 Core oldAffectedByteRanges 转成 UTF-16（用完整 oldText）。
    val oldRanges =
        event.visualIntent.oldAffectedByteRanges.map { (start: Int, end: Int) ->
            TextOffsetUtils.utf16TextRangeForUtf8(event.oldText, start, end)
        }

    // newRanges：从 Core newAffectedByteRanges 转成 UTF-16（用完整 newText）。
    val newRanges =
        event.visualIntent.newAffectedByteRanges.map { (start: Int, end: Int) ->
            TextOffsetUtils.utf16TextRangeForUtf8(event.newText, start, end)
        }

    // #641 评论 问题2 + 评论 5457777142 问题1：只要 Core 的 coordinated cursor 要动画，
    // 就构造 CursorVisualIntent。byte→UTF-16 换算必须用 `utf16OffsetForUtf8ByteOrNull`：
    // 若返回 null（byte offset 越界或落在多字节字符中间），**不**把 UTF-8 byte offset
    // 当成 UTF-16 offset fallback（那会错位），而是直接放弃这次视觉光标（cursor = null）。
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

    // #641 评论 5458880786 问题2b：用 oldText/newText 做 code-point-safe diff 算 replaceBounds —
    // retained reflow 用确定边界算 prefix/suffix，不再从空 oldRanges/newRanges 猜。
    val replaceBounds = computeVisualReplaceBounds(event.oldText, event.newText)

    return EditorVisualIntent(
        oldRanges = oldRanges,
        newRanges = newRanges,
        textKind = textKind,
        cursor = cursor,
        newTextLength = event.newText.length,
        // #641 评论 5459531909 第1项：传完整新正文，layout 关联改用正文一致判断，
        // 不再只看长度（i→W、候选等长替换、自动纠错长度相同但布局可能不同）。
        expectedNewText = event.newText,
        replaceBounds = replaceBounds,
    )
}

/**
 * #641 评论 5458880786 问题2b：用 oldText/newText 做 code-point-safe diff 算 [VisualReplaceBounds] —
 * 共同前缀 + 共同后缀算出最小 replace 边界，offset 是 UTF-16。
 *
 * 和 [com.xiwei.sujian.feature.editor.input.EditorTextFieldStateBridge] 里 computeReplaceBounds
 * 同样的 codePointAt/codePointBefore + charCount 逻辑，保证 emoji/supplementary plane 字符
 * （surrogate pair）不会被拆在 high/low 中间。
 * retained mapping 固定：prefix 0..oldStart ↔ 0..newStart，
 * suffix oldEnd..oldText.length ↔ newEnd..newText.length。
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

/**
 * #641：收集 Core 视觉意图事件，映射为 [EditorVisualIntent] 喂给 [ComposeEditorVisualState]。
 * 按 target 过滤，避免其他 target 的视觉意图污染当前 overlay。
 *
 * #641 评论 问题3 + 评论 5457777142 问题4：收集
 * [com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy] 的 effective 策略
 * 传给 [ComposeEditorVisualState.onVisualIntent] —
 * 不再写死 200ms，也不再提前压成一个 `durationMillis`。
 * overlay 根据 [com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy.coordinated]
 * 决定一条还是两条 timeline：
 * - coordinated=true：一个 timeline（textDurationMillis），cursor 共用；
 * - coordinated=false：textProgress + cursorProgress 两个 timeline；
 * - CURSOR_ONLY：单独用 cursorDurationMillis。
 * reduceMotion / textEnabled / cursorEnabled 也在 overlay 那一层一次性落实。
 *
 * 提取为独立 composable 以降低 [WritingPaneEditorContent] 的认知复杂度。
 */
@Composable
private fun CollectVisualIntentEvents(
    viewModel: EditorViewModel,
    targetId: String,
    visualState: ComposeEditorVisualState,
    coordinator: com.xiwei.sujian.feature.editor.window.EditorWindowHost,
) {
    val motionPolicy by coordinator.motionPolicyFlow.collectAsStateWithLifecycle()
    val currentMotionPolicy by rememberUpdatedState(motionPolicy)
    androidx.compose.runtime.LaunchedEffect(viewModel, targetId) {
        viewModel.visualIntentEvents
            .collect { event ->
                if (event.targetId != targetId) return@collect
                val editorVisualIntent = mapCoreVisualIntentToEditorVisualIntent(event)
                // #641 评论 5457777142 问题4 + 评论 5458283021 问题3c：
                // 把原始 EditorMotionPolicy 传给 onVisualIntent，
                // onVisualIntent 内部调用 effective() 算 hasTextAnimation/hasCursorAnimation，
                // 提前落实到视觉状态（hiddenRanges/drawsVisualCursor）。
                visualState.onVisualIntent(editorVisualIntent, currentMotionPolicy)
            }
    }
}

/**
 * #641 评论 问题7d + 评论 5457777142 问题5 + 评论 5458283021 问题4：收集 undo/redo 后的权威编辑器快照，把正文写回 bridge。
 *
 * performUndo/performRedo 在 applyUndoRestored 后查询 Core snapshot 并发布到
 * [com.xiwei.sujian.feature.editor.window.EditorWindowHost.authoritativeEditorSnapshots]。
 *
 * #641 评论 5458283021 问题4：单一串行 collector —
 * 不再用两个独立 collector 竞争。pending authoritative 事实收进
 * [com.xiwei.sujian.feature.editor.input.EditorTextFieldStateBridge]，
 * 由同一个 `snapshotFlow { EditorInputSnapshot }.collect(bridge::onInputSnapshot)` 决定顺序：
 * 1. composition != null：不提交（IME 正在编辑 buffer），暂存 pending 到 bridge。
 * 2. composition 刚结束且存在 pending：bridge.onInputSnapshot 先 applyAuthoritativeText 消费 pending，不提交本地 diff。
 * 3. 没 pending 才 commitIfNeeded。
 * 删除了独立的 `snapshotFlow { bridge.state.composition }.collect` collector，
 * 避免两个 collector 同时唤醒无顺序保证导致刚撤销内容又提交一次。
 *
 * 提取为独立 composable 以降低 [WritingPaneEditorContent] 的认知复杂度。
 */
@Composable
@Suppress("CognitiveComplexMethod")
private fun CollectAuthoritativeEditorSnapshots(
    coordinator: com.xiwei.sujian.feature.editor.window.EditorWindowHost,
    viewModel: EditorViewModel,
    targetId: String,
    bridge: com.xiwei.sujian.feature.editor.input.EditorTextFieldStateBridge,
) {
    // 收到 undo/redo snapshot：composition==null 立即应用，composition!=null 暂存到 bridge。
    // #641 评论 5458283021 问题4：pending 存到 bridge（而非 viewModel），
    // 由单一 onInputSnapshot 串行入口消费，不再用独立 composition collector 竞争。
    androidx.compose.runtime.LaunchedEffect(targetId) {
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
                    // composition 活跃：暂存 pending 到 bridge（UTF-8→UTF-16 转换）。
                    val utf16Selection =
                        com.xiwei.sujian.feature.editor.input.TextOffsetUtils.utf16TextRangeForUtf8(
                            snapshot.text,
                            snapshot.selectionAnchorUtf8,
                            snapshot.selectionHeadUtf8,
                        )
                    bridge.storePendingAuthoritative(snapshot.text, utf16Selection)
                }
            }
    }
}
