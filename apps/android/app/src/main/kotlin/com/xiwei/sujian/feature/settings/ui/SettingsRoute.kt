package com.xiwei.sujian.feature.settings.ui

import android.annotation.SuppressLint
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.tween
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import com.xiwei.sujian.R
import com.xiwei.sujian.app.di.LocalSujianAppDependencies
import com.xiwei.sujian.app.navigation.SettingsSection
import com.xiwei.sujian.core.designsystem.icon.SujianIcons
import com.xiwei.sujian.core.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.core.designsystem.theme.LocalSujianDimensions
import com.xiwei.sujian.core.designsystem.theme.SujianDimensions
import kotlinx.coroutines.flow.distinctUntilChanged

/** JankStats PerformanceMetricsState key for interaction context (settings_scroll / settings_expand / settings_collapse). */
private const val SETTINGS_JANK_INTERACTION_KEY = "interaction"

/**
 * 设置列表分组（手机列表按组呈现，组内每一项显示标题、说明或当前值与展开箭头）。
 */
enum class SettingsGroup(val titleResId: Int) {
    Appearance(R.string.pref_group_appearance),
    Writing(R.string.pref_group_writing),
    DataSync(R.string.pref_group_data_sync),
    Advanced(R.string.pref_group_advanced),
    About(R.string.pref_group_about),
}

data class SettingsCategory(
    val section: SettingsSection,
    val titleResId: Int,
    val icon: ImageVector,
    val group: SettingsGroup,
)

val settingsCategories =
    listOf(
        SettingsCategory(
            SettingsSection.Appearance,
            R.string.pref_category_appearance,
            SujianIcons.Palette,
            SettingsGroup.Appearance,
        ),
        SettingsCategory(
            SettingsSection.Editor,
            R.string.pref_category_editor,
            SujianIcons.Edit,
            SettingsGroup.Writing,
        ),
        SettingsCategory(SettingsSection.Save, R.string.pref_category_save, SujianIcons.Save, SettingsGroup.Writing),
        SettingsCategory(
            SettingsSection.Sync,
            R.string.pref_category_sync,
            SujianIcons.CloudSync,
            SettingsGroup.DataSync,
        ),
        SettingsCategory(
            SettingsSection.Ai,
            R.string.pref_category_ai,
            SujianIcons.AutoStories,
            SettingsGroup.Advanced,
        ),
        SettingsCategory(
            SettingsSection.Diagnostics,
            R.string.pref_category_diagnostics,
            SujianIcons.BugReport,
            SettingsGroup.Advanced,
        ),
        SettingsCategory(
            SettingsSection.Laboratory,
            R.string.pref_category_laboratory,
            SujianIcons.Science,
            SettingsGroup.Advanced,
        ),
        SettingsCategory(SettingsSection.About, R.string.pref_category_about, SujianIcons.Info, SettingsGroup.About),
    )

/**
 * 设置一级 destination 的唯一内容 — 同页折叠面板。
 *
 * 每个分类是一个标题 item + 展开后的字段 item。展开状态用 [rememberSaveable] 跨配置变更保留。
 *
 * #630 评论 5312333045 项3: 扁平 LazyColumn — 分类标题是一条 item，展开后的每个设置
 * 分类也是独立 item，并且都有稳定 key。不再在 SettingsCategoryItem 里用
 * content: @Composable () Unit 包住整个分类。
 *
 * context.getString 在协程 collect 回调中解析错误文案，无法用 stringResource，
 * 因此本文件带单规则 SuppressLint。
 */
