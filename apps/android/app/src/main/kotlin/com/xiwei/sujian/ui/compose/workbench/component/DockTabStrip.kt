package com.xiwei.sujian.ui.compose.workbench.component

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.designsystem.theme.LocalSujianDimensions
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelId
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelState

@Composable
fun DockTabStrip(
    panels: List<WorkbenchPanelState>,
    activeTabId: WorkbenchPanelId,
    onActivateTab: (WorkbenchPanelId) -> Unit,
    modifier: Modifier = Modifier,
) {
    val dims = LocalSujianDimensions.current
    Surface(
        color = MaterialTheme.colorScheme.surfaceVariant,
        tonalElevation = 0.dp,
        modifier = modifier,
    ) {
        Row(
            modifier = Modifier.fillMaxWidth().height(36.dp).padding(horizontal = dims.space4),
            horizontalArrangement = Arrangement.Start,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            for (panel in panels) {
                val isActive = panel.id == activeTabId
                val icon = panelIconForId(panel.id)
                androidx.compose.foundation.layout.Row(
                    modifier = Modifier
                        .clickable { onActivateTab(panel.id) }
                        .padding(horizontal = dims.space8, vertical = dims.space4),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    if (icon != null) {
                        Icon(
                            imageVector = icon,
                            contentDescription = null,
                            modifier = Modifier.size(14.dp),
                            tint = if (isActive) MaterialTheme.colorScheme.primary
                            else MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    Text(
                        text = panelTitleForId(panel.id),
                        style = MaterialTheme.typography.labelSmall,
                        color = if (isActive) MaterialTheme.colorScheme.primary
                        else MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
    }
}
