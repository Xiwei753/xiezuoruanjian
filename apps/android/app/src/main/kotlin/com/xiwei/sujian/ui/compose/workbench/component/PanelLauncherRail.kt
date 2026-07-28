package com.xiwei.sujian.ui.compose.workbench.component

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
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
fun PanelLauncherRail(
    zone: DockZone,
    panels: List<WorkbenchPanelState>,
    onTogglePanel: (WorkbenchPanelId) -> Unit,
    modifier: Modifier = Modifier,
) {
    val dims = LocalSujianDimensions.current
    val visiblePanels = panels.sortedBy { it.order }

    Surface(
        color = MaterialTheme.colorScheme.surface,
        tonalElevation = 2.dp,
        modifier = modifier,
    ) {
        if (zone == DockZone.Bottom) {
            androidx.compose.foundation.layout.Row(
                modifier = Modifier.fillMaxWidth().padding(vertical = dims.space4),
                horizontalArrangement = Arrangement.Center,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                for (panel in visiblePanels) {
                    val icon = panelIconForId(panel.id) ?: SujianIcons.Widgets
                    IconButton(
                        onClick = { onTogglePanel(panel.id) },
                        modifier = Modifier.size(48.dp),
                    ) {
                        Icon(
                            imageVector = icon,
                            contentDescription = panelTitleForId(panel.id),
                            tint = if (panel.visibility == PanelVisibility.Expanded)
                                MaterialTheme.colorScheme.primary
                            else
                                MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }
        } else {
            Column(
                modifier = Modifier.fillMaxHeight().padding(horizontal = dims.space4),
                verticalArrangement = Arrangement.Top,
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Spacer(modifier = Modifier.height(dims.space8))
                for (panel in visiblePanels) {
                    val icon = panelIconForId(panel.id) ?: SujianIcons.Widgets
                    IconButton(
                        onClick = { onTogglePanel(panel.id) },
                        modifier = Modifier.size(48.dp),
                    ) {
                        Icon(
                            imageVector = icon,
                            contentDescription = panelTitleForId(panel.id),
                            tint = if (panel.visibility == PanelVisibility.Expanded)
                                MaterialTheme.colorScheme.primary
                            else
                                MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }
        }
    }
}
