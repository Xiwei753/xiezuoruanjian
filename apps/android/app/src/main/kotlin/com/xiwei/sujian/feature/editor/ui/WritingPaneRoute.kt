package com.xiwei.sujian.feature.editor.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.Checkbox
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import com.xiwei.sujian.R
import com.xiwei.sujian.app.di.LocalSujianAppDependencies
import com.xiwei.sujian.feature.editor.input.TextOffsetUtils
import com.xiwei.sujian.feature.editor.presentation.CorrectionResourceFeedbackLevel
import com.xiwei.sujian.feature.editor.presentation.EditorCorrectionResourceState
import com.xiwei.sujian.feature.editor.presentation.EditorViewModel
import com.xiwei.sujian.feature.editor.presentation.confirmEditorAttached
import com.xiwei.sujian.feature.editor.presentation.isCurrentChapter
import com.xiwei.sujian.feature.editor.presentation.onEditorApplied
import com.xiwei.sujian.feature.editor.session.EditorCorrectionIgnoreScope
import com.xiwei.sujian.feature.editor.session.EditorCorrectionSuggestion
import com.xiwei.sujian.feature.editor.session.TextEditorProfile
import com.xiwei.sujian.feature.editor.visual.ComposeEditorVisualState
import com.xiwei.sujian.feature.editor.window.EditableTextTarget
import kotlinx.coroutines.flow.collectLatest

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
 *
 * Issue #737：保留平台自动换行 — 不引入软换行占位字符（U+200B）或 projection。
 * [rememberEditorTextStyle] 只负责产品需要的 line break / CJK 规则。
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
    WritingPaneCorrectionHintCollector(viewModel, context)

    val targetId =
        remember(projectId, volumeId, chapterId) {
            "chapter-body:$projectId:$volumeId:$chapterId"
        }

    val uiState by viewModel.uiState.collectAsStateWithLifecycle()

    WritingPaneMotionPolicySync(coordinator, uiState.settings)
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
    val correctionState by viewModel.correctionState.collectAsStateWithLifecycle()
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
            projectId = projectId,
            chapterId = chapterId,
            targetId = targetId,
            isActivePane = isActivePane,
            sessionState = sessionState,
            correctionState = correctionState,
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
    projectId: String,
    chapterId: String,
    targetId: String,
    isActivePane: Boolean,
    sessionState: com.xiwei.sujian.feature.editor.session.EditorSessionState,
    correctionState: com.xiwei.sujian.feature.editor.session.EditorCorrectionState,
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
            val correctionResources by viewModel.correctionResourceState.collectAsStateWithLifecycle()
            var showCorrectionResources by rememberSaveable(targetId) { mutableStateOf(false) }
            var authorizedToUseResources by rememberSaveable(targetId) { mutableStateOf(false) }
            val modelPicker =
                rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
                    if (uri != null && authorizedToUseResources) viewModel.importCorrectionModel(uri)
                }
            val dictionaryPicker =
                rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
                    if (uris.isNotEmpty() && authorizedToUseResources) {
                        viewModel.importCorrectionDictionaries(uris)
                    }
                }

            SetupCorrectionRequests(viewModel, targetId, bridge)

            val projection = coordinator.sessionCoordinator.getProjectionSnapshot(targetId)
            val viewportState =
                com.xiwei.sujian.feature.editor.layout.rememberEditorViewportState(
                    targetId = targetId,
                    initialAnchor = projection?.viewportAnchor,
                )

            val visualState =
                remember(targetId) {
                    ComposeEditorVisualState(
                        targetId = targetId,
                    )
                }

            // #691 评论 5679815971 问题1 / Issue #732 评论 5763493968 第1节：
            // 统一设置同步入口 — 设置 → EditorSessionCoordinator.motionPolicyFlow 唯一写入口
            // 由 WritingPaneMotionPolicySync 完成；这里只把 coordinator 的 policy 流绑定到
            // visualState（collect 一次 → updateMotionPolicy），不再并行调 applyMotionPolicyAtFrame。
            // policy 切换、patch 消费、motion 创建全部走 EditorTextFieldDrawLayer 的同一只 frame clock。
            BindMotionPolicyToVisualState(coordinator, visualState)

            SetupViewportSnapshot(targetId, viewportState, coordinator)

            SetupInputSnapshotCollector(bridge, visualState)

            SetupCommittedTextCollector(
                targetId = targetId,
                coordinator = coordinator,
                viewModel = viewModel,
                bridge = bridge,
            )

            CollectAuthoritativeEditorSnapshots(
                coordinator = coordinator,
                viewModel = viewModel,
                targetId = targetId,
                bridge = bridge,
            )

            CollectEditFactEvents(
                viewModel = viewModel,
                targetId = targetId,
                visualState = visualState,
                coordinator = coordinator,
            )

            val searchHighlightsUtf16 = collectSearchHighlightsUtf16(coordinator, targetId, bridge)
            val activeCorrectionSuggestions =
                if (correctionState.targetId == targetId &&
                    correctionState.projectId == projectId &&
                    correctionState.chapterId == chapterId &&
                    correctionState.sessionId == sessionState.sessionId &&
                    correctionState.textRevision == sessionState.revision
                ) {
                    correctionState.suggestions
                } else {
                    emptyList()
                }
            val correctionHighlightsUtf16 =
                bridge.state.text.toString().let { text ->
                    activeCorrectionSuggestions.mapNotNull { suggestion ->
                        val range =
                            TextOffsetUtils.utf16TextRangeForUtf8(
                                text,
                                suggestion.startByte,
                                suggestion.endByte,
                            )
                        if (range.collapsed || range.end > text.length) {
                            null
                        } else {
                            com.xiwei.sujian.feature.editor.projection.TextRange(range.start, range.end)
                        }
                    }
                }

            var selectedCorrection by remember(targetId, sessionState.sessionId) {
                mutableStateOf<EditorCorrectionSuggestion?>(null)
            }
            val selected =
                selectedCorrection?.takeIf { it in activeCorrectionSuggestions }
                    ?: activeCorrectionSuggestions.firstOrNull()

            Column(modifier = modifier) {
                CorrectionResourceStatusRow(
                    state = correctionResources,
                    onManage = { showCorrectionResources = true },
                    onCheckChapter = { viewModel.requestCorrectionChapter(targetId) },
                    modifier = Modifier.fillMaxWidth().padding(bottom = 8.dp),
                )
                if (showCorrectionResources) {
                    CorrectionResourcesDialog(
                        state = correctionResources,
                        authorized = authorizedToUseResources,
                        onAuthorizedChange = { authorizedToUseResources = it },
                        onImportModel = { modelPicker.launch(arrayOf("*/*")) },
                        onImportDictionaries = { dictionaryPicker.launch(arrayOf("*/*")) },
                        onDismiss = { showCorrectionResources = false },
                    )
                }
                if (activeCorrectionSuggestions.isNotEmpty() && selected != null) {
                    CorrectionSuggestionPanel(
                        suggestions = activeCorrectionSuggestions,
                        selectedSuggestion = selected,
                        onSelect = { selectedCorrection = it },
                        onReplace = { viewModel.acceptCorrectionSuggestion(targetId, selected) },
                        onIgnoreOnce = {
                            viewModel.ignoreCorrectionSuggestion(
                                targetId,
                                selected,
                                EditorCorrectionIgnoreScope.ONCE,
                            )
                        },
                        onIgnoreChapter = {
                            viewModel.ignoreCorrectionSuggestion(
                                targetId,
                                selected,
                                EditorCorrectionIgnoreScope.CHAPTER,
                            )
                        },
                        onAddDictionary = {
                            viewModel.addCorrectionDictionaryEntry(targetId, selected.original)
                        },
                        onDisableRule = {
                            viewModel.ignoreCorrectionSuggestion(
                                targetId,
                                selected,
                                EditorCorrectionIgnoreScope.RULE,
                            )
                        },
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(bottom = 8.dp),
                    )
                }
                WritingEditorSurface(
                    bridge = bridge,
                    visualState = visualState,
                    viewportState = viewportState,
                    textStyle = rememberEditorTextStyle(uiState.settings),
                    textColor = MaterialTheme.colorScheme.onSurface,
                    cursorColor = MaterialTheme.colorScheme.primary,
                    inputEnabled = inputEnabled,
                    onSurfaceReady = {
                        val lease = coordinator.attachSurface(targetId) ?: return@WritingEditorSurface false
                        viewModel.confirmEditorAttached(targetId, lease)
                        true
                    },
                    searchHighlights = searchHighlightsUtf16,
                    correctionHighlights = correctionHighlightsUtf16,
                    modifier = Modifier.weight(1f),
                )
            }
        }
        EditorSurfaceMode.Preview -> {
            val previewState = coordinator.getChapterPreviewState(targetId)
            if (previewState != null && previewState.text.isNotEmpty()) {
                ReadonlyChapterPreview(previewState = previewState)
            }
        }
    }
}

