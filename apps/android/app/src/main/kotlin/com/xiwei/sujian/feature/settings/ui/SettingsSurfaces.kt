package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.padding
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
 * - 外层设置组 surfaceContainerLow（[SettingsGroupHeader] + [SettingsExpandedGroupContainer] 外壳）
 * - 展开后的字段组 surfaceContainerHigh（[SettingsExpandedGroupContainer] 内层）
 * - 搜索入口 surfaceContainerHighest（[SettingsSearchEntry]）
 *
 * 层级靠 tonal surface 区分，不靠边框阴影（所有 Surface 显式 shadowElevation = 0.dp），
 * 也不用 primary 文本充当分组层级。
 * 放在 feature/settings/ui 而非 :core:designsystem，因为这些容器只服务设置页结构，
 * 不是通用设计系统组件（AGENTS.md：:core:designsystem 不依赖 :app）。
 *
 * #630 R14：一个真实字段组 = 一个内层 High 卡片 = 一个 Lazy item。
 * 删除逐行 Surface 和 animateItem，字段组内部就是普通 Column。
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
 * 与下方一连串 [SettingsExpandedGroupContainer] 视觉上拼成一张外层大卡片。
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
 * #630 评论 5324547885: 此组件只包分类标题行；展开内容改用 [SettingsExpandedGroupContainer]。
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
 * 此标题属于展开内容卡（[SettingsExpandedGroupContainer] 内的 High surface）内部，
 * 不要在外面再套一层同色 Surface。颜色由父 [SettingsExpandedGroupContainer] 的
 * inner High surface 提供。
 *
 * #630 R14：删除自己的 start/end = 16.dp，只保留纵向间距，
 * 避免标题比字段再多缩一层。
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
            modifier = Modifier.padding(top = 12.dp, bottom = 4.dp),
        )
    }
}

/**
 * #630 评论 5327560790: 单个展开 item 的 closeOuterGroup 决策 — 纯函数。
 *
 * 仅当该分类是组内最后分类 且 该 item 是分类内最后展开 item 时才收口外层组。
 *
 * @param closeOuterGroup 该分类是否为所在 [SettingsGroup] 的最后一个分类
 *        （由 [SettingsRoute] 按 `isLastCategory` 传入）。
 * @param isLastItemOfCategory 该 item 是否为所在分类的最后一个展开 item。
 */
fun expandedItemClosesOuterGroup(
    closeOuterGroup: Boolean,
    isLastItemOfCategory: Boolean,
): Boolean = closeOuterGroup && isLastItemOfCategory

/**
 * #630 评论 5327560790: 外层 Low 圆角决策 — 纯函数。
 *
 * 仅在整个 [SettingsGroup] 最后一行收口画底圆角，其余矩形；
 * 展开内容永不画 [SettingsGroupTopShape]。
 */
fun settingsExpandedOuterShape(closeOuterGroup: Boolean): Shape =
    if (closeOuterGroup) SettingsGroupBottomShape else RectangleShape

// ── 字段组容器（R14） ──

private val ExpandedGroupTopShape = RoundedCornerShape(topStart = 12.dp, topEnd = 12.dp)
private val ExpandedGroupBottomShape = RoundedCornerShape(bottomStart = 12.dp, bottomEnd = 12.dp)
private val ExpandedGroupFullShape = RoundedCornerShape(12.dp)

/**
 * #630 R14：字段组容器 — 一个真实字段组一个 High Surface item，组内多个字段普通布局。
 *
 * 结构：
 * - 外层 surfaceContainerLow 与分类标题同级大卡片（与 [SettingsGroupItemContainer] 一致）
 * - 内层 surfaceContainerHigh 统一 16dp 内容内边距 + 12dp 圆角
 * - 首行上圆角，末行下圆角，中间行直角
 *
 * 解决三个问题：
 * 1. 性能：每个字段组只有一个 Lazy item，组内字段不做 animateItem，
 *    普通滚动时没有 placement 动画持续参与布局
 * 2. 视觉：字段组统一 16dp content padding、12dp 圆角，
 *    标题、输入框、开关、滑块、按钮共用同一条内容起始线
 * 3. M3 冲突：不要魔改 OutlinedTextField 本身，
 *    由本容器统一提供内容内边距，输入框自然对齐到内容起始线
 *
 * @param closeOuterGroup 该行是否需要为整个 [SettingsGroup] 画外层底圆角
 * @param firstInGroup 该字段组是否为所在分类的第一个字段组（控制上圆角）
 * @param lastInGroup 该字段组是否为所在分类的最后一个字段组（控制下圆角）
 */
@Composable
fun SettingsExpandedGroupContainer(
    closeOuterGroup: Boolean,
    firstInGroup: Boolean,
    lastInGroup: Boolean,
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit,
) {
    val outerShape = settingsExpandedOuterShape(closeOuterGroup)
    val innerShape =
        when {
            firstInGroup && lastInGroup -> ExpandedGroupFullShape
            firstInGroup -> ExpandedGroupTopShape
            lastInGroup -> ExpandedGroupBottomShape
            else -> RectangleShape
        }
    Surface(
        modifier = modifier.fillMaxWidth(),
        color = MaterialTheme.colorScheme.surfaceContainerLow,
        shape = outerShape,
        shadowElevation = 0.dp,
    ) {
        Surface(
            modifier =
                Modifier
                    .fillMaxWidth()
                    .padding(horizontal = SettingsExpandedInset),
            color = MaterialTheme.colorScheme.surfaceContainerHigh,
            shape = innerShape,
            shadowElevation = 0.dp,
        ) {
            Column(
                modifier = Modifier.padding(16.dp),
            ) {
                content()
            }
        }
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

/** Lazy item contentType 常量：展开后的字段组（surfaceContainerHigh） */
const val CONTENT_TYPE_EXPANDED_FIELD_GROUP = "expanded_field_group"
