package com.xiwei.sujian.app.navigation

import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.stringResource
import com.xiwei.sujian.R
import com.xiwei.sujian.app.SujianAppState
import com.xiwei.sujian.app.di.SujianAppDependencies
import com.xiwei.sujian.app.presentation.screen.SujianChromeAction
import com.xiwei.sujian.app.presentation.screen.SujianChromeSpec
import com.xiwei.sujian.core.designsystem.component.SujianIconButton
import com.xiwei.sujian.core.designsystem.icon.SujianIcons
import com.xiwei.sujian.core.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.feature.project.ui.ProjectNavigationState
import com.xiwei.sujian.feature.project.ui.WorkspaceLocation
import com.xiwei.sujian.feature.project.ui.guardedBack
import com.xiwei.sujian.feature.sync.data.model.SyncIndicatorState
import com.xiwei.sujian.feature.sync.data.model.SyncTrigger
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch

@Stable
internal class SujianTopBarInfo(
    val title: String,
    val navigationIcon: ImageVector?,
    val onNavigationClick: (() -> Unit)?,
    val actions: @Composable () -> Unit,
    val containerColor: Color,
)

/** 顶栏操作所需的环境依赖 — 打包传递，避免函数参数超出门禁阈值。 */
internal data class SujianTopBarEnv(
    val syncState: SyncIndicatorState,
    val coroutineScope: CoroutineScope,
    val deps: SujianAppDependencies,
    val topLevelBackStack: SujianTopLevelBackStack,
)

@Composable
private fun rememberSujianTopBarTitle(
    currentRoute: SujianRoute,
    appState: SujianAppState,
    workspaceLocation: WorkspaceLocation,
): String =
    when (currentRoute) {
        is SujianRoute.Works ->
            when (workspaceLocation) {
                // #625 第二段：作品根页（ProjectList 位置）标题用「素笺」，
                // 进入 ChapterTree / Editor 后用项目标题。
                WorkspaceLocation.ProjectList -> stringResource(id = R.string.title_sujian)
                is WorkspaceLocation.ChapterTree, is WorkspaceLocation.Editor ->
                    appState.currentProjectTitle.ifEmpty { stringResource(id = R.string.title_projects) }
            }
        is SujianRoute.StarMap -> stringResource(id = R.string.title_starmap)
        is SujianRoute.Stats -> stringResource(id = R.string.title_stats)
        is SujianRoute.Settings -> stringResource(id = R.string.action_settings)
    }

/** 顶栏返回逻辑 — 先生成唯一的返回动作，再决定是否显示图标（#597 评论问题三）。
 * 工作区内的返回（正文→章节树→作品列表）统一走 [ProjectNavigationState.back]，
 * 与系统返回、页面返回共用同一个工作区 navigator 历史（返回历史始终同一份）。 */
@Composable
private fun rememberSujianTopBarNavigation(
    currentRoute: SujianRoute,
    env: SujianTopBarEnv,
    workspaceNavState: ProjectNavigationState,
): Pair<ImageVector?, (() -> Unit)?> {
    val onNavigationClick: (() -> Unit)? =
        when (currentRoute) {
            is SujianRoute.Settings -> {
                { env.topLevelBackStack.removeLastOrNull() }
            }
            is SujianRoute.Works -> {
                if (workspaceNavState.canNavigateBack) {
                    {
                        env.coroutineScope.launch {
                            // #624 评论12 第1项：顶栏返回统一走 guardedBack —
                            // 先保存活动正文（ActiveDocumentGate flush），成功才导航离开。
                            workspaceNavState.guardedBack()
                        }
                    }
                } else {
                    null
                }
            }
            is SujianRoute.StarMap -> null
            is SujianRoute.Stats -> null
        }
    val navigationIcon = if (onNavigationClick != null) SujianIcons.ArrowBack else null
    return navigationIcon to onNavigationClick
}

/** 顶栏右侧操作 — 顺序由 Core screen contract 的 HeaderTrailing order 决定（#610）：
 * 作品页/写作区依次提供 同步状态、搜索、设置（视觉从右往左为 设置/搜索/同步）。 */

