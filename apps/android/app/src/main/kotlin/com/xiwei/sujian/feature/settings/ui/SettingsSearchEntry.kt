package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.R
import com.xiwei.sujian.core.designsystem.icon.SujianIcons
import com.xiwei.sujian.core.designsystem.theme.LocalSujianDimensions

/**
 * 设置页顶部的紧凑全局搜索入口 — 搜索图标 + 提示文字，整块可点击。
 *
 * 用 [Surface]（surfaceContainerHighest + [MaterialTheme.shapes.extraLarge]）呈现
 * 草图里的圆角搜索条，而不是没有背景的 clickable Row。
 *
 * 它只是 #477 全局搜索的入口占位：不在这里实现设置过滤，
 * 也不维护 query/results 状态。#477 接入后只需在调用处替换 [onClick] 回调，
 * 不重做设置页面结构。
 */
@Composable
internal fun SettingsSearchEntry(
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val dims = LocalSujianDimensions.current
    Surface(
        onClick = onClick,
        modifier = modifier.fillMaxWidth().testTag("settings_search_entry"),
        shape = MaterialTheme.shapes.extraLarge,
        color = MaterialTheme.colorScheme.surfaceContainerHighest,
        shadowElevation = 0.dp,
    ) {
        Row(
            modifier = Modifier.padding(horizontal = dims.space16, vertical = dims.space8),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(
                imageVector = SujianIcons.Search,
                contentDescription = null,
                modifier = Modifier.size(20.dp),
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text(
                text = stringResource(R.string.settings_search_hint),
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(start = dims.space8),
            )
        }
    }
}
