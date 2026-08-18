package com.xiwei.sujian.feature.settings.ui

import androidx.compose.animation.core.tween
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyItemScope
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp

/**
 * 设置页自己的三层 Material 3 容器组件。
 *
 * 三层语义：
 * - 页面 surface
 * - 外层设置组 surfaceContainerLow（[SettingsGroupHeader] + [SettingsExpandedRowContainer] 外壳）
 * - 分类标题卡 surfaceContainer / 展开内容卡 surfaceContainerHigh（内嵌 High surface）
 * - 搜索入口 surfaceContainerHighest（[SettingsSearchEntry]）
 *
 * 层级靠 tonal surface 区分，不靠边框阴影（所有 Surface 显式 shadowElevation = 0.dp），
 * 也不用 primary 文本充当分组层级。
 * 放在 feature/settings/ui 而非 :core:designsystem，因为这些容器只服务设置页结构，
 * 不是通用设计系统组件（AGENTS.md：:core:designsystem 不依赖 :app）。
 *
 * #630 评论 5324547885 项1: 改为"三个完整区域"结构 —
 * 页面 surface → 组 Low → 分类标题 Container / 展开内容 High，
 * 不做逐行换色或阴影；[SettingsExpandedRowContainer] 在 Low 外壳内额外水平内缩，
 * 字段组内部 High surface 连续拼接且行间不漏 Low 色带，
 * 只有不同真实字段组间保留间距。
 */
private val SettingsGroupTopShape = RoundedCornerShape(topStart = 28.dp, topEnd = 28.dp)
private val SettingsGroupBottomShape = RoundedCornerShape(bottomStart = 28.dp, bottomEnd = 28.dp)

// #630 评论 5326175206 项1: 横向内缩常量 — 单一来源，展开内容比分类标题再内缩 8dp
private val SettingsCategoryInset = 12.dp
private val SettingsExpandedExtraInset = 8.dp
private val SettingsExpandedInset = SettingsCategoryInset + SettingsExpandedExtraInset

/**
 * 分组标题行 — [Surface] 顶部 28dp 圆角、底部直角，标题放在容器里面。
 *
 * 与下方一连串 [SettingsExpandedRowContainer] 视觉上拼成一张外层大卡片。
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
 * 分组内单个分类项的容器 — 与 [SettingsGroupHeader] 同色，
 * 首项顶部 28dp 圆角，末项底部 28dp 圆角，中间项直角，
 * 视觉上与标题行拼成一张外层大卡片。
 *
 * #630 评论 5324547885: 此组件只包分类标题行；展开内容改用 [SettingsExpandedRowContainer]。
 * 显式 [shadowElevation] = 0.dp：静态分组不靠阴影分离。
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
        shadowElevation = 0.dp,
    ) {
        Column(
            modifier =
                Modifier
                    .padding(
                        start = SettingsCategoryInset,
                        end = SettingsCategoryInset,
                        top = topPadding,
                        bottom = bottomPadding,
                    ),
        ) {
            content()
        }
    }
}

/**
 * 展开后字段组标题 — 仅渲染标题文字，不再套独立 [Surface]。
 *
 * 此标题属于展开内容卡（[SettingsExpandedRowContainer] 内的 High surface）内部，
 * 不要在外面再套一层同色 Surface。颜色由父 [SettingsExpandedRowContainer] 的
 * inner High surface 提供。
 */
@Composable
fun SettingsFieldGroupTitle(
    title: String,
    modifier: Modifier = Modifier,
    semanticId: String? = null,
) {
    Box(
        modifier =
            modifier
                .fillMaxWidth()
                .then(if (semanticId != null) Modifier.testTag(semanticId) else Modifier),
    ) {
        Text(
            text = title,
            style = MaterialTheme.typography.titleSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = 12.dp, bottom = 4.dp),
        )
    }
}

