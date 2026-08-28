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
import com.xiwei.sujian.feature.editor.presentation.EditorSettingsState
import com.xiwei.sujian.feature.editor.presentation.EditorViewModel
import com.xiwei.sujian.feature.editor.presentation.applyExternalContentToUi
import com.xiwei.sujian.feature.editor.presentation.confirmEditorAttached
import com.xiwei.sujian.feature.editor.presentation.isCurrentChapter
import com.xiwei.sujian.feature.editor.presentation.notifySyncMergeConflict
import com.xiwei.sujian.feature.editor.presentation.onEditorApplied
import com.xiwei.sujian.feature.editor.presentation.reloadSettings
import com.xiwei.sujian.feature.editor.presentation.shouldConsumePendingAfterFact
import com.xiwei.sujian.feature.editor.session.ExternalContentDecision
import com.xiwei.sujian.feature.editor.session.SessionResetSource
import com.xiwei.sujian.feature.editor.session.TextEditorProfile
import com.xiwei.sujian.feature.editor.session.WindowBindingState
import com.xiwei.sujian.feature.editor.session.applyExternalContentFact
import com.xiwei.sujian.feature.editor.session.applyLocalEdit
import com.xiwei.sujian.feature.editor.session.consumePendingExternalFact
import com.xiwei.sujian.feature.editor.session.shouldApplyExternalContent
import com.xiwei.sujian.feature.editor.session.storePendingExternalFact
import com.xiwei.sujian.feature.editor.window.EditableTextTarget

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
 * #641：正文 UI 由 state-based [BasicTextField] 接管，不再有 presentationVisible —
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
            content = uiState.content,
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
 */
