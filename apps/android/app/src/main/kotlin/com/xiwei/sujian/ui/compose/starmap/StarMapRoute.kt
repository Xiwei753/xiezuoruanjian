@file:Suppress("StringLiteralDuplication") // #597 技术债：协议字符串天然重复

package com.xiwei.sujian.ui.compose.starmap

import androidx.compose.material3.adaptive.ExperimentalMaterial3AdaptiveApi
import androidx.compose.material3.adaptive.layout.AnimatedPane
import androidx.compose.material3.adaptive.layout.ListDetailPaneScaffold
import androidx.compose.material3.adaptive.layout.ListDetailPaneScaffoldRole
import androidx.compose.material3.adaptive.navigation.BackNavigationBehavior
import androidx.compose.material3.adaptive.navigation.rememberListDetailPaneScaffoldNavigator
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import com.xiwei.sujian.R
import com.xiwei.sujian.data.BridgeResult
import com.xiwei.sujian.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.editor.v2.compose.LocalEditorWindowHost
import com.xiwei.sujian.editor.v2.coordinator.EditorWindowHost
import com.xiwei.sujian.editor.v2.coordinator.EditableTextTarget
import com.xiwei.sujian.editor.v2.coordinator.EditingState
import com.xiwei.sujian.editor.v2.coordinator.TextEditorProfile
import com.xiwei.sujian.model.StarMapGraphNode
import com.xiwei.sujian.model.StarMapNodeKind
import com.xiwei.sujian.ui.compose.navigation.StarMapTopBarState
import com.xiwei.sujian.ui.compose.navigation.predictiveBackStateFraction
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * 星图目的地：列表与编辑是同一个目的地内的窗格状态，由同一个 Material3
 * Adaptive 列表—详情 navigator 管理。
 *
 * - 窄窗口自动单栏（列表或编辑全屏），宽窗口自动双栏（列表+编辑并排）；
 * - 系统返回手势按 BackEvent.progress 真实 seek 窗格过渡（拖动跟手、
 *   取消回原位、提交完成剩余过渡），顶栏返回调用同一 navigator；
 * - 列表层返回交给全局 NavDisplay（Works 常驻栈底，pop 带真实手势进度）。
 * - 数据读取和操作通过 [StarMapViewModel] 完成，旋转/分屏/折叠后状态不丢。
 */
@OptIn(ExperimentalMaterial3AdaptiveApi::class)
@Composable
fun StarMapScreen(
    topBarState: com.xiwei.sujian.ui.compose.navigation.StarMapTopBarState,
    viewModel: StarMapViewModel,
    modifier: Modifier = Modifier
) {
    // 星图列表—编辑窗格共用同一个 Material3 Adaptive 列表—详情 navigator：
    // 不再强制 maxHorizontalPartitions = 1，宽窗口自动双栏。
    val navigator = rememberListDetailPaneScaffoldNavigator<String>()
    val coroutineScope = rememberCoroutineScope()
    val currentStarmapId = navigator.currentDestination?.contentKey

    // 编辑窗格内容在弹栈过渡期间保持上一次打开的星图：contentKey 在 pop 开始
    // 即变为 null，若直接读它，退出动画中编辑内容会瞬间消失（手势结束跳变）；
    // 这里只在向前导航时更新，弹出后由退出动画继续显示旧编辑内容。
    var editorStarmapId by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(currentStarmapId) {
        if (currentStarmapId != null) {
            editorStarmapId = currentStarmapId
        }
    }

    // 预测返回：手势拖动时按 BackEvent.progress seek 窗格过渡；取消回到原位；
    // 提交后播放剩余过渡（navigateBack 挂起至动画完成），动画结束后才复位编辑态
    // 顶栏（标题/返回/操作），无手势结束跳变。
    androidx.activity.compose.PredictiveBackHandler(enabled = navigator.canNavigateBack()) { progressEvents ->
        com.xiwei.sujian.diagnostics.DiagnosticsEvents.predictiveBack("starmap_editor", "start")
        try {
            progressEvents.collect { event ->
                if (event.progress != 0f) {
                    navigator.seekBack(
                        BackNavigationBehavior.PopUntilScaffoldValueChange,
                        predictiveBackStateFraction(event.progress),
                    )
                }
            }
            navigator.navigateBack(BackNavigationBehavior.PopUntilScaffoldValueChange)
            topBarState.clear()
            com.xiwei.sujian.diagnostics.DiagnosticsEvents.workspaceBack("starmap_editor")
        } catch (e: CancellationException) {
            com.xiwei.sujian.diagnostics.DiagnosticsEvents.predictiveBack("starmap_editor", "cancel")
            withContext(NonCancellable) {
                navigator.seekBack(BackNavigationBehavior.PopUntilScaffoldValueChange, 0f)
            }
            throw e
        }
    }

    ListDetailPaneScaffold(
        modifier = modifier,
        directive = navigator.scaffoldDirective,
        scaffoldState = navigator.scaffoldState,
        listPane = {
            AnimatedPane {
                StarMapListPane(
                    viewModel = viewModel,
                    onSelectStarmap = { starmapId ->
                        coroutineScope.launch {
                            navigator.navigateTo(ListDetailPaneScaffoldRole.Detail, starmapId)
                        }
                    },
                    modifier = Modifier.testTag(SujianSemanticIds.StarMapList),
                )
            }
        },
        detailPane = {
            AnimatedPane {
                val starmapId = editorStarmapId
                if (starmapId != null) {
                    StarMapEditorPane(
                        starmapId = starmapId,
                        topBarState = topBarState,
                        viewModel = viewModel,
                        onBack = {
                            coroutineScope.launch {
                                navigator.navigateBack(BackNavigationBehavior.PopUntilScaffoldValueChange)
                                topBarState.clear()
                                com.xiwei.sujian.diagnostics.DiagnosticsEvents.workspaceBack("starmap_editor")
                            }
                        },
                        modifier = Modifier.testTag(SujianSemanticIds.StarMapEditor),
                    )
                }
            }
        },
    )
}

