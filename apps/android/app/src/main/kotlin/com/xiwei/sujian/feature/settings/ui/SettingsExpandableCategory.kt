package com.xiwei.sujian.feature.settings.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.Transition
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.tween
import androidx.compose.animation.core.updateTransition
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.core.designsystem.component.SujianListItem
import com.xiwei.sujian.core.designsystem.icon.SujianIcons

/**
 * #633 评论 5379618506：可展开分类的标题辅助信息 —
 * 把 summary/value/isLastCategory 包装成 data class 以控制参数数量。
 *
 * @param isLastCategory 该分类是否为所在 [SettingsGroup] 的最后一个分类。
 *        决定母项未展开时是否画底圆角、展开外壳是否收口整组。
 */
data class SettingsCategoryHeaderInfo(
    val summary: String?,
    val value: String?,
    val isLastCategory: Boolean,
)

/**
 * #633 评论 5379618506：可展开分类 — 独占"母项圆角 + 展开动画"状态。
 *
 * 关键：只有这一处拥有展开动画。
 * - 不再给字段 animateItem()
 * - 不再给每张内卡各自 AnimatedVisibility()
 * - 不再同时套 animateContentSize()
 *
 * 一个 category = 一个 [Transition] + 一个 [AnimatedVisibility]。
 *
 * 展开开始时立刻去掉母项下圆角；收起时要等 [AnimatedVisibility] 完全结束后
 * 才恢复母项下圆角（用 `transition.currentState || transition.targetState` 判断）。
 */
@Composable
fun SettingsExpandableCategory(
    title: String,
    headerInfo: SettingsCategoryHeaderInfo,
    expanded: Boolean,
    onExpandedChange: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
    expandedContent: @Composable ColumnScope.() -> Unit,
) {
    val transition =
        updateTransition(
            targetState = expanded,
            label = "settings:$title",
        )

    // 展开开始时立刻去掉母项下圆角；
    // 收起时要等 AnimatedVisibility 完全结束后才恢复母项下圆角。
    val visuallyExpanded = transition.currentState || transition.targetState

    val headerShape =
        if (headerInfo.isLastCategory && !visuallyExpanded) {
            SettingsGroupBottomShape
        } else {
            RectangleShape
        }

    Column(modifier = modifier.fillMaxWidth()) {
        Surface(
            modifier = Modifier.fillMaxWidth(),
            color = MaterialTheme.colorScheme.surfaceContainerLow,
            shape = headerShape,
            shadowElevation = 0.dp,
        ) {
            SujianListItem(
                headline = title,
                supportingText = headerInfo.summary,
                valueText = headerInfo.value,
                onClick = { onExpandedChange(!expanded) },
                trailingContent = {
                    val arrowRotation by transition.animateFloat(
                        label = "settings-arrow",
                    ) { isExpanded ->
                        if (isExpanded) 180f else 0f
                    }
                    Icon(
                        imageVector = SujianIcons.KeyboardArrowDown,
                        contentDescription = null,
                        modifier =
                            Modifier.graphicsLayer {
                                rotationZ = arrowRotation
                            },
                    )
                },
            )
        }

        transition.AnimatedVisibility(
            visible = { it },
            enter =
                expandVertically(
                    expandFrom = Alignment.Top,
                    animationSpec = tween(150),
                ) + fadeIn(animationSpec = tween(110)),
            exit =
                shrinkVertically(
                    shrinkTowards = Alignment.Top,
                    animationSpec = tween(130),
                ) + fadeOut(animationSpec = tween(90)),
        ) {
            SettingsExpandedShell(
                closesGroup = headerInfo.isLastCategory,
                content = expandedContent,
            )
        }
    }
}
