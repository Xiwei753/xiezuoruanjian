package com.xiwei.sujian.feature.settings.ui

import android.annotation.SuppressLint
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import com.xiwei.sujian.R
import com.xiwei.sujian.app.di.LocalSujianAppDependencies
import com.xiwei.sujian.app.navigation.SettingsSection
import com.xiwei.sujian.core.designsystem.icon.SujianIcons
import com.xiwei.sujian.core.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.core.designsystem.theme.LocalSujianDimensions

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
 * 每个分类是一个 [SettingsExpandableSection]：点击标题行切换展开/折叠，
 * 展开内容在行内用 [AnimatedVisibility] 呈现，不导航到子页面，
 * 不建立第二套页面导航状态。展开状态用 [rememberSaveable] 跨配置变更保留。
 *
 * context.getString 在协程 collect 回调中解析错误文案，无法用 stringResource，
 * 因此本文件带单规则 SuppressLint。
 */
@SuppressLint("LocalContextGetResourceValueCall")
@Composable
fun SettingsRoute(modifier: Modifier = Modifier) {
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

    Box(modifier = modifier.fillMaxSize()) {
        LazyColumn(
            modifier = Modifier.fillMaxSize().testTag(SujianSemanticIds.SettingsScreen),
            contentPadding = PaddingValues(vertical = dims.space8),
        ) {
            SettingsGroup.entries.forEach { group ->
                val categories = settingsCategories.filter { it.group == group }
                if (categories.isEmpty()) return@forEach
                item(key = "settings_group_${group.name}") {
                    Text(
                        text = stringResource(id = group.titleResId),
                        style = MaterialTheme.typography.titleSmall,
                        color = MaterialTheme.colorScheme.primary,
                        modifier = Modifier.padding(horizontal = dims.space16, vertical = dims.space8),
                    )
                }
                items(categories, key = { it.section.name }) { category ->
                    SettingsCategoryItem(
                        category = category,
                        vm = vm,
                        expanded = expansionState.isExpanded(category.section),
                        onExpandedChange = { isExpanded ->
                            expansionState.setExpanded(category.section, isExpanded)
                        },
                        onIntent = vm::handleIntent,
                    )
                }
            }
        }
        androidx.compose.material3.SnackbarHost(
            hostState = snackbarHostState,
            modifier = Modifier.align(Alignment.BottomCenter),
        )
    }
}

/**
 * 单个设置分类的折叠面板项 — 从 [SettingsRoute] 中提取以控制函数长度与认知复杂度。
 *
 * #618 六：不再接收整份 [SettingsUiState]；头部当前值与展开内容分别只订阅
 * 本分类对应的 section state，其它分类的状态变化不会让本项重组。
 */
@Composable
private fun SettingsCategoryItem(
    category: SettingsCategory,
    vm: SettingsViewModel,
    expanded: Boolean,
    onExpandedChange: (Boolean) -> Unit,
    onIntent: (SettingsIntent) -> Unit,
) {
    SettingsExpandableSection(
        title = stringResource(id = category.titleResId),
        summary = settingsCategorySummary(category),
        value = settingsCategoryValue(category, vm),
        expanded = expanded,
        onExpandedChange = onExpandedChange,
        content = {
            when (category.section) {
                SettingsSection.Appearance -> {
                    val state by vm.appearanceState.collectAsStateWithLifecycle()
                    AppearanceSettings(state = state, onIntent = onIntent)
                }
                SettingsSection.Editor -> {
                    val state by vm.editorState.collectAsStateWithLifecycle()
                    EditorSettings(state = state, onIntent = onIntent)
                }
                SettingsSection.Save -> {
                    val state by vm.saveState.collectAsStateWithLifecycle()
                    SaveSettings(state = state, onIntent = onIntent)
                }
                SettingsSection.Sync -> {
                    val state by vm.syncState.collectAsStateWithLifecycle()
                    SyncSettings(state = state, onIntent = onIntent)
                }
                SettingsSection.Ai -> {
                    val state by vm.aiState.collectAsStateWithLifecycle()
                    AiSettings(state = state, onIntent = onIntent)
                }
                SettingsSection.Diagnostics -> {
                    val state by vm.diagnosticsState.collectAsStateWithLifecycle()
                    DiagnosticsSettings(state = state, onIntent = onIntent)
                }
                SettingsSection.Laboratory -> {
                    val state by vm.laboratoryState.collectAsStateWithLifecycle()
                    LaboratorySettings(state = state, onIntent = onIntent)
                }
                SettingsSection.About -> {
                    val state by vm.aboutState.collectAsStateWithLifecycle()
                    AboutSettings(state = state, onIntent = onIntent)
                }
            }
        },
    )
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
 * 与 [settingsCategoryValue] 的“真实当前值”独立展示，不再二选一。
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
            toggleValue(state.projectSyncConfig.enabled == true)
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
