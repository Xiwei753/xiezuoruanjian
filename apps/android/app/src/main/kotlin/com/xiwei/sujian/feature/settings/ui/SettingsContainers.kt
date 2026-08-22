package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.unit.dp

/**
 * #633 评论 5379618506：设置页视觉原语 — 固定三种东西：组顶、展开外壳、内卡。
 *
 * 删除旧的 First/Middle/Last 拼卡模型（SettingsExpandedFieldContainer /
 * ExpandedFieldPosition / SettingsExpandedGroupContainer / firstInGroup /
 * lastInGroup / CONTENT_TYPE_*）。不是删除 Low/High 层级，而是删除"每个 Lazy item
 * 自己画一截 Low + 一截 High"的错误建模。
 *
 * 视觉关系固定：
 * ```
 * SettingsGroupHeader / 前面的母设置项（Low）
 * └── 母项展开时：下边直角
 * SettingsExpandedShell（Low）
 * ├── 顶边永远直角
 * ├── SettingsInnerCard（High，全圆角）
 * ├── 8dp Low 间距
 * ├── SettingsInnerCard（High，全圆角）
 * └── 如果这是整组最后：只有外壳最底部 28dp 圆角
 * ```
 *
 * 层级靠 tonal surface 区分，不靠边框阴影（所有 Surface 显式 shadowElevation = 0.dp）。
 * 放在 feature/settings/ui 而非 :core:designsystem，因为这些容器只服务设置页结构，
 * 不是通用设计系统组件（AGENTS.md：:core:designsystem 不依赖 :app）。
 */
internal val SettingsGroupTopShape =
    RoundedCornerShape(topStart = 28.dp, topEnd = 28.dp)
internal val SettingsGroupBottomShape =
    RoundedCornerShape(bottomStart = 28.dp, bottomEnd = 28.dp)
private val SettingsInnerCardShape = RoundedCornerShape(12.dp)

private val SettingsExpandedInset = 20.dp
private val SettingsExpandedTopGap = 8.dp
private val SettingsExpandedBottomGap = 12.dp
private val SettingsInnerCardGap = 8.dp

/**
 * 分组标题行 — [Surface] 顶部 28dp 圆角、底部直角，标题放在容器里面。
 *
 * 与下方一连串 [SettingsExpandedShell] 视觉上拼成一张外层大卡片。
 * 显式 [shadowElevation] = 0.dp：静态分组不靠阴影分离，只靠 tonal surface。
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
        shadowElevation = 0.dp,
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
 * #633 评论 5379618506：展开外壳 — 永远没有上圆角。
 *
 * 只有它本身也是整个 SettingsGroup 最后一块时（[closesGroup] = true），才画底圆角。
 * 内部用 Column + spacedBy(8dp) 统一产生内卡间距，不再靠每个 item 自己拼圆角。
 *
 * @param closesGroup 该展开外壳是否为整个 [SettingsGroup] 的最后一块（决定底圆角）
 */
@Composable
fun SettingsExpandedShell(
    closesGroup: Boolean,
    modifier: Modifier = Modifier,
    content: @Composable ColumnScope.() -> Unit,
) {
    val outerShape =
        if (closesGroup) SettingsGroupBottomShape else RectangleShape

    Surface(
        modifier = modifier.fillMaxWidth(),
        color = MaterialTheme.colorScheme.surfaceContainerLow,
        shape = outerShape,
        shadowElevation = 0.dp,
    ) {
        Column(
            modifier =
                Modifier.padding(
                    start = SettingsExpandedInset,
                    end = SettingsExpandedInset,
                    top = SettingsExpandedTopGap,
                    bottom = SettingsExpandedBottomGap,
                ),
            verticalArrangement = Arrangement.spacedBy(SettingsInnerCardGap),
            content = content,
        )
    }
}

/**
 * #633 评论 5379618506：内卡 — surfaceContainerHigh，全 12dp 圆角。
 *
 * 一个逻辑字段组 = 一张 [SettingsInnerCard]。组内字段普通 Column 布局，
 * 不再给每张内卡各自 AnimatedVisibility / animateItem / animateContentSize。
 */
@Composable
fun SettingsInnerCard(
    modifier: Modifier = Modifier,
    content: @Composable ColumnScope.() -> Unit,
) {
    Surface(
        modifier = modifier.fillMaxWidth(),
        color = MaterialTheme.colorScheme.surfaceContainerHigh,
        shape = SettingsInnerCardShape,
        shadowElevation = 0.dp,
    ) {
        Column(
            modifier = Modifier.padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
            content = content,
        )
    }
}

/**
 * 字段组小标题 — 仅渲染标题文字，不再套独立 [Surface]。
 *
 * 颜色由父 [SettingsInnerCard] 的 High surface 提供。
 */
@Composable
fun SettingsFieldGroupTitle(
    title: String,
    modifier: Modifier = Modifier,
) {
    Text(
        text = title,
        style = MaterialTheme.typography.titleSmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = modifier.padding(bottom = 4.dp),
    )
}