@SuppressLint("LocalContextGetResourceValueCall")
@Composable
fun SettingsRoute(modifier: Modifier = Modifier) {
    val view = androidx.compose.ui.platform.LocalView.current
    // #631: screen 标记移至 SujianNavigationSuite，SettingsRoute 不再写 screen。
    val deps = LocalSujianAppDependencies.current
    val vm: SettingsViewModel =
        viewModel(
            factory =
                SettingsViewModel.Factory(
                    deps.settingsRepository,
                    deps.themeRepository,
                    deps.syncRepository,
                    deps.syncCoordinator,
                ),
        )
    val snackbarHostState = rememberSettingsSnackbarHost(vm)
    val dims = LocalSujianDimensions.current

    val expansionState =
        rememberSaveable(saver = SettingsExpansionState.Saver) {
            SettingsExpansionState()
        }

    // #630 评论5324547885项3：Settings 自身绘制层动画 — 120~140ms alpha + translationY。
    // 与 SujianTopLevelSwitchMotion 同路线：只动画 graphicsLayer，不改布局尺寸。
    // 旧页（Works）由 noPageTransitionMetadata 直接退出，无双页 crossfade。
    val entryAlpha = remember { Animatable(0f) }
    LaunchedEffect(Unit) {
        entryAlpha.animateTo(
            targetValue = 1f,
            animationSpec = tween(durationMillis = 130),
        )
    }

    // #630 评论25 项2 / #631：Settings 页 JankStats interaction 上下文。
    // 用 snapshotFlow 追踪滚动状态，finally 里确保 removeState 防止泄漏。
    val listState = rememberLazyListState()
    LaunchedEffect(view, listState) {
        val state =
            androidx.metrics.performance.PerformanceMetricsState
                .getHolderForHierarchy(view)
                ?.state
        try {
            snapshotFlow { listState.isScrollInProgress }
                .distinctUntilChanged()
                .collect { scrolling ->
                    if (scrolling) {
                        state?.putState(SETTINGS_JANK_INTERACTION_KEY, "settings_scroll")
                    } else {
                        state?.removeState(SETTINGS_JANK_INTERACTION_KEY)
                    }
                }
        } finally {
            state?.removeState(SETTINGS_JANK_INTERACTION_KEY)
        }
    }

    Box(
        modifier =
            modifier
                .fillMaxSize()
                .graphicsLayer {
                    alpha = entryAlpha.value
                    translationY = (1f - entryAlpha.value) * 8.dp.toPx()
                },
    ) {
        SettingsLazyColumn(
            vm = vm,
            expansionState = expansionState,
            dims = dims,
            listState = listState,
            onInteraction = { interaction ->
                val holder =
                    androidx.metrics.performance.PerformanceMetricsState
                        .getHolderForHierarchy(view)
                holder?.state?.putSingleFrameState(SETTINGS_JANK_INTERACTION_KEY, interaction)
            },
        )
        androidx.compose.material3.SnackbarHost(
            hostState = snackbarHostState,
            modifier = Modifier.align(Alignment.BottomCenter),
        )
    }
}

/**
 * 设置页扁平 LazyColumn — 从 SettingsRoute 提取以降低认知复杂度。
 *
 * #630 评论12 项1: 按 category 顺序交错注册 item，
 * 一个 category 标题后面立刻插自己的展开项，再进入下一个 category。
 *
 * LazyColumn 的 forEach + when 结构天然复杂，但每个分支都是独立的 item 注册，
 * 无法进一步拆分而不破坏扁平 Lazy 的语义。
 */
@Suppress("CognitiveComplexMethod", "CyclomaticComplexMethod")
@Composable
private fun SettingsLazyColumn(
    vm: SettingsViewModel,
    expansionState: SettingsExpansionState,
    dims: SujianDimensions,
    listState: androidx.compose.foundation.lazy.LazyListState =
        androidx.compose.foundation.lazy.rememberLazyListState(),
    onInteraction: (String) -> Unit = {},
) {
    LazyColumn(
        state = listState,
        modifier = Modifier.fillMaxSize().testTag(SujianSemanticIds.SettingsScreen),
        contentPadding = PaddingValues(horizontal = dims.space16, vertical = dims.space8),
    ) {
        // 设置页顶部紧凑全局搜索入口
        item(key = "settings_search_entry", contentType = CONTENT_TYPE_SEARCH) {
            SettingsSearchEntry(onClick = {})
        }
        item(key = "settings_search_entry_bottom_spacer", contentType = CONTENT_TYPE_SPACER) {
            Spacer(Modifier.height(dims.space16))
        }
        var firstGroupShown = false
        SettingsGroup.entries.forEach { group ->
            val categories = settingsCategories.filter { it.group == group }
            if (categories.isEmpty()) return@forEach
            if (firstGroupShown) {
                item(key = "settings_group_spacer_${group.name}", contentType = CONTENT_TYPE_SPACER) {
                    Spacer(Modifier.height(dims.space16))
                }
            }
            firstGroupShown = true
            item(key = "settings_group_${group.name}", contentType = CONTENT_TYPE_GROUP_HEADER) {
                SettingsGroupHeader(title = stringResource(id = group.titleResId))
            }
            categories.forEachIndexed { index, category ->
                val isLastCategory = index == categories.lastIndex
                val isExpanded = expansionState.isExpanded(category.section)
                item(key = "settings_category_${category.section.name}", contentType = CONTENT_TYPE_CATEGORY_HEADER) {
                    SettingsGroupItemContainer(isLast = isLastCategory && !isExpanded) {
                        SettingsExpandableSection(
                            title = stringResource(id = category.titleResId),
                            summary = settingsCategorySummary(category),
                            value = settingsCategoryValue(category, vm),
                            expanded = isExpanded,
                            onExpandedChange = { newValue ->
                                expansionState.setExpanded(category.section, newValue)
                                onInteraction(
                                    if (newValue) "settings_expand" else "settings_collapse",
                                )
                            },
                        )
                    }
                }
                if (isExpanded) {
                    // #630 评论13 项2: 扁平 LazyColumn — 每个设置字段是独立 item，
                    // 直接向父 LazyListScope 注册，不再包在 ExpandedSettingsContent 里。
                    // 注意：collectAsStateWithLifecycle() 必须在 item 块内调用，
                    // 因为 LazyListScope 扩展函数不是 @Composable 上下文。
                    when (category.section) {
                        SettingsSection.Appearance -> appearanceSettingsItems(vm, isLastCategory)
                        SettingsSection.Editor -> editorSettingsItems(vm, isLastCategory)
                        SettingsSection.Save -> saveSettingsItems(vm, isLastCategory)
                        SettingsSection.Sync -> syncSettingsItems(vm, isLastCategory)
                        SettingsSection.Ai -> aiSettingsItems(vm, isLastCategory)
                        SettingsSection.Diagnostics -> diagnosticsSettingsItems(vm, isLastCategory)
                        SettingsSection.Laboratory -> laboratorySettingsItems(vm, isLastCategory)
                        SettingsSection.About -> aboutSettingsItems(vm, isLastCategory)
                    }
                }
            }
        }
    }
}