/**
 * #630 评论 5324547885 项1: 展开内容行容器 — 三层语义中的"展开内容卡"。
 *
 * 结构：外层 surfaceContainerLow 填满页面宽度（与分类标题同级大卡片），
 * 内部 surfaceContainerHigh 额外水平内缩（8dp），只在字段组的首行/末行圆角。
 *
 * 字段组内部 High surface 连续拼接，行与行之间不漏 Low 色带；
 * 只有不同真实字段组间（如"主题"→"字体与排版"）保留 8dp 间距，
 * 露出中间的 Low 背景色。
 *
 * 外层 Low surface 的 28dp 圆角只在整个展开类别的首行和末行生效；
 * 中间行的外层是 plain rectangle，视觉上与 [SettingsGroupHeader] 拼成连续大卡片。
 * 显式 [shadowElevation] = 0.dp：静态分组不靠阴影分离。
 *
 * @param firstInCategory 该行是否为展开类别的第一个 item；控制外层 Low 圆角。
 * @param lastInCategory 该行是否为展开类别的最后一个 item；控制外层 Low 圆角。
 * @param firstInGroup 该行是否为字段组的第一行（通常对应标题行）；控制内层 High 圆角。
 * @param lastInGroup 该行是否为字段组的最后一行；控制内层 High 圆角。
 */
@Composable
fun SettingsExpandedRowContainer(
    firstInCategory: Boolean,
    lastInCategory: Boolean,
    firstInGroup: Boolean,
    lastInGroup: Boolean,
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit,
) {
    val outerShape =
        when {
            firstInCategory && lastInCategory -> SettingsGroupTopShape
            firstInCategory -> SettingsGroupTopShape
            lastInCategory -> SettingsGroupBottomShape
            else -> RectangleShape
        }

    // #630 评论 5326175206 项1: firstInGroup 且非 firstInCategory 时，顶部留 8dp Low 背景色
    // 作为不同字段组间的真实组间距；category 最后一行底部也留 8dp Low padding。
    val groupTopPadding =
        if (firstInGroup && !firstInCategory) SettingsExpandedExtraInset else 0.dp
    val categoryBottomPadding =
        if (lastInCategory) SettingsExpandedExtraInset else 0.dp

    Surface(
        modifier = modifier.fillMaxWidth(),
        color = MaterialTheme.colorScheme.surfaceContainerLow,
        shape = outerShape,
        shadowElevation = 0.dp,
    ) {
        Column {
            if (groupTopPadding > 0.dp) {
                Spacer(modifier = Modifier.height(groupTopPadding))
            }
            Surface(
                modifier = Modifier.fillMaxWidth().padding(horizontal = SettingsExpandedInset),
                color = MaterialTheme.colorScheme.surfaceContainerHigh,
                shape = expandedInnerShape(firstInGroup, lastInGroup),
                shadowElevation = 0.dp,
            ) {
                content()
            }
            if (categoryBottomPadding > 0.dp) {
                Spacer(modifier = Modifier.height(categoryBottomPadding))
            }
        }
    }
}

/**
 * [SettingsExpandedRowContainer] 内层 High surface 的 shape。
 *
 * 字段组首行圆角上，末行圆角下，中间行 plain；组间间距由外层 Low surface 的
 * [SettingsExpandedInset] 水平 padding + 组间 Low padding 露出。
 */
private val ExpandedInnerTopShape = RoundedCornerShape(topStart = 12.dp, topEnd = 12.dp)
private val ExpandedInnerBottomShape = RoundedCornerShape(bottomStart = 12.dp, bottomEnd = 12.dp)
private val ExpandedInnerFullShape = RoundedCornerShape(12.dp)

private fun expandedInnerShape(
    firstInGroup: Boolean,
    lastInGroup: Boolean,
): Shape =
    when {
        firstInGroup && lastInGroup -> ExpandedInnerFullShape
        firstInGroup -> ExpandedInnerTopShape
        lastInGroup -> ExpandedInnerBottomShape
        else -> RectangleShape
    }

/**
 * #630 评论 5324547885 项1: 行级字段壳 — 纯布局容器，不再每行创建 [Surface]。
 *
 * 旧实现每行一个 [Surface(surfaceContainerHigh)] 导致行间漏出 Low 色带；
 * 现在颜色由 [SettingsExpandedRowContainer] 的 inner High surface 统一提供，
 * 本组件只负责首尾 padding / 语义，本身透明。
 */
@Composable
fun SettingsFieldRowContainer(
    isFirst: Boolean = false,
    isLast: Boolean = false,
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit,
) {
    Box(modifier = modifier.fillMaxWidth()) {
        content()
    }
}