@Composable
private fun rememberSujianTopBarActions(
    currentRoute: SujianRoute,
    chrome: SujianChromeSpec,
    env: SujianTopBarEnv,
): @Composable () -> Unit {
    // #624 评论5：CompositionLocal 只能在组合上下文读取 — 先取窗口宿主，
    // 供 Settings onClick 在导航前立刻收 IME。
    val editorWindowHost = com.xiwei.sujian.feature.editor.ui.LocalEditorWindowHost.current
    val actions: @Composable () -> Unit = {
        if (currentRoute is SujianRoute.Works) {
            chrome.actions.forEach { action ->
                when (action) {
                    SujianChromeAction.Settings ->
                        SujianIconButton(
                            onClick = {
                                // #624 评论5：进入设置前先立刻收 IME（清焦点 + 隐藏输入法），
                                // 再切页面 — 不等 AndroidView onRelease 晚一拍才 hide keyboard。
                                editorWindowHost?.dismissImeForNavigation()
                                env.topLevelBackStack.add(SujianRoute.Settings)
                            },
                            icon = SujianIcons.Settings,
                            contentDescription = stringResource(id = R.string.action_settings),
                            semanticId = SujianSemanticIds.NavigationSettings,
                        )
                    SujianChromeAction.Search ->
                        // #624 评论6：写作页/作品页搜索入口恢复可用状态 — #477 的全局搜索
                        // 界面接入前 onClick 保持空动作，但图标不能从产品契约消失。
                        SujianIconButton(
                            onClick = { },
                            icon = SujianIcons.Search,
                            contentDescription = stringResource(id = R.string.cd_search_dev),
                            semanticId = SujianSemanticIds.NavigationSearch,
                        )
                    SujianChromeAction.Sync ->
                        SujianIconButton(
                            onClick = rememberSujianManualSyncOnClick(env),
                            icon =
                                when (env.syncState) {
                                    SyncIndicatorState.Unconfigured -> SujianIcons.CloudOff
                                    SyncIndicatorState.Syncing -> SujianIcons.CloudSync
                                    SyncIndicatorState.Synced -> SujianIcons.CloudDone
                                    SyncIndicatorState.Failed -> SujianIcons.CloudError
                                },
                            contentDescription = stringResource(id = R.string.cd_sync_manual),
                            semanticId = SujianSemanticIds.NavigationSync,
                        )
                }
            }
        }
    }
    return actions
}

/** #600：手动同步 onClick — 提取为独立函数降低 rememberSujianTopBarActions 认知复杂度。 */
private fun rememberSujianManualSyncOnClick(env: SujianTopBarEnv): () -> Unit =
    {
        env.coroutineScope.launch {
            // sync 已改为 per-project — 手动同步针对当前活动作品。
            val pid = com.xiwei.sujian.app.state.ActiveProjectGate.currentProjectId()
            if (pid != null) {
                env.deps.syncCoordinator.runSync(SyncTrigger.Manual, pid)
            }
        }
    }

/** 顶栏信息 — 标题/返回/操作/透明背景 全部由同一份 [SujianChromeSpec] 决策驱动。 */
@Composable
internal fun rememberSujianTopBarInfo(
    currentRoute: SujianRoute,
    appState: SujianAppState,
    chrome: SujianChromeSpec,
    env: SujianTopBarEnv,
    workspaceNavState: ProjectNavigationState,
): SujianTopBarInfo {
    val topBarNavigation =
        rememberSujianTopBarNavigation(currentRoute, env, workspaceNavState)
    return SujianTopBarInfo(
        title =
            if (chrome.showTitle) {
                // #625 第二段：顶栏标题按业务位置区分「素笺」/项目标题。
                rememberSujianTopBarTitle(currentRoute, appState, workspaceNavState.currentLocation)
            } else {
                ""
            },
        navigationIcon = topBarNavigation.first,
        onNavigationClick = topBarNavigation.second,
        actions = rememberSujianTopBarActions(currentRoute, chrome, env),
        containerColor =
            if (chrome.appBarTransparent) {
                Color.Transparent
            } else {
                MaterialTheme.colorScheme.surface
            },
    )
}
