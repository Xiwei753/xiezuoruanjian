package com.xiwei.sujian.ui.phone.portrait

import androidx.compose.foundation.layout.Row
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.nestedscroll.nestedScroll
import com.xiwei.sujian.designsystem.component.SujianIconButton
import com.xiwei.sujian.designsystem.icon.SujianIcons
import com.xiwei.sujian.model.SyncIndicatorState

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PhoneTopBar(
    spec: PhoneChromeSpec,
    onBack: () -> Unit,
    onSettings: () -> Unit,
    onSearch: () -> Unit,
    onSync: () -> Unit,
    syncState: SyncIndicatorState,
    modifier: Modifier = Modifier,
) {
    val scrollBehavior = TopAppBarDefaults.pinnedScrollBehavior()
    val containerColor = if (spec.appBarTransparent) {
        Color.Transparent
    } else {
        MaterialTheme.colorScheme.surface
    }

    TopAppBar(
        title = {
            if (spec.title != null) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(text = spec.title, style = MaterialTheme.typography.titleLarge)
                }
            }
        },
        modifier = modifier.nestedScroll(scrollBehavior.nestedScrollConnection),
        navigationIcon = {
            if (spec.showBack) {
                SujianIconButton(
                    onClick = onBack,
                    icon = SujianIcons.ArrowBack,
                    contentDescription = "返回",
                )
            }
        },
        actions = {
            if (spec.showSync) {
                SujianIconButton(
                    onClick = onSync,
                    icon = when (syncState) {
                        SyncIndicatorState.Unconfigured -> SujianIcons.CloudOff
                        SyncIndicatorState.Syncing -> SujianIcons.CloudSync
                        SyncIndicatorState.Synced -> SujianIcons.CloudDone
                        SyncIndicatorState.Failed -> SujianIcons.CloudError
                    },
                    contentDescription = when (syncState) {
                        SyncIndicatorState.Unconfigured -> "未配置同步"
                        SyncIndicatorState.Syncing -> "正在同步"
                        SyncIndicatorState.Synced -> "已同步"
                        SyncIndicatorState.Failed -> "同步失败"
                    },
                    iconTint = when (syncState) {
                        SyncIndicatorState.Unconfigured -> Color.Gray
                        SyncIndicatorState.Syncing -> Color(0xFFE6A800)
                        SyncIndicatorState.Synced -> Color(0xFF4CAF50)
                        SyncIndicatorState.Failed -> Color(0xFFF44336)
                    },
                )
            }
            if (spec.showSearch) {
                SujianIconButton(
                    onClick = onSearch,
                    icon = SujianIcons.Search,
                    contentDescription = "搜索",
                )
            }
            if (spec.showSettings) {
                SujianIconButton(
                    onClick = onSettings,
                    icon = SujianIcons.Settings,
                    contentDescription = "设置",
                )
            }
        },
        colors = TopAppBarDefaults.topAppBarColors(
            containerColor = containerColor,
            titleContentColor = MaterialTheme.colorScheme.onSurface,
        ),
    )
}
