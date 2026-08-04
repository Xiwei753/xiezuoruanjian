package com.xiwei.sujian.ui.compose.starmap

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import com.xiwei.sujian.designsystem.component.SujianCard
import com.xiwei.sujian.designsystem.component.SujianFab
import com.xiwei.sujian.designsystem.component.SujianIconButton
import com.xiwei.sujian.designsystem.icon.SujianIcons
import com.xiwei.sujian.designsystem.theme.LocalSujianDimensions
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import com.xiwei.sujian.R
import com.xiwei.sujian.model.StarMapViewportData
import com.xiwei.sujian.ui.compose.navigation.StarMapTopBarState

/**
 * 星图列表 — 根壳已统一处理系统栏 Insets、一级 TopAppBar 与 FAB 层级，
 * 这里只渲染列表内容与新建 FAB。
 */
@Composable
internal fun StarMapListContent(
    state: StarMapListUiState,
    onSelectStarmap: (String) -> Unit,
    onCreateClick: () -> Unit,
    modifier: Modifier = Modifier
) {
    val dims = LocalSujianDimensions.current

    Box(modifier = modifier.fillMaxSize()) {
        if (state.isLoading) {
            Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                Text(stringResource(id = R.string.loading), style = MaterialTheme.typography.bodyLarge)
            }
        } else if (state.starMaps.isEmpty()) {
            Column(
                modifier = Modifier.fillMaxSize().padding(dims.space32),
                verticalArrangement = Arrangement.Center,
                horizontalAlignment = Alignment.CenterHorizontally
            ) {
                Text(stringResource(id = R.string.starmap_empty), style = MaterialTheme.typography.titleMedium)
                Spacer(modifier = Modifier.height(dims.space8))
                Text(stringResource(id = R.string.starmap_empty_hint), style = MaterialTheme.typography.bodyMedium)
            }
        } else {
            LazyColumn(
                contentPadding = PaddingValues(horizontal = dims.space16, vertical = dims.space8),
                modifier = Modifier.fillMaxSize()
            ) {
                items(state.starMaps, key = { it.starmapId }) { meta ->
                    SujianCard(
                        onClick = { onSelectStarmap(meta.starmapId) },
                        modifier = Modifier.fillMaxWidth().padding(bottom = dims.space8),
                    ) {
                        Column(modifier = Modifier.padding(dims.space16)) {
                            Text(meta.title, style = MaterialTheme.typography.titleMedium)
                            if (meta.description.isNotBlank()) {
                                Text(meta.description, style = MaterialTheme.typography.bodySmall)
                            }
                            Text(stringResource(R.string.starmap_node_edge_count, meta.nodeCount, meta.edgeCount), style = MaterialTheme.typography.bodySmall)
                        }
                    }
                }
            }
        }

        SujianFab(
            onClick = onCreateClick,
            icon = SujianIcons.Add,
            contentDescription = stringResource(id = R.string.starmap_create_new),
            modifier = Modifier.align(Alignment.BottomEnd).padding(dims.space16),
        )
    }
}

/**
 * 星图编辑器 — 标题、返回和操作按钮通过 [topBarState] 上抛给唯一根壳的
 * 一级 TopAppBar；正文只渲染画布与节点编辑面板。
 */
@Composable
internal fun StarMapEditorContent(
    state: StarMapEditorUiState,
    topBarState: StarMapTopBarState,
    onBack: () -> Unit,
    onAddNodeClick: () -> Unit,
    onAddEdgeClick: () -> Unit,
    onNodeDrag: (nodeId: String, x: Float, y: Float) -> Unit,
    onViewportChange: (viewport: StarMapViewportData) -> Unit,
    onNodeTap: (nodeId: String) -> Unit,
    onNodeDoubleTap: (geometry: NodeTextGeometry) -> Unit,
    onRetrySaves: () -> Unit = {},
    modifier: Modifier = Modifier
) {
    val dims = LocalSujianDimensions.current
    val editorTitle = state.starMapData?.graph?.title ?: stringResource(id = R.string.title_starmap)
    val nodeCount = state.starMapData?.graph?.nodes?.size ?: 0
    val edgeCount = state.starMapData?.graph?.edges?.size ?: 0

    val topBarActions: @Composable () -> Unit = {
        Text(
            stringResource(R.string.starmap_node_edge_count, nodeCount, edgeCount),
            style = MaterialTheme.typography.bodySmall
        )
        Spacer(modifier = Modifier.width(dims.space8))
        SujianIconButton(
            onClick = onAddEdgeClick,
            icon = SujianIcons.Add,
            contentDescription = stringResource(id = R.string.starmap_add_edge),
        )
        SujianIconButton(
            onClick = onAddNodeClick,
            icon = SujianIcons.Add,
            contentDescription = stringResource(id = R.string.starmap_add_node),
        )
    }

    LaunchedEffect(editorTitle, onBack, nodeCount, edgeCount) {
        topBarState.update(
            title = editorTitle,
            onBack = onBack,
            actions = topBarActions,
        )
    }

    Box(modifier = modifier.fillMaxSize()) {
        if (state.isLoading) {
            Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                Text(stringResource(id = R.string.loading), style = MaterialTheme.typography.bodyLarge)
            }
        } else if (state.starMapData != null) {
            if (state.lastError != null) {
                Text(state.lastError, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error)
            }
            if (state.layoutSaveError != null) {
                Text(state.layoutSaveError, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error)
            }
            if (state.viewportSaveError != null) {
                Text(state.viewportSaveError, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error)
            }
            if (state.hasPendingLayoutSave || state.hasPendingViewportSave) {
                SujianIconButton(
                    onClick = onRetrySaves,
                    icon = SujianIcons.Add,
                    contentDescription = stringResource(id = R.string.starmap_retry_save),
                )
            }
            StarMapCanvas(
                data = state.starMapData,
                onNodeDrag = onNodeDrag,
                onViewportChange = onViewportChange,
                onNodeTap = onNodeTap,
                onNodeDoubleTap = onNodeDoubleTap,
                editingNodeId = state.editingNodeId,
                modifier = Modifier.fillMaxSize()
            )
        } else {
            Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                Text(stringResource(id = R.string.starmap_load_failed), style = MaterialTheme.typography.bodyLarge)
            }
        }
    }
}
