package com.xiwei.sujian.feature.settings.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.Icon
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import com.xiwei.sujian.core.designsystem.component.SujianListItem
import com.xiwei.sujian.core.designsystem.icon.SujianIcons

/**
 * 设置页同页折叠面板 — 标题行 + 当前值 + 展开箭头 + 行内内容。
 *
 * 点击标题行切换展开/折叠；展开内容用 [AnimatedVisibility] 做行内动画，
 * 不导航到子页面，不建立第二套页面导航状态。
 */
@Composable
fun SettingsExpandableSection(
    title: String,
    summary: String?,
    value: String?,
    expanded: Boolean,
    onExpandedChange: (Boolean) -> Unit,
    content: @Composable () -> Unit,
) {
    Column(modifier = Modifier.fillMaxWidth()) {
        SujianListItem(
            headline = title,
            supportingText = summary,
            valueText = value,
            onClick = { onExpandedChange(!expanded) },
            trailingContent = {
                Icon(
                    imageVector =
                        if (expanded) {
                            SujianIcons.KeyboardArrowUp
                        } else {
                            SujianIcons.KeyboardArrowDown
                        },
                    contentDescription = null,
                )
            },
        )
        AnimatedVisibility(
            visible = expanded,
            enter = expandVertically() + fadeIn(),
            exit = shrinkVertically() + fadeOut(),
        ) {
            content()
        }
    }
}
