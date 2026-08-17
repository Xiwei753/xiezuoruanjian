package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp

/**
 * 设置页自己的三层 Material 3 容器组件。
 *
 * 颜色层级固定为：
 * - 页面 surface
 * - 分组外壳 surfaceContainerLow（[SettingsGroupHeader] + [SettingsGroupItemContainer]）
 * - 可展开分类卡 surfaceContainer（[SettingsExpandableSection]）
 * - 展开后字段组 surfaceContainerHigh（[SettingsFieldGroup]）
 * - 搜索入口 surfaceContainerHighest（[SettingsSearchEntry]）
 *
 * 层级靠 tonal surface 区分，不靠边框阴影，也不用 primary 文本充当分组层级。
 * 放在 feature/settings/ui 而非 :core:designsystem，因为这些容器只服务设置页结构，
 * 不是通用设计系统组件（AGENTS.md：:core:designsystem 不依赖 :app）。
 */
private val SettingsGroupTopShape = RoundedCornerShape(topStart = 28.dp, topEnd = 28.dp)
private val SettingsGroupBottomShape = RoundedCornerShape(bottomStart = 28.dp, bottomEnd = 28.dp)

/**
 * 分组标题行 — [Surface] 顶部 28dp 圆角、底部直角，标题放在容器里面。
 *
 * 与下方一连串 [SettingsGroupItemContainer] 视觉上拼成一张外层大卡片。
 */
@Composable
fun SettingsGroupHeader(
    title: String,
    modifier: Modifier = Modifier,
) {
    Surface(
        modifier = modifier.fillMaxWidth(),
        color = MaterialTheme.colorScheme.surfaceContainerLow,
        shape = SettingsGroupTopShape,
    ) {
        Text(
            text = title,
            style = MaterialTheme.typography.titleSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
        )
    }
}

/**
 * 分组内单个分类项的容器 — 与 [SettingsGroupHeader] 同色，
 * 首项顶部 28dp 圆角，末项底部 28dp 圆角，中间项直角，
 * 视觉上与标题行拼成一张外层大卡片。
 *
 * 扁平 LazyColumn 场景：展开分类的字段拆成多个独立 item，
 * 每个 item 通过 [isFirst]/[isLast] 控制圆角，保持同一张卡片视觉。
 *
 * @param isLast 是否为分组最后一项；决定底部圆角与底部 padding。
 * @param isFirst 是否为展开分类的第一个 item；决定顶部圆角。
 */
@Composable
fun SettingsGroupItemContainer(
    isLast: Boolean,
    modifier: Modifier = Modifier,
    isFirst: Boolean = false,
    content: @Composable () -> Unit,
) {
    val shape =
        when {
            isFirst && isLast -> SettingsGroupTopShape // 展开分类只有一个 item
            isFirst -> SettingsGroupTopShape
            isLast -> SettingsGroupBottomShape
            else -> RectangleShape
        }
    val bottomPadding = if (isLast) 12.dp else 8.dp
    val topPadding = if (isFirst && !isLast) 0.dp else 0.dp // isFirst 由上方 category header 间距处理
    Surface(
        modifier = modifier.fillMaxWidth(),
        color = MaterialTheme.colorScheme.surfaceContainerLow,
        shape = shape,
    ) {
        Column(
            modifier =
                Modifier
                    .padding(start = 12.dp, end = 12.dp, top = topPadding, bottom = bottomPadding),
        ) {
            content()
        }
    }
}

/**
 * 展开后字段组 — surfaceContainerHigh + [MaterialTheme.shapes.large]，
 * 标题 titleSmall/onSurfaceVariant，内容 16dp padding。
 *
 * 替代旧 SujianSection，避免"展开分类 padding + SujianSection 标题 + SujianCard padding"
 * 一层套一层；外层 padding 只保留一层。
 */
@Composable
fun SettingsFieldGroup(
    title: String,
    modifier: Modifier = Modifier,
    semanticId: String? = null,
    content: @Composable () -> Unit,
) {
    Surface(
        modifier =
            modifier
                .fillMaxWidth()
                .then(if (semanticId != null) Modifier.testTag(semanticId) else Modifier),
        color = MaterialTheme.colorScheme.surfaceContainerHigh,
        shape = MaterialTheme.shapes.large,
    ) {
        Column(modifier = Modifier.padding(16.dp)) {
            Text(
                text = title,
                style = MaterialTheme.typography.titleSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(bottom = 8.dp),
            )
            content()
        }
    }
}