@Composable
private fun CorrectionResourceStatusRow(
    state: EditorCorrectionResourceState,
    onManage: () -> Unit,
    onCheckChapter: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Card(modifier = modifier) {
        Column(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(2.dp),
        ) {
            val modelName = state.activeModelDisplayName
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(modifier = Modifier.weight(1f)) {
                    Text(
                        text = stringResource(R.string.editor_correction_resource_title),
                        style = MaterialTheme.typography.labelLarge,
                    )
                    Text(
                        text = when {
                            state.modelConfigured && !state.modelFileAvailable ->
                                stringResource(R.string.editor_correction_resource_model_unavailable)
                            state.modelFileAvailable && modelName != null ->
                                stringResource(R.string.editor_correction_resource_model_configured, modelName)
                            state.modelFileAvailable ->
                                stringResource(R.string.editor_correction_resource_model_configured_unknown)
                            else -> stringResource(R.string.editor_correction_resource_missing_model)
                        },
                        style = MaterialTheme.typography.bodySmall,
                    )
                    Text(
                        text = if (state.bundledLexiconAvailable) {
                            stringResource(R.string.editor_correction_resource_bundled_ready)
                        } else {
                            stringResource(R.string.editor_correction_resource_bundled_unavailable)
                        },
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Text(
                        text = stringResource(
                            R.string.editor_correction_resource_dictionary_count,
                            state.dictionaryFiles.size,
                        ),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                TextButton(onClick = onManage, enabled = !state.preparing) {
                    Text(stringResource(R.string.editor_correction_resource_manage))
                }
                Button(onClick = onCheckChapter, enabled = state.bundledLexiconAvailable && !state.preparing) {
                    Text(stringResource(R.string.editor_correction_resource_check_chapter))
                }
            }
            state.feedback?.let { feedback ->
                Text(
                    text = feedback.message,
                    style = MaterialTheme.typography.bodySmall,
                    color = correctionResourceFeedbackColor(feedback.level),
                )
            }
        }
    }
}

@Composable
private fun CorrectionResourcesDialog(
    state: EditorCorrectionResourceState,
    authorized: Boolean,
    onAuthorizedChange: (Boolean) -> Unit,
    onImportModel: () -> Unit,
    onImportDictionaries: () -> Unit,
    onDismiss: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = { if (!state.preparing) onDismiss() },
        title = { Text(stringResource(R.string.editor_correction_resource_title)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(stringResource(R.string.editor_correction_resource_license_help))
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Checkbox(checked = authorized, onCheckedChange = onAuthorizedChange)
                    Text(
                        text = stringResource(R.string.editor_correction_resource_authorization),
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
                Text(
                    text = stringResource(R.string.editor_correction_resource_dictionary_help),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                TextButton(
                    onClick = onImportModel,
                    enabled = authorized && !state.preparing,
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text(stringResource(R.string.editor_correction_resource_import_model))
                }
                TextButton(
                    onClick = onImportDictionaries,
                    enabled = authorized && !state.preparing,
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text(stringResource(R.string.editor_correction_resource_import_dictionaries))
                }
                if (state.preparing) {
                    Text(
                        text = stringResource(R.string.editor_correction_resource_preparing),
                        color = MaterialTheme.colorScheme.primary,
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
                state.feedback?.let { feedback ->
                    Text(
                        text = feedback.message,
                        color = correctionResourceFeedbackColor(feedback.level),
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
            }
        },
        confirmButton = {
            TextButton(onClick = onDismiss, enabled = !state.preparing) {
                Text(stringResource(R.string.action_cancel))
            }
        },
    )
}

@Composable
private fun correctionResourceFeedbackColor(level: CorrectionResourceFeedbackLevel) =
    when (level) {
        CorrectionResourceFeedbackLevel.Success -> MaterialTheme.colorScheme.primary
        CorrectionResourceFeedbackLevel.Warning -> MaterialTheme.colorScheme.tertiary
        CorrectionResourceFeedbackLevel.Error -> MaterialTheme.colorScheme.error
    }

@Composable
private fun CorrectionSuggestionPanel(
    suggestions: List<EditorCorrectionSuggestion>,
    selectedSuggestion: EditorCorrectionSuggestion,
    onSelect: (EditorCorrectionSuggestion) -> Unit,
    onReplace: () -> Unit,
    onIgnoreOnce: () -> Unit,
    onIgnoreChapter: () -> Unit,
    onAddDictionary: () -> Unit,
    onDisableRule: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Card(modifier = modifier) {
        Column(modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp)) {
            Text(
                text = stringResource(R.string.editor_correction_suggestions),
                style = MaterialTheme.typography.labelLarge,
            )
            Row(
                modifier = Modifier.horizontalScroll(rememberScrollState()),
            ) {
                suggestions.forEach { suggestion ->
                    val isSelected = suggestion == selectedSuggestion
                    Surface(
                        modifier = Modifier
                            .padding(end = 6.dp, top = 6.dp, bottom = 4.dp)
                            .clickable { onSelect(suggestion) },
                        shape = MaterialTheme.shapes.small,
                        color = if (isSelected) {
                            MaterialTheme.colorScheme.secondaryContainer
                        } else {
                            MaterialTheme.colorScheme.surfaceVariant
                        },
                    ) {
                        Text(
                            text = "${suggestion.original} → ${suggestion.replacement}",
                            modifier = Modifier.padding(horizontal = 10.dp, vertical = 6.dp),
                            style = MaterialTheme.typography.labelMedium,
                            color = if (isSelected) {
                                MaterialTheme.colorScheme.onSecondaryContainer
                            } else {
                                MaterialTheme.colorScheme.onSurfaceVariant
                            },
                        )
                    }
                }
            }
            Text(
                text = "${selectedSuggestion.original} → ${selectedSuggestion.replacement}",
                style = MaterialTheme.typography.titleSmall,
            )
            Text(
                text = stringResource(
                    R.string.editor_correction_reason,
                    localizedCorrectionSource(selectedSuggestion.source),
                    localizedCorrectionRule(selectedSuggestion.ruleId),
                ),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            CorrectionSeverityBadge(selectedSuggestion.severity)
            Spacer(modifier = Modifier.height(2.dp))
            Row(modifier = Modifier.horizontalScroll(rememberScrollState())) {
                TextButton(onClick = onReplace) {
                    Text(stringResource(R.string.editor_correction_replace))
                }
                TextButton(onClick = onIgnoreOnce) {
                    Text(stringResource(R.string.editor_correction_ignore_once))
                }
                TextButton(onClick = onIgnoreChapter) {
                    Text(stringResource(R.string.editor_correction_ignore_chapter))
                }
            }
            Row(modifier = Modifier.horizontalScroll(rememberScrollState())) {
                TextButton(onClick = onAddDictionary) {
                    Text(stringResource(R.string.editor_correction_add_dictionary))
                }
                TextButton(onClick = onDisableRule) {
                    Text(stringResource(R.string.editor_correction_disable_rule))
                }
            }
        }
    }
}

@Composable
private fun CorrectionSeverityBadge(severity: String) {
    val (containerColor, contentColor) =
        when (severity) {
            "error" -> MaterialTheme.colorScheme.errorContainer to MaterialTheme.colorScheme.onErrorContainer
            "warning" -> MaterialTheme.colorScheme.tertiaryContainer to MaterialTheme.colorScheme.onTertiaryContainer
            "hint" -> MaterialTheme.colorScheme.secondaryContainer to MaterialTheme.colorScheme.onSecondaryContainer
            else -> MaterialTheme.colorScheme.surfaceVariant to MaterialTheme.colorScheme.onSurfaceVariant
        }
    Surface(shape = MaterialTheme.shapes.small, color = containerColor) {
        Text(
            text = localizedCorrectionSeverity(severity),
            modifier = Modifier.padding(horizontal = 8.dp, vertical = 3.dp),
            color = contentColor,
            style = MaterialTheme.typography.labelSmall,
        )
    }
}

@Composable
private fun localizedCorrectionSource(source: String): String =
    when (source) {
        "confusion_word" -> stringResource(R.string.editor_correction_source_confusion_word)
        "proper_name" -> stringResource(R.string.editor_correction_source_proper_name)
        "oov" -> stringResource(R.string.editor_correction_source_oov)
        "character_ngram" -> stringResource(R.string.editor_correction_source_character_ngram)
        "rule" -> stringResource(R.string.editor_correction_source_rule)
        else -> if (source.isBlank()) stringResource(R.string.editor_correction_unknown) else source
    }

@Composable
private fun localizedCorrectionRule(ruleId: String): String =
    when (ruleId) {
        "confusion.custom" -> stringResource(R.string.editor_correction_rule_confusion_custom)
        "proper.similarity" -> stringResource(R.string.editor_correction_rule_proper_similarity)
        "detector.exact" -> stringResource(R.string.editor_correction_rule_detector_exact)
        "detector.oov" -> stringResource(R.string.editor_correction_rule_detector_oov)
        "detector.char_mad" -> stringResource(R.string.editor_correction_rule_detector_char_mad)
        "detector.candidate" -> stringResource(R.string.editor_correction_rule_detector_candidate)
        "punctuation.repeated" -> stringResource(R.string.editor_correction_rule_punctuation_repeated)
        "character.repeated" -> stringResource(R.string.editor_correction_rule_character_repeated)
        "fixed.should" -> stringResource(R.string.editor_correction_rule_fixed_should)
        "fixed.already" -> stringResource(R.string.editor_correction_rule_fixed_already)
        "particle.dei.urgent" -> stringResource(R.string.editor_correction_rule_particle_dei_urgent)
        "particle.dei.run" -> stringResource(R.string.editor_correction_rule_particle_dei_run)
        "particle.dei.slow" -> stringResource(R.string.editor_correction_rule_particle_dei_slow)
        else -> if (ruleId.isBlank()) stringResource(R.string.editor_correction_unknown) else ruleId
    }

@Composable
private fun localizedCorrectionSeverity(severity: String): String =
    when (severity) {
        "error" -> stringResource(R.string.editor_correction_severity_error)
        "warning" -> stringResource(R.string.editor_correction_severity_warning)
        "hint" -> stringResource(R.string.editor_correction_severity_hint)
        else -> if (severity.isBlank()) stringResource(R.string.editor_correction_unknown) else severity
    }

@Composable
private fun WritingPaneCorrectionHintCollector(
    viewModel: EditorViewModel,
    context: android.content.Context,
) {
    LaunchedEffect(viewModel, context) {
        viewModel.correctionHintEvents.collect { message ->
            android.widget.Toast.makeText(context, message, android.widget.Toast.LENGTH_SHORT).show()
        }
    }
}

/** Debounced check of the sentence containing the caret; suggestions stay tied to the Core revision. */
@Composable
private fun SetupCorrectionRequests(
    viewModel: EditorViewModel,
    targetId: String,
    bridge: com.xiwei.sujian.feature.editor.input.EditorTextFieldStateBridge,
) {
    LaunchedEffect(viewModel, targetId, bridge) {
        androidx.compose.runtime.snapshotFlow {
            CorrectionCaretSnapshot(
                text = bridge.state.text.toString(),
                caretUtf16 = bridge.state.selection.end,
                composing = bridge.state.composition != null,
            )
        }.collectLatest { snapshot ->
            viewModel.clearCorrectionSuggestions(targetId)
            if (snapshot.composing) return@collectLatest
            kotlinx.coroutines.delay(450)
            val range = sentenceRangeAroundCaret(snapshot.text, snapshot.caretUtf16) ?: return@collectLatest
            val startByte = TextOffsetUtils.utf8OffsetForCharIndex(snapshot.text, range.first)
            val endByte = TextOffsetUtils.utf8OffsetForCharIndex(snapshot.text, range.last + 1)
            viewModel.requestCorrectionSegment(
                targetId = targetId,
                startByte = startByte,
                endByte = endByte,
                visibleText = snapshot.text.substring(range.first, range.last + 1),
            )
        }
    }
}

private data class CorrectionCaretSnapshot(
    val text: String,
    val caretUtf16: Int,
    val composing: Boolean,
)

private fun sentenceRangeAroundCaret(text: String, caretUtf16: Int): IntRange? {
    if (text.isEmpty()) return null
    val caret = TextOffsetUtils.safeCharIndex(text, caretUtf16)
    var start = caret
    while (start > 0 && !isCorrectionSentenceBoundary(text[start - 1])) start--
    var end = caret
    while (end < text.length && !isCorrectionSentenceBoundary(text[end])) end++
    if (end < text.length && text[end] != '\n' && text[end] != '\r') end++
    while (start < end && text[start].isWhitespace()) start++
    while (end > start && text[end - 1].isWhitespace()) end--
    return if (start < end) start until end else null
}

private fun isCorrectionSentenceBoundary(char: Char): Boolean =
    char == '。' || char == '！' || char == '？' || char == '!' || char == '?' ||
        char == '.' || char == '\n' || char == '\r'

/**
 * 设置 viewport snapshot 保存 — 提取以降低 [WritingPaneEditorContent] 长度。
 */
@Composable
private fun SetupViewportSnapshot(
    targetId: String,
    viewportState: com.xiwei.sujian.feature.editor.layout.EditorViewportState,
    coordinator: com.xiwei.sujian.feature.editor.window.EditorWindowHost,
) {
    androidx.compose.runtime.DisposableEffect(targetId, viewportState) {
        onDispose {
            val anchor = viewportState.snapshotAnchor()
            coordinator.sessionCoordinator.saveProjectionSnapshot(
                targetId,
                com.xiwei.sujian.feature.editor.projection.ProjectionSnapshot(anchor),
            )
        }
    }
}

/**
 * 设置 input snapshot 收集器 — 提取以降低 [WritingPaneEditorContent] 长度。
 *
 * #694 评论 5693864609 问题2：本地视觉先看到 composition 生命周期，
 * 再由 bridge 把最终正文提交 Core。
 *
 * #694 评论 5694645209 问题1：先调 bridge 拿 [InputSnapshotOutcome]，
 * 再传 visualState.onInputSnapshotResolved —
 * 不再让视觉层在 bridge 之前把 composition == null 当成"commit 已成功"。
 */
@Composable
private fun SetupInputSnapshotCollector(
    bridge: com.xiwei.sujian.feature.editor.input.EditorTextFieldStateBridge,
    visualState: ComposeEditorVisualState,
) {
    androidx.compose.runtime.LaunchedEffect(bridge) {
        androidx.compose.runtime.snapshotFlow {
            com.xiwei.sujian.feature.editor.input.EditorInputSnapshot(
                text = bridge.state.text.toString(),
                selection = bridge.state.selection,
                composition = bridge.state.composition,
            )
        }.collect { snapshot ->
            // #694 评论 5694645209 问题1：先调 bridge 拿 outcome，再传 visualState 收口。
            // bridge 决定是否存在 pending authoritative / Core 是否接受本地 commit，
            // 视觉层根据 outcome 决定是否 finishCompositionCommit / 取消本次 composition local visual state。
            val outcome = bridge.onInputSnapshot(snapshot)
            visualState.onInputSnapshotResolved(snapshot, outcome)
        }
    }
}

/**
 * 设置 committed text 收集器 — 提取以降低 [WritingPaneEditorContent] 长度。
 */
@Composable
private fun SetupCommittedTextCollector(
    targetId: String,
    coordinator: com.xiwei.sujian.feature.editor.window.EditorWindowHost,
    viewModel: com.xiwei.sujian.feature.editor.presentation.EditorViewModel,
    bridge: com.xiwei.sujian.feature.editor.input.EditorTextFieldStateBridge,
) {
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
}

/**
 * 收集 UTF-16 搜索高亮 — 提取以降低 [WritingPaneEditorContent] 长度。
 */
@Composable
private fun collectSearchHighlightsUtf16(
    coordinator: com.xiwei.sujian.feature.editor.window.EditorWindowHost,
    targetId: String,
    bridge: com.xiwei.sujian.feature.editor.input.EditorTextFieldStateBridge,
): List<com.xiwei.sujian.feature.editor.projection.TextRange> {
    val decorations = coordinator.getTargetDecorations(targetId)
    val authoritativeText = bridge.mirroredText
    return decorations?.searchHighlightsUtf8?.map { (start, end) ->
        val r =
            com.xiwei.sujian.feature.editor.input.TextOffsetUtils
                .utf16TextRangeForUtf8(authoritativeText, start, end)
        com.xiwei.sujian.feature.editor.projection.TextRange(r.start, r.end)
    } ?: emptyList()
}

/**
 * #641 评论 问题6d：编辑器 TextStyle 构造 — 字号/行距/首行缩进写进 Compose paragraph/text style。
 */
@Composable
private fun rememberEditorTextStyle(
    settings: com.xiwei.sujian.feature.editor.presentation.EditorSettingsState,
): androidx.compose.ui.text.TextStyle {
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
        // Issue #717 评论 5741910919：显式指定 LineBreak.Simple。
        // 配合 OutputTransformation 在长西文单词内部插入 U+200B，
        // 让换行算法能在这些零宽空格处断行，避免整词推到下一行留下大块空白。
        lineBreak = androidx.compose.ui.text.style.LineBreak.Simple,
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
    target.onCancel = { }
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
