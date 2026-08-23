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
 *        决定整块分类最外层 Surface 是否使用 [SettingsGroupBottomShape]。
 */
data class SettingsCategoryHeaderInfo(
    val summary: String?,
    val value: String?,
    val isLastCategory: Boolean,
)

/**
 * #633 评论 5379618506 / #635 评论 5385740370：可展开分类 —
 * 独占"整组外轮廓 + 展开动画"状态。
 *
 * 关键：只有这一处拥有展开动画，也只有这一处拥有整组底圆角。
 * - 不再给字段 animateItem()
 * - 不再给每张内卡各自 AnimatedVisibility()
 * - 不再同时套 animateContentSize()
 *
 * 一个 category = 一个 [Transition] + 一个 [AnimatedVisibility]。
 *
 * #635 评论 5385740370：底圆角不再随展开/收起在母栏 Surface 上硬切。
 * 整块 category 的最外层 Low Surface 是圆角唯一拥有者：
 * - `isLastCategory=true` 时外层永久 [SettingsGroupBottomShape]，否则 [RectangleShape]；
 * - 母栏内部 Surface 永远 [RectangleShape]；
 * - [AnimatedVisibility] 放在外层 Surface 的 Column 中，外层底边随展开区高度
 *   自然下移/上移，不再出现"母栏圆角突然变直又恢复"的一帧。
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

    val categoryShape =
        if (headerInfo.isLastCategory) SettingsGroupBottomShape else RectangleShape

    Surface(
        modifier = modifier.fillMaxWidth(),
        color = MaterialTheme.colorScheme.surfaceContainerLow,
        shape = categoryShape,
        shadowElevation = 0.dp,
    ) {
        Column(Modifier.fillMaxWidth()) {
            Surface(
                modifier = Modifier.fillMaxWidth(),
                color = MaterialTheme.colorScheme.surfaceContainerLow,
                shape = RectangleShape,
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
                SettingsExpandedShell(content = expandedContent)
            }
        }
    }
}
