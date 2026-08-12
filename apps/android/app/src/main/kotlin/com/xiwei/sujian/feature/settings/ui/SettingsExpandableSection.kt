package com.xiwei.sujian.feature.settings.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
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
 *
 * #618 四：去掉 expandVertically/shrinkVertically — 它们逐帧改变内容高度，
 * 外层 LazyColumn 每次展开都连续触发布局阶段；现在只做短的绘制层淡入淡出，
 * 高度只在展开状态改变时布局一次，不再为了动画连续重排列表。
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
        AnimatedVisibility(
            visible = expanded,
            enter = fadeIn(animationSpec = tween(120)),
            exit = fadeOut(animationSpec = tween(90)),
        ) {
            content()
        }
    }
}