// ── Lazy item content type constants ──

/** Lazy item contentType 常量：搜索入口 */
const val CONTENT_TYPE_SEARCH = "search"

/** Lazy item contentType 常量：组间分隔 spacer */
const val CONTENT_TYPE_SPACER = "spacer"

/** Lazy item contentType 常量：分组标题（surfaceContainerLow） */
const val CONTENT_TYPE_GROUP_HEADER = "group_header"

/** Lazy item contentType 常量：可展开分类标题行（surfaceContainer） */
const val CONTENT_TYPE_CATEGORY_HEADER = "category_header"

/** Lazy item contentType 常量：展开后的子组标题（surfaceContainerHigh） */
const val CONTENT_TYPE_EXPANDED_GROUP_TITLE = "expanded_group_title"

/** Lazy item contentType 常量：展开后的设置字段行（surfaceContainerHigh） */
const val CONTENT_TYPE_EXPANDED_FIELD = "expanded_field"

// ── Lazy item 展开动画 ──

/**
 * #630 评论 5324547885 项2：展开字段统一动画参数。
 *
 * 为所有展开后的 Lazy item 提供一致的 animateItem 参数，
 * fadeIn 100ms / fadeOut 70ms / placement 120ms。
 */
private val SettingsExpandedFadeInSpec: androidx.compose.animation.core.FiniteAnimationSpec<Float> =
    androidx.compose.animation.core.tween(durationMillis = 100)
private val SettingsExpandedFadeOutSpec: androidx.compose.animation.core.FiniteAnimationSpec<Float> =
    androidx.compose.animation.core.tween(durationMillis = 70)
private val SettingsExpandedPlacementSpec:
    androidx.compose.animation.core.FiniteAnimationSpec<androidx.compose.ui.unit.IntOffset> =
    androidx.compose.animation.core.tween(durationMillis = 120)

// #630 评论 5326175206 项2: 只负责 placement 的统一 wrapper — 旧 item 位移动画
private val SettingsMovablePlacementSpec:
    androidx.compose.animation.core.FiniteAnimationSpec<androidx.compose.ui.unit.IntOffset> =
    androidx.compose.animation.core.tween(durationMillis = 120)

/**
 * 展开字段统一动画包裹组件 — 在 LazyListScope extension 的 item {} 块内调用。
 *
 * 必须在 `item(key = ..., type = ...) { SettingsExpandedItemContent { ... } }`
 * 的上下文中使用。在 item {} 块中 `this` 是 `LazyItemScope`，
 * `Modifier.animateItem()` 作为 `LazyItemScope` 的扩展函数可正常调用。
 *
 * 8 个设置 section 文件直接复用，不需要各写一套动画参数。
 */
@Composable
fun LazyItemScope.SettingsExpandedItemContent(
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit,
) {
    Box(
        modifier =
            modifier.animateItem(
                fadeInSpec = SettingsExpandedFadeInSpec,
                fadeOutSpec = SettingsExpandedFadeOutSpec,
                placementSpec = SettingsExpandedPlacementSpec,
            ),
    ) {
        content()
    }
}

/**
 * #630 评论 5326175206 项2: 只负责 placement 的统一 wrapper — 旧 item 位移动画。
 *
 * 给 group spacer、group header、category title 等会因展开/折叠位移的旧 item
 * 提供 120ms placement 动画，不做 fade。
 */
@Composable
fun LazyItemScope.SettingsMovableItemContent(
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit,
) {
    Box(
        modifier =
            modifier.animateItem(
                fadeInSpec = null,
                fadeOutSpec = null,
                placementSpec = SettingsMovablePlacementSpec,
            ),
    ) {
        content()
    }
}

/**
 * #630 评论 5324547885 项1: Lazy item 转场动画辅助 modifier（备选）。
 *
 * 当需要更灵活的 Modifier 拼接时使用，否则优先用 [SettingsExpandedItemContent]。
 */
@Composable
fun LazyItemScope.settingsExpandedItemModifier(): Modifier =
    Modifier.animateItem(
        fadeInSpec = tween(100),
        fadeOutSpec = tween(70),
        placementSpec = tween(120),
    )
