package com.xiwei.sujian.feature.editor.ui

import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import com.xiwei.sujian.app.di.LocalSujianAppDependencies
import com.xiwei.sujian.feature.editor.presentation.EditorViewModel
import com.xiwei.sujian.feature.editor.presentation.confirmEditorAttached
import com.xiwei.sujian.feature.editor.presentation.isCurrentChapter
import com.xiwei.sujian.feature.editor.session.TextEditorProfile
import com.xiwei.sujian.feature.editor.session.WindowBindingState
import com.xiwei.sujian.feature.editor.visual.ComposeEditorVisualState
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

    WritingPaneMotionPolicySync(coordinator, uiState.settings, chapterId)
    WritingPaneTypographySync(coordinator, uiState.settings)
    WritingPaneSettingsReload(viewModel, targetId)

    val chapter = ChapterRef(projectId, volumeId, chapterId, chapterTitle)
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

    val isActivePane = currentViewModel.isCurrentChapter(projectId, volumeId, chapterId)
    WritingPaneLayout(
        modifier = modifier,
        uiState = uiState,
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

            val projection = coordinator.sessionCoordinator.getProjectionSnapshot(targetId)
            val viewportState = com.xiwei.sujian.feature.editor.layout.rememberEditorViewportState(
                targetId = targetId,
                initialAnchor = projection?.viewportAnchor,
            )

            val visualState = remember(targetId) { ComposeEditorVisualState() }

            androidx.compose.runtime.DisposableEffect(targetId, viewportState) {
                onDispose {
                    val anchor = viewportState.snapshotAnchor()
                    coordinator.sessionCoordinator.saveProjectionSnapshot(
                        targetId,
                        com.xiwei.sujian.feature.editor.projection.ProjectionSnapshot(anchor),
                    )
                }
            }

            androidx.compose.runtime.LaunchedEffect(bridge) {
                androidx.compose.runtime.snapshotFlow {
                    com.xiwei.sujian.feature.editor.input.EditorInputSnapshot(
                        text = bridge.state.text.toString(),
                        selection = bridge.state.selection,
                        composition = bridge.state.composition,
                    )
                }.collect(bridge::onInputSnapshot)
            }

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
                ReadonlyChapterPreview(previewState = previewState)
            }
        }
    }
}

/**
 * #641 评论 问题6d：编辑器 TextStyle 构造 — 字号/行距/首行缩进写进 Compose paragraph/text style。
 */
@Composable
private fun rememberEditorTextStyle(settings: com.xiwei.sujian.feature.editor.presentation.EditorSettingsState): androidx.compose.ui.text.TextStyle {
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

    androidx.compose.runtime.DisposableEffect(targetId) {
        coordinator.registerTarget(target)
        onDispose {
            coordinator.detachWindowBinding(coordinator.windowId, targetId)
        }
    }
    return target
}

/**
 * #630 评论 5329388516: WritingPane showEditor 门槛 — 纯函数。
 */
internal fun shouldShowEditorForWritingPane(
    loading: Boolean,
    settingsReady: Boolean,
    isCurrentChapter: Boolean,
): Boolean = !loading && settingsReady && isCurrentChapter

/** #595 一：显式 Factory 注入进程级容器依赖 + 会话层协调器。 */
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
    androidx.compose.runtime.LaunchedEffect(Unit) {
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