@Composable
private fun WritingPaneEditorContent(
    coordinator: com.xiwei.sujian.feature.editor.window.EditorWindowHost,
    targetId: String,
    isActivePane: Boolean,
    sessionState: com.xiwei.sujian.feature.editor.session.EditorSessionState,
    uiState: com.xiwei.sujian.feature.editor.presentation.EditorUiState,
    modifier: Modifier,
) {
    val deps = LocalSujianAppDependencies.current
    // #624 评论17 第2部分：Route 层收集 targetDecorationsVersionFlow 触发重排，
    // Layout 层不自己 collect session/window flow。
    @Suppress("UNUSED_EXPRESSION")
    (coordinator.targetDecorationsVersionFlow.collectAsStateWithLifecycle().value)
    val surfaceMode = editorSurfaceMode(sessionState.bindingState, coordinator.windowId, targetId, isActivePane)
    when (surfaceMode) {
        EditorSurfaceMode.EditorHost -> {
            val bridge =
                rememberEditorTextFieldBridge(
                    coordinator = coordinator,
                    appServiceBridge = deps.appServiceBridge,
                    targetId = targetId,
                    initialText = uiState.content,
                    bindingState = sessionState.bindingState,
                )
            val visualState = remember { com.xiwei.sujian.feature.editor.visual.ComposeEditorVisualState() }
            WritingEditorSurface(
                bridge = bridge,
                visualState = visualState,
                textStyle =
                    androidx.compose.ui.text.TextStyle(
                        fontSize =
                            androidx.compose.ui.unit.TextUnit(
                                uiState.settings.fontSize,
                                androidx.compose.ui.unit.TextUnitType.Sp,
                            ),
                        lineHeight =
                            androidx.compose.ui.unit.TextUnit(
                                uiState.settings.fontSize * uiState.settings.lineSpacingMultiplier,
                                androidx.compose.ui.unit.TextUnitType.Sp,
                            ),
                    ),
                textColor = androidx.compose.ui.graphics.Color.Black,
                cursorColor = androidx.compose.ui.graphics.Color.Black,
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

/** target 创建与输入回调绑定（输入回调经 rememberUpdatedState 始终指向最新 VM）。 */
@Composable
private fun rememberWritingPaneTarget(
    currentViewModel: EditorViewModel,
    coordinator: com.xiwei.sujian.feature.editor.window.EditorWindowHost,
    targetId: String,
    content: String,
): EditableTextTarget {
    val target =
        remember(targetId) {
            EditableTextTarget(targetId = targetId)
        }
    // #624 评论9：热路径不再传整章 String — onTextChanged/onCommit 已删除，
    // 改用 onEditorApplied 接轻量 EditorAppliedEvent（保存调度/统计/字数增量）。
    target.onEditorApplied = { event ->
        currentViewModel.onEditorApplied(event)
    }
    // commit/cancel 仍由 EditorWindowHost 的 onCommitRequested/onCancelRequested
    // 触发 commitActiveEdit/cancelActiveEdit；target.onCommit/onCancel 不再走
    // 整章 String 热路径。
    target.onCommit = { }
    target.onCancel = {}
    target.updateProfile(TextEditorProfile.DocumentBody)
    target.updatePersistent(true)
    target.updateText(content)

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
 * #624 评论17 问题2：编辑器附着决策 — 纯函数，覆盖所有 WindowBindingState 分支。
 *
 * - [EditorAttachAction.Confirm]：Attached 且 window/target 匹配 → confirmEditorAttached；
 * - [EditorAttachAction.Wait]：Attaching 且 window/target 匹配 → 等待 AndroidView factory；
 * - [EditorAttachAction.BeginEdit]：Idle/Detached 或 Attaching/Attached 属于旧窗口
 *   → beginEdit，让 session 层 restamp 到当前窗口；
 * - [EditorAttachAction.Hold]：Committing/Cancelling/Detaching → 不发起新绑定。
 */
sealed interface EditorAttachAction {
    data object Confirm : EditorAttachAction

    data object Wait : EditorAttachAction

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
                EditorAttachAction.Confirm
            } else {
                EditorAttachAction.BeginEdit
            }
        is WindowBindingState.Attaching ->
            if (bindingState.windowId == windowId && bindingState.targetId == targetId) {
                EditorAttachAction.Wait
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
 * #595 一 / #624 评论17 问题2：编辑器附着 — 用 [editorAttachDecision] 纯函数决策，
 * 覆盖所有 WindowBindingState 分支。
 *
 * #624 评论17 问题2：删除 "prepared" 假窗口后，绑定状态机为
 * Detached → Attaching(realWindowId) → Attached(realWindowId)。
 * - Attached 且 windowId/targetId 都匹配 → Confirm（confirmEditorAttached）；
 * - Attaching 且 windowId/targetId 都匹配 → Wait（等待 attachView 推进到 Attached）；
 * - Attaching/Attached 属于旧 window → BeginEdit（restamp 到新窗口）；
 * - Idle/Detached → BeginEdit；
 * - Committing/Cancelling/Detaching → Hold（等待当前事务结束）。
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
            EditorAttachAction.Confirm -> {
                currentViewModel.confirmEditorAttached(targetId)
            }
            EditorAttachAction.Wait -> {
                // 等待 session 绑定推进到 Attached，不解除冻结。
            }
            EditorAttachAction.BeginEdit -> {
                if (shouldBeginEditForEditorAttach(inputs.uiState.loading, inputs.uiState.settingsReady)) {
                    // #641：正文由 BasicTextField(TextFieldState) 接管，不再 updateTargetText；
                    // beginEdit 只预准备 Rust session，排版由 Compose 层直接应用。
                    coordinator.beginEdit(targetId)
                }
            }
            EditorAttachAction.Hold -> {
                // Committing/Cancelling/Detaching — 不发起新绑定。
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
        ExternalContentDecision.Apply -> {
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
            if (resetResult is com.xiwei.sujian.feature.editor.session.ExternalResetResult.Success) {
                // #595 五：仅 Core reset 成功才一次性提交会话事实与 UI —
                // reset 失败时保持旧正文与旧版本，不得推进任何状态（旧实现
                // 无条件推进导致 Rust session/SessionStore/ViewModel 三份分裂）。
                coordinator.sessionCoordinator.applyExternalContentFact(fact)
                // #624 评论17 问题3：真正 Apply 提交版本后清除未解决事实。
                coordinator.sessionCoordinator.consumePendingExternalFact(targetId)
                if (fact.origin == com.xiwei.sujian.feature.editor.session.DocumentFactOrigin.SYNC_MERGED) {
                    // #595 三：同步合并同时更新 ViewModel 正文/hash/保存状态/字数 —
                    // 磁盘、Rust session、ViewModel 三方保持一致。
                    viewModel.applyExternalContentToUi(targetId, fact.text, fact.sourceVersion.contentHash)
                }
            }
        }
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
 * #641 评论1 第2节：[EditorTextFieldStateBridge] 创建 — 持有 [TextFieldState]，
 * 把 Android 已提交的文本变化转成现有 Core 事务，复用
 * [EditorSessionCoordinator.applyLocalEdit] 的 dirty/revision/autosave/统计链。
 *
 * bridge 按 (targetId, bindingState) remember — session 重新绑定时重建，
 * 外部权威正文（同步/撤销/重载）经 resetPersistentSession 后 session 重附着，
 * bridge 以新正文初始化。不每次重组重建。
 */
@Composable
private fun rememberEditorTextFieldBridge(
    coordinator: com.xiwei.sujian.feature.editor.window.EditorWindowHost,
    appServiceBridge: com.xiwei.sujian.core.interop.app.AppServiceBridge,
    targetId: String,
    initialText: String,
    bindingState: com.xiwei.sujian.feature.editor.session.WindowBindingState,
): com.xiwei.sujian.feature.editor.input.EditorTextFieldStateBridge {
    val commitToCore: (
        com.xiwei.sujian.feature.editor.input.CommittedTextEdit,
    ) -> com.xiwei.sujian.feature.editor.input.CommitResult =
        remember(coordinator, appServiceBridge, targetId) {
            { edit ->
                val lease = coordinator.sessionCoordinator.currentInputLease()
                if (lease == null || lease.targetId != targetId) {
                    com.xiwei.sujian.feature.editor.input.CommitResult.Rejected(edit.oldText, edit.selection)
                } else {
                    val byteStart = edit.oldText.substring(0, edit.replaceStart).toByteArray(Charsets.UTF_8).size
                    val byteEndExclusive =
                        edit.oldText.substring(0, edit.replaceEndExclusive).toByteArray(Charsets.UTF_8).size
                    val kernelBridge =
                        com.xiwei.sujian.feature.editor.interop.TextEditSessionBridge(appServiceBridge, lease.sessionId)
                    val snapshot = coordinator.queryTargetSnapshot(targetId)
                    val expectedRevision = snapshot?.revision ?: 0L
                    val result =
                        kernelBridge.replace(
                            byteStart = byteStart,
                            byteEndExclusive = byteEndExclusive,
                            replacementText = edit.newText,
                            originalText = edit.oldText,
                            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
                            expectedRevision = expectedRevision,
                        )
                    if (result != null) {
                        coordinator.sessionCoordinator.applyLocalEdit(
                            com.xiwei.sujian.feature.editor.session.EditorDocumentUpdate.LocalInput(
                                targetId = targetId,
                                revision = result.newRevision.toLong(),
                                transactionId = result.transactionId.toLong(),
                                selectionAnchorUtf8 = result.newSelectionStart.toInt(),
                                selectionHeadUtf8 = result.newSelectionEnd.toInt(),
                                lease = lease,
                                contentChanged = result.displayPatches.isNotEmpty(),
                            ),
                        )
                        com.xiwei.sujian.feature.editor.input.CommitResult.Accepted
                    } else {
                        val fallbackText = snapshot?.text ?: edit.oldText
                        com.xiwei.sujian.feature.editor.input.CommitResult.Rejected(
                            fallbackText,
                            androidx.compose.ui.text.TextRange(
                                (snapshot?.selectionAnchorUtf8 ?: 0).coerceAtLeast(0),
                                (snapshot?.selectionHeadUtf8 ?: 0).coerceAtLeast(0),
                            ),
                        )
                    }
                }
            }
        }

    val bridge =
        remember(targetId, bindingState) {
            com.xiwei.sujian.feature.editor.input.EditorTextFieldStateBridge(
                initialText = initialText,
                initialSelection = androidx.compose.ui.text.TextRange(0, 0),
                commitToCore = commitToCore,
            )
        }

    // #641 评论1 第2节：观察 TextFieldState 快照，提交已完成的文本变化给 Core。
    // IME composing 中间态不提交（bridge.onInputSnapshot 内部判断）。
    androidx.compose.runtime.LaunchedEffect(bridge) {
        androidx.compose.runtime.snapshotFlow {
            com.xiwei.sujian.feature.editor.input.EditorInputSnapshot(
                text = bridge.state.text.toString(),
                selection = bridge.state.selection,
                composition = bridge.state.composition,
            )
        }.collect(bridge::onInputSnapshot)
    }

    return bridge
}