/**
 * 星图列表窗格 — 使用 ViewModel 持有状态。
 */
@Composable
private fun StarMapListPane(
    viewModel: StarMapViewModel,
    onSelectStarmap: (String) -> Unit,
    modifier: Modifier = Modifier
) {
    val coordinator = LocalEditorWindowHost.current
        ?: throw IllegalStateException(
            "StarMapListPane requires an EditorWindowHost. " +
            "Ensure the host Activity provides one via CompositionLocalProvider."
        )

    LaunchedEffect(Unit) {
        viewModel.loadStarMaps()
    }

    StarMapListContent(
        state = viewModel.listState,
        onSelectStarmap = onSelectStarmap,
        onCreateClick = { viewModel.onShowCreateDialog(true) },
        modifier = modifier
    )

    if (viewModel.showCreateDialog) {
        StarMapCreateDialog(
            coordinator = coordinator,
            onConfirm = { title, description ->
                viewModel.createStarmap(title, description)
                viewModel.onShowCreateDialog(false)
            },
            onDismiss = { viewModel.onShowCreateDialog(false) }
        )
    }
}
 @Suppress("CognitiveComplexMethod", "CyclomaticComplexMethod", "LongMethod", "StringLiteralDuplication") // #597 技术债：待重构拆分

/**
 * 星图编辑器窗格 — 使用 ViewModel 持有状态。
 */