/**
 * 收集设置保存失败事件并在底部 snackbar 展示（协程上下文用
 * context.getString，非 Composable 上下文无法用 stringResource）。
 *
 * context.getString 在协程 collect 回调中解析错误文案，无法用 stringResource，
 * 因此本函数带单规则 SuppressLint。
 */
@SuppressLint("LocalContextGetResourceValueCall")
@Composable
private fun rememberSettingsSnackbarHost(vm: SettingsViewModel): androidx.compose.material3.SnackbarHostState {
    val context = LocalContext.current
    val snackbarHostState = remember { androidx.compose.material3.SnackbarHostState() }
    LaunchedEffect(Unit) {
        vm.saveFailureEvents.collect { errorResId ->
            snackbarHostState.showSnackbar(context.getString(errorResId))
        }
    }
    return snackbarHostState
}

/**
 * 设置列表项的功能说明（静态文案，不冒充状态）：
 * 与 [settingsCategoryValue] 的"真实当前值"独立展示，不再二选一。
 */
@Composable
internal fun settingsCategorySummary(category: SettingsCategory): String? =
    when (category.section) {
        SettingsSection.Appearance -> stringResource(id = R.string.pref_summary_appearance)
        SettingsSection.Editor -> stringResource(id = R.string.pref_summary_editor)
        SettingsSection.Save -> stringResource(id = R.string.pref_summary_save)
        SettingsSection.Sync -> stringResource(id = R.string.pref_summary_sync)
        SettingsSection.Ai -> stringResource(id = R.string.pref_summary_ai)
        SettingsSection.Diagnostics -> stringResource(id = R.string.pref_summary_diagnostics)
        SettingsSection.Laboratory -> stringResource(id = R.string.pref_summary_laboratory)
        SettingsSection.About -> stringResource(id = R.string.pref_summary_about)
    }

/**
 * 设置列表项的真实当前值：全部来自本分类对应的 section state（#618 六），
 * 保存后随状态即时更新；无当前值可展示的分类（实验室/关于）返回 null，只保留功能说明。
 */
@Composable
internal fun settingsCategoryValue(
    category: SettingsCategory,
    vm: SettingsViewModel,
): String? =
    when (category.section) {
        SettingsSection.Appearance -> {
            val state by vm.appearanceState.collectAsStateWithLifecycle()
            appearanceModeValue(state.appearanceMode)
        }
        SettingsSection.Editor -> {
            val state by vm.editorState.collectAsStateWithLifecycle()
            fontSizeValue(state.fontSize)
        }
        SettingsSection.Save -> {
            val state by vm.saveState.collectAsStateWithLifecycle()
            toggleValue(state.autoSaveEnabled)
        }
        SettingsSection.Sync -> {
            val state by vm.syncState.collectAsStateWithLifecycle()
            toggleValue(state.syncConfig.enabled == true)
        }
        SettingsSection.Ai -> {
            val state by vm.aiState.collectAsStateWithLifecycle()
            toggleValue(state.enabled)
        }
        SettingsSection.Diagnostics -> {
            val state by vm.diagnosticsState.collectAsStateWithLifecycle()
            toggleValue(state.enabled)
        }
        SettingsSection.Laboratory,
        SettingsSection.About,
        -> null
    }

@Composable
private fun appearanceModeValue(mode: String): String =
    when (mode) {
        "light" -> stringResource(id = R.string.theme_light)
        "dark" -> stringResource(id = R.string.theme_dark)
        else -> stringResource(id = R.string.theme_system)
    }

@Composable
private fun fontSizeValue(fontSize: Float): String =
    stringResource(
        id = R.string.pref_value_font_size,
        if (fontSize % 1f == 0f) fontSize.toInt().toString() else fontSize.toString(),
    )

@Composable
private fun toggleValue(enabled: Boolean): String =
    if (enabled) {
        stringResource(id = R.string.pref_state_on)
    } else {
        stringResource(id = R.string.pref_state_off)
    }
