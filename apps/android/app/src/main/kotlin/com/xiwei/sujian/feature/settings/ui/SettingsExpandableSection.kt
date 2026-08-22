package com.xiwei.sujian.feature.settings.ui

import androidx.compose.material3.Icon
import androidx.compose.runtime.Composable
import com.xiwei.sujian.core.designsystem.component.SujianListItem
import com.xiwei.sujian.core.designsystem.icon.SujianIcons

/**
 * 设置页折叠面板标题行 — 只显示标题、当前值和展开箭头。
 *
 * 展开后的内容由父 LazyColumn 按 item 插入，不在本组件内渲染。
 * 分类背景（surfaceContainer + large shape）只由 SettingsGroupItemContainer 负责，
 * 本组件不再画第二层 Surface，避免 Low 外壳 → category Surface → ListItem 三层嵌套。
 *
 * #630 评论 5312333045 项3: 去掉 AnimatedVisibility(content) —
 * 展开内容已由父 LazyColumn 按 item 插入，不再把整个分类当成一个动画大块。
 *
 * #632 评论 5378239827 项1: 删除第二层 Surface — 分类背景和圆角只由
 * SettingsGroupItemContainer 负责，本组件直接返回 SujianListItem。
 */
@Composable
fun SettingsExpandableSection(
    title: String,
    summary: String?,
    value: String?,
    expanded: Boolean,
    onExpandedChange: (Boolean) -> Unit,
) {
    SujianListItem(
        headline = title,
        supportingText = summary,
        valueText = value,
        onClick = {
            com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.settingsSection(title, !expanded)
            onExpandedChange(!expanded)
        },
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
}
