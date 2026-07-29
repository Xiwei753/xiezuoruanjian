package com.xiwei.sujian.ui.compose.workbench.component

import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInWindow
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.R
import com.xiwei.sujian.designsystem.icon.SujianIcons
import com.xiwei.sujian.designsystem.theme.LocalSujianDimensions
import com.xiwei.sujian.ui.compose.workbench.model.DockZone
import com.xiwei.sujian.ui.compose.workbench.model.DragDropTarget
import com.xiwei.sujian.ui.compose.workbench.model.PanelVisibility
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelId
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelState

@Composable
fun WorkbenchPanelFrame(
    panelState: WorkbenchPanelState,
    onFloat: () -> Unit,
    onCollapse: () -> Unit,
    onClose: () -> Unit,
    modifier: Modifier = Modifier,
    title: String? = null,
    titleBarModifier: Modifier = Modifier,
    onTitleBarPositionChanged: ((xWindowPx: Float, yWindowPx: Float) -> Unit)? = null,
    onDragStart: ((Float, Float) -> Unit)? = null,
    onDrag: ((Float, Float) -> Unit)? = null,
    onDragEnd: ((Float, Float) -> Unit)? = null,
    onDragCancel: (() -> Unit)? = null,
    content: @Composable () -> Unit,
) {
    val dims = LocalSujianDimensions.current
    val density = LocalDensity.current
    var isDragging by remember { mutableStateOf(false) }

    Column(modifier = modifier) {
        Surface(
            color = MaterialTheme.colorScheme.surfaceVariant,
            tonalElevation = 1.dp,
        ) {
            androidx.compose.foundation.layout.Row(
                modifier = Modifier.padding(horizontal = dims.space8, vertical = dims.space4)
                    .height(40.dp)
                    .then(
                        if (onTitleBarPositionChanged != null) {
                            Modifier.onGloballyPositioned { coords ->
                                val pos = coords.positionInWindow()
                                onTitleBarPositionChanged(pos.x, pos.y)
                            }
                        } else Modifier
                    )
                    .then(
                        if (onDragStart != null && onDrag != null && onDragEnd != null) {
                            Modifier.pointerInput(panelState.id) {
                                detectDragGestures(
                                    onDragStart = { offset ->
                                        isDragging = true
                                        onDragStart(offset.x / density.density, offset.y / density.density)
                                    },
                                    onDragEnd = {
                                        isDragging = false
                                        onDragEnd(0f, 0f)
                                    },
                                    onDragCancel = {
                                        isDragging = false
                                        onDragCancel?.invoke()
                                    },
                                ) { change, dragAmount ->
                                    change.consume()
                                    onDrag(dragAmount.x / density.density, dragAmount.y / density.density)
                                }
                            }
                        } else Modifier
                    )
                    .then(titleBarModifier),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                val panelIcon = panelIconForId(panelState.id)
                if (panelIcon != null) {
                    Icon(
                        imageVector = panelIcon,
                        contentDescription = null,
                        modifier = Modifier.size(18.dp),
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Spacer(modifier = Modifier.width(dims.space8))
                }
                Text(
                    text = title ?: panelTitleForId(panelState.id),
                    style = MaterialTheme.typography.titleSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.weight(1f),
                )
                IconButton(onClick = onFloat, modifier = Modifier.size(28.dp)) {
                    Icon(
                        imageVector = SujianIcons.OpenInNew,
                        contentDescription = stringResource(R.string.workbench_panel_float),
                        modifier = Modifier.size(16.dp),
                    )
                }
                IconButton(onClick = onCollapse, modifier = Modifier.size(28.dp)) {
                    Icon(
                        imageVector = SujianIcons.UnfoldLess,
                        contentDescription = stringResource(R.string.workbench_panel_collapse),
                        modifier = Modifier.size(16.dp),
                    )
                }
                IconButton(onClick = onClose, modifier = Modifier.size(28.dp)) {
                    Icon(
                        imageVector = SujianIcons.Close,
                        contentDescription = stringResource(R.string.workbench_panel_close),
                        modifier = Modifier.size(16.dp),
                    )
                }
            }
        }
        content()
    }
}

@Composable
fun panelTitleForId(id: WorkbenchPanelId): String = when (id) {
    WorkbenchPanelId.ProjectNavigator -> stringResource(R.string.workbench_panel_project_navigator)
    WorkbenchPanelId.ChapterNavigator -> stringResource(R.string.workbench_panel_chapter_navigator)
    WorkbenchPanelId.AiAssistant -> stringResource(R.string.workbench_panel_ai_assistant)
    WorkbenchPanelId.Search -> stringResource(R.string.workbench_panel_search)
    WorkbenchPanelId.Statistics -> stringResource(R.string.workbench_panel_statistics)
    WorkbenchPanelId.StarMap -> stringResource(R.string.workbench_panel_starmap)
    WorkbenchPanelId.DocumentOutline -> stringResource(R.string.workbench_panel_document_outline)
    WorkbenchPanelId.CharacterInfo -> stringResource(R.string.workbench_panel_character_info)
}

fun panelIconForId(id: WorkbenchPanelId): ImageVector? = when (id) {
    WorkbenchPanelId.ProjectNavigator -> SujianIcons.AutoStoriesOutlined
    WorkbenchPanelId.ChapterNavigator -> SujianIcons.MenuBook
    WorkbenchPanelId.AiAssistant -> SujianIcons.SmartToy
    WorkbenchPanelId.Search -> SujianIcons.Search
    WorkbenchPanelId.Statistics -> SujianIcons.BarChartOutlined
    WorkbenchPanelId.StarMap -> SujianIcons.HubOutlined
    WorkbenchPanelId.DocumentOutline -> SujianIcons.ListAlt
    WorkbenchPanelId.CharacterInfo -> SujianIcons.PersonOutline
}
