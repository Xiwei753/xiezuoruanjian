package com.xiwei.sujian.ui.phone.portrait

import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.stringResource
import com.xiwei.sujian.R
import com.xiwei.sujian.designsystem.icon.SujianIcons
import com.xiwei.sujian.designsystem.theme.LocalSujianDimensions

private data class BottomBarItem(
    val root: PhoneRoot,
    val labelResId: Int,
    val selectedIcon: ImageVector,
    val unselectedIcon: ImageVector,
)

private val bottomBarItems = listOf(
    BottomBarItem(
        root = PhoneRoot.Works,
        labelResId = R.string.title_projects,
        selectedIcon = SujianIcons.AutoStories,
        unselectedIcon = SujianIcons.AutoStoriesOutlined,
    ),
    BottomBarItem(
        root = PhoneRoot.StarMap,
        labelResId = R.string.title_starmap,
        selectedIcon = SujianIcons.Hub,
        unselectedIcon = SujianIcons.HubOutlined,
    ),
    BottomBarItem(
        root = PhoneRoot.Stats,
        labelResId = R.string.title_stats,
        selectedIcon = SujianIcons.BarChart,
        unselectedIcon = SujianIcons.BarChartOutlined,
    ),
)

@Composable
fun PhoneBottomBar(
    selectedRoot: PhoneRoot,
    onRootSelected: (PhoneRoot) -> Unit,
    modifier: Modifier = Modifier,
) {
    NavigationBar(modifier = modifier) {
        bottomBarItems.forEach { item ->
            NavigationBarItem(
                selected = selectedRoot == item.root,
                onClick = { onRootSelected(item.root) },
                icon = {
                    Icon(
                        imageVector = if (selectedRoot == item.root) item.selectedIcon else item.unselectedIcon,
                        contentDescription = stringResource(id = item.labelResId),
                    )
                },
                label = { Text(text = stringResource(id = item.labelResId)) },
            )
        }
    }
}