@Composable
private fun StarMapEditorPane(
    starmapId: String,
    topBarState: com.xiwei.sujian.ui.compose.navigation.StarMapTopBarState,
    viewModel: StarMapViewModel,
    onBack: () -> Unit,
    modifier: Modifier = Modifier
) {
    val coordinator = LocalEditorWindowHost.current
        ?: throw IllegalStateException(
            "StarMapEditorPane requires an EditorWindowHost. " +
            "Ensure the host Activity provides one via CompositionLocalProvider."
        )
    val coroutineScope = rememberCoroutineScope()

    LaunchedEffect(starmapId) {
        viewModel.loadStarMap(starmapId)
    }

    LaunchedEffect(starmapId, viewModel.editorState.starMapData?.loadPhase, viewModel.editorState.starMapData?.complete) {
        viewModel.advanceLoadPhase(starmapId)
    }

    DisposableEffect(starmapId) {
        onDispose {
            viewModel.flushAndCloseStarmapStore(starmapId)
        }
    }

    StarMapEditorContent(
        state = viewModel.editorState,
        topBarState = topBarState,
        onBack = onBack,
        onAddNodeClick = { viewModel.onShowAddNodeDialog(true) },
        onAddEdgeClick = { viewModel.onShowAddEdgeDialog(true) },
        onNodeDrag = { nodeId, x, y ->
            viewModel.onNodeDrag(starmapId, nodeId, x, y)
        },
        onViewportChange = { viewport ->
            viewModel.onViewportChange(starmapId, viewport)
        },
        onNodeTap = { nodeId ->
            viewModel.onNodeTap(nodeId)
        },
        onNodeDoubleTap = { geometry ->
            val graphNode = viewModel.editorState.starMapData?.graph?.nodes?.find { it.id == geometry.nodeId }
            if (graphNode != null) {
                val targetId = "starmap-node-title:${starmapId}:${geometry.nodeId}"
                val target = EditableTextTarget(targetId = targetId)
                target.updateProfile(TextEditorProfile.CanvasLabel)
                target.updatePersistent(false)
                target.updateText(graphNode.title)
                    target.onCommit = { finalText ->
                        if (finalText.isNotBlank() && finalText.trim() != graphNode.title) {
                            coroutineScope.launch {
                                val ok = viewModel.executeOperation("update_node") {
                                    viewModel.repository.updateStarmapNode(starmapId, geometry.nodeId, title = finalText.trim())
                                }.await()
                                if (ok) {
                                    viewModel.stopEditingNode()
                                    coordinator.detachWindowBinding(coordinator.windowId, targetId)
                                    viewModel.loadStarMap(starmapId)
                                }
                            }
                        } else {
                            viewModel.stopEditingNode()
                            coordinator.detachWindowBinding(coordinator.windowId, targetId)
                        }
                    }
                target.onCancel = {
                        viewModel.stopEditingNode()
                        coordinator.detachWindowBinding(coordinator.windowId, targetId)
                    }
                target.onEditingStateChanged = { state ->
                        if (state == EditingState.IDLE || state == EditingState.RELEASED) {
                            viewModel.stopEditingNode()
                        }
                    }
                coordinator.registerTarget(target)
                coordinator.updateTargetGeometry(targetId, geometry.windowRect)
                coordinator.updateTargetTransform(
                    targetId,
                    com.xiwei.sujian.editor.v2.coordinator.Transform2D(
                        translateX = 0f,
                        translateY = 0f,
                        scaleX = geometry.scale,
                        scaleY = geometry.scale
                    )
                )
                if (coordinator.beginEdit(targetId, graphNode.title.toByteArray(Charsets.UTF_8).size)) {
                    viewModel.startEditingNode(geometry.nodeId)
                }
            }
        },
        onRetrySaves = {
            viewModel.retryPendingSaves(starmapId)
        },
        modifier = modifier
    )

    viewModel.editorState.selectedNodeId?.let { nodeId ->
        val graphNode = viewModel.editorState.starMapData?.graph?.nodes?.find { it.id == nodeId }
        if (graphNode != null) {
            NodeEditPanel(
                node = graphNode,
                coordinator = coordinator,
                onUpdate = { newTitle, newKind ->
                    coroutineScope.launch {
                        val ok = viewModel.executeOperation("update_node") {
                            viewModel.repository.updateStarmapNode(starmapId, nodeId, title = newTitle, kind = newKind)
                        }.await()
                        if (ok) {
                            viewModel.clearNodeSelection()
                            viewModel.loadStarMap(starmapId)
                        }
                    }
                },
                onDelete = {
                    coroutineScope.launch {
                        val ok = viewModel.executeOperation("delete_node") {
                            viewModel.repository.deleteStarmapNode(starmapId, nodeId)
                        }.await()
                        if (ok) {
                            viewModel.clearNodeSelection()
                            viewModel.loadStarMap(starmapId)
                        }
                    }
                },
                onDismiss = { viewModel.clearNodeSelection() }
            )
        }
    }

    if (viewModel.showAddNodeDialog) {
        StarMapAddNodeDialog(
            coordinator = coordinator,
            onConfirm = { title, kind ->
                coroutineScope.launch {
                    val ok = viewModel.executeOperation("add_node") {
                        val nodeId = java.util.UUID.randomUUID().toString()
                        val node = StarMapGraphNode(
                            id = nodeId,
                            title = title,
                            kind = kind
                        )
                        viewModel.repository.addStarmapNode(starmapId, node)
                    }.await()
                    if (ok) {
                        viewModel.onShowAddNodeDialog(false)
                        viewModel.loadStarMap(starmapId)
                    }
                }
            },
            onDismiss = { viewModel.onShowAddNodeDialog(false) }
        )
    }

    if (viewModel.showAddEdgeDialog && viewModel.editorState.starMapData != null) {
        StarMapAddEdgeDialog(
            nodes = viewModel.editorState.starMapData!!.graph.nodes,
            onConfirm = { fromNodeId, toNodeId ->
                coroutineScope.launch {
                    val ok = viewModel.executeOperation("add_edge") {
                        viewModel.repository.addStarmapEdge(starmapId, fromNodeId, toNodeId)
                    }.await()
                    if (ok) {
                        viewModel.onShowAddEdgeDialog(false)
                        viewModel.loadStarMap(starmapId)
                    }
                }
            },
            onDismiss = { viewModel.onShowAddEdgeDialog(false) }
        )
    }
}
