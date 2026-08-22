package com.xiwei.sujian.feature.settings.ui

import android.annotation.SuppressLint
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.tween
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
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
import androidx.compose.ui.platform.LocalView
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
 * 每个分类是一个 [SettingsExpandableCategory]（独占母项圆角 + 展开动画）。
 * 展开状态用 [rememberSaveable] 跨配置变更保留。
 *
 * #633 评论 5379618506：改成有限内容滚动 — Column + verticalScroll。
 * 删除惰性列表 API（item / key / contentType）与旧 Settings 滚动列表组件、惰性列表状态记忆入口。
 * 一个 category = 一个 Transition + 一个 AnimatedVisibility（在 [SettingsExpandableCategory] 内）。
 *
 * context.getString 在协程 collect 回调中解析错误文案，无法用 stringResource，
 * 因此本文件带单规则 SuppressLint。
 */
@SuppressLint("LocalContextGetResourceValueCall")
@Composable
fun SettingsRoute(modifier: Modifier = Modifier) {
    val view = LocalView.current
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
    val scrollState = rememberScrollState()
    SettingsScrollJankTracker(view, scrollState)

    Box(
        modifier =
            modifier
                .fillMaxSize()
                .graphicsLayer {
                    alpha = entryAlpha.value
                    translationY = (1f - entryAlpha.value) * 8.dp.toPx()
                },
    ) {
        Column(
            modifier =
                Modifier
                    .fillMaxSize()
                    .verticalScroll(scrollState)
                    .padding(horizontal = dims.space16, vertical = dims.space8)
                    .testTag(SujianSemanticIds.SettingsScreen),
        ) {
            SettingsSearchEntry(onClick = {})
            Spacer(Modifier.height(dims.space16))

            SettingsGroupList(expansionState, vm)
        }
        androidx.compose.material3.SnackbarHost(
            hostState = snackbarHostState,
            modifier = Modifier.align(Alignment.BottomCenter),
        )
    }
}

/**
 * #630 评论25 项2 / #631：Settings 页滚动 JankStats interaction 上下文追踪。
 */
@Composable
private fun SettingsScrollJankTracker(
    view: android.view.View,
    scrollState: androidx.compose.foundation.ScrollState,
) {
    LaunchedEffect(view, scrollState) {
        val state =
            androidx.metrics.performance.PerformanceMetricsState
                .getHolderForHierarchy(view)
                ?.state
        try {
            snapshotFlow { scrollState.isScrollInProgress }
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
}

/**
 * #633 评论 5379618506：按 [SettingsGroup] 渲染分类列表。
 * 每组带 header，组间 spacer；每个分类是一个 [SettingsExpandableCategory]。
 */
@Composable
private fun SettingsGroupList(
    expansionState: SettingsExpansionState,
    vm: SettingsViewModel,
) {
    val dims = LocalSujianDimensions.current
    var firstGroup = true
    SettingsGroup.entries.forEach { group ->
        val categories = settingsCategories.filter { it.group == group }
        if (categories.isEmpty()) return@forEach

        if (!firstGroup) Spacer(Modifier.height(dims.space16))
        firstGroup = false

        SettingsGroupHeader(title = stringResource(id = group.titleResId))

        categories.forEachIndexed { index, category ->
            val expanded = expansionState.isExpanded(category.section)
            val categoryTitle = stringResource(id = category.titleResId)

            SettingsExpandableCategory(
                title = categoryTitle,
                headerInfo =
                    SettingsCategoryHeaderInfo(
                        summary = settingsCategorySummary(category),
                        value = settingsCategoryValue(category, vm),
                        isLastCategory = index == categories.lastIndex,
                    ),
                expanded = expanded,
                onExpandedChange = { next ->
                    com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.settingsSection(
                        categoryTitle,
                        next,
                    )
                    expansionState.setExpanded(category.section, next)
                },
            ) {
                SettingsCategoryContent(category.section, vm)
            }
        }
    }
}

/**
 * #633 评论 5379618506：把 section 路由到对应 content Composable。
 */
@Composable
private fun SettingsCategoryContent(
    section: SettingsSection,
    vm: SettingsViewModel,
) {
    when (section) {
        SettingsSection.Appearance -> AppearanceSettingsContent(vm)
        SettingsSection.Editor -> EditorSettingsContent(vm)
        SettingsSection.Save -> SaveSettingsContent(vm)
        SettingsSection.Sync -> SyncSettingsContent(vm)
        SettingsSection.Ai -> AiSettingsContent(vm)
        SettingsSection.Diagnostics -> DiagnosticsSettingsContent(vm)
        SettingsSection.Laboratory -> LaboratorySettingsContent(vm)
        SettingsSection.About -> AboutSettingsContent(vm)
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
