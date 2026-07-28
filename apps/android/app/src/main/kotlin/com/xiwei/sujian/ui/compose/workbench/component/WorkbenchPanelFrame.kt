package com.xiwei.sujian.ui.compose.workbench.component

import androidx.compose.foundation.background
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
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.R
import com.xiwei.sujian.designsystem.icon.SujianIcons
import com.xiwei.sujian.designsystem.theme.LocalSujianDimensions
import com.xiwei.sujian.ui.compose.workbench.model.DockZone
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
    content: @Composable () -> Unit,
) {
    val dims = LocalSujianDimensions.current
    Column(modifier = modifier) {
        Surface(
            color = MaterialTheme.colorScheme.surfaceVariant,
            tonalElevation = 1.dp,
        ) {
            androidx.compose.foundation.layout.Row(
                modifier = Modifier.padding(horizontal = dims.space8, vertical = dims.space4)
                    .height(40.dp)
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
