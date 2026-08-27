package com.xiwei.sujian.app.navigation

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.consumeWindowInsets
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationRail
import androidx.compose.material3.NavigationRailItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.app.presentation.layout.AndroidLayoutSpec
import com.xiwei.sujian.app.presentation.screen.SujianChromeSpec
import com.xiwei.sujian.core.designsystem.component.SujianSnackbar
import com.xiwei.sujian.core.designsystem.component.SujianTopAppBar
import com.xiwei.sujian.core.designsystem.testing.SujianSemanticIds

/** compact 底栏 — 一级入口只保留 作品/星图/统计（#597 评论问题一）。 */
@Composable
private fun SujianCompactBottomBar(
    currentTopDestination: SujianDestination,
    onTopLevelSelected: (SujianDestination) -> Unit,
) {
    NavigationBar(
        windowInsets = WindowInsets(0.dp),
        modifier = Modifier.testTag(SujianSemanticIds.NavigationBar),
    ) {
        SujianDestination.entries.forEach { destination ->
            NavigationBarItem(
                selected = currentTopDestination == destination,
                onClick = { onTopLevelSelected(destination) },
                icon = {
                    Icon(
                        imageVector =
                            if (currentTopDestination == destination) {
                                destination.selectedIcon
                            } else {
                                destination.unselectedIcon
                            },
                        contentDescription = stringResource(id = destination.labelResId),
                    )
                },
                label = { Text(text = stringResource(id = destination.labelResId)) },
                modifier = Modifier.navItemModifier(destination),
            )
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun SujianCompactNavScaffold(
    modifier: Modifier,
    topBarInfo: SujianTopBarInfo,
    showTopBar: Boolean,
    snackbarHostState: SnackbarHostState,
    bottomBar: @Composable () -> Unit,
    navDisplayContent: @Composable () -> Unit,
) {
    Scaffold(
        modifier = modifier.fillMaxSize(),
        topBar = {
            // #628 验收点 6：Workbench Writing 由工作台自己拥有顶部工具栏，
            // 外层 app shell 不再额外画通用顶栏。窄屏 SinglePane 仍画透明 App bar。
            if (showTopBar) {
                SujianTopAppBar(
                    title = topBarInfo.title,
                    navigationIcon = topBarInfo.navigationIcon,
                    onNavigationClick = topBarInfo.onNavigationClick,
                    actions = topBarInfo.actions,
                    containerColor = topBarInfo.containerColor,
                )
            }
        },
        bottomBar = bottomBar,
        snackbarHost = {
            SnackbarHost(hostState = snackbarHostState) { data ->
                SujianSnackbar(data = data)
            }
        },
        containerColor = MaterialTheme.colorScheme.background,
        contentWindowInsets = WindowInsets.safeDrawing,
    ) { innerPadding ->
        Box(
            modifier =
                Modifier
                    .fillMaxSize()
                    .padding(innerPadding)
                    .consumeWindowInsets(innerPadding),
        ) {
            navDisplayContent()
        }
    }
}

/** 宽窗口一级导航 — NavigationRail 容器（#597 九：NavigationRail 稳定语义 ID）。 */
@Composable
private fun SujianWideRail(
    currentTopDestination: SujianDestination,
    onTopLevelSelected: (SujianDestination) -> Unit,
) {
    NavigationRail(
        modifier = Modifier.fillMaxHeight().testTag(SujianSemanticIds.NavigationRail),
        windowInsets = WindowInsets(0.dp),
    ) {
        SujianDestination.entries.forEach { destination ->
            NavigationRailItem(
                selected = currentTopDestination == destination,
                onClick = { onTopLevelSelected(destination) },
                icon = {
                    Icon(
                        imageVector =
                            if (currentTopDestination == destination) {
                                destination.selectedIcon
                            } else {
                                destination.unselectedIcon
                            },
                        contentDescription = stringResource(id = destination.labelResId),
                    )
                },
                label = { Text(text = stringResource(id = destination.labelResId)) },
                modifier = Modifier.navItemModifier(destination),
            )
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun SujianWideNavScaffold(
    modifier: Modifier,
    topBarInfo: SujianTopBarInfo,
    showTopBar: Boolean,
    snackbarHostState: SnackbarHostState,
    rail: (@Composable () -> Unit)?,
    navDisplayContent: @Composable () -> Unit,
) {
    Scaffold(
        modifier = modifier.fillMaxSize(),
        topBar = {
            // #628 验收点 6：Workbench Writing 由工作台自己拥有顶部工具栏，
            // 外层 app shell 不再额外画通用顶栏。
            if (showTopBar) {
                SujianTopAppBar(
                    title = topBarInfo.title,
                    navigationIcon = topBarInfo.navigationIcon,
                    onNavigationClick = topBarInfo.onNavigationClick,
                    actions = topBarInfo.actions,
                    containerColor = topBarInfo.containerColor,
                )
            }
        },
        snackbarHost = {
            SnackbarHost(hostState = snackbarHostState) { data ->
                SujianSnackbar(data = data)
            }
        },
        containerColor = MaterialTheme.colorScheme.background,
        contentWindowInsets = WindowInsets.safeDrawing,
    ) { innerPadding ->
        Row(
            modifier =
                Modifier
                    .fillMaxSize()
                    .padding(innerPadding)
                    .consumeWindowInsets(innerPadding),
        ) {
            rail?.invoke()
            Box(
                modifier =
                    Modifier
                        .weight(1f)
                        .fillMaxHeight(),
            ) {
                navDisplayContent()
            }
        }
    }
}

/** 外层 scaffold 的顶栏/Snackbar 配置 — 打包传递，避免函数参数超出门禁阈值。 */
internal data class SujianNavScaffoldChrome(
    val topBarInfo: SujianTopBarInfo,
    val showOuterTopBar: Boolean,
    val snackbarHostState: SnackbarHostState,
)

/** 一级导航选择 — 当前目的地 + 切换回调，打包传递避免函数参数超出门禁阈值。 */
internal data class SujianTopLevelSelection(
    val current: SujianDestination,
    val onSelected: (SujianDestination) -> Unit,
)

/**
 * #628 验收点 6：根据 [AndroidLayoutSpec.useBottomNavigation] 选择 compact 或 wide scaffold。
 * 提取以降低 [SujianNavigationSuite] 行数 — 顶栏归属由 showOuterTopBar 统一判定。
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun SujianNavScaffoldContent(
    modifier: Modifier,
    layoutSpec: AndroidLayoutSpec,
    chrome: SujianChromeSpec,
    scaffoldChrome: SujianNavScaffoldChrome,
    selection: SujianTopLevelSelection,
    navDisplayContent: @Composable () -> Unit,
) {
    if (layoutSpec.useBottomNavigation) {
        SujianCompactNavScaffold(
            modifier = modifier,
            topBarInfo = scaffoldChrome.topBarInfo,
            showTopBar = scaffoldChrome.showOuterTopBar,
            snackbarHostState = scaffoldChrome.snackbarHostState,
            // #597 正文一：进入正文后隐藏底栏；设置页从顶栏进入，也不再显示底栏。
            bottomBar =
                if (chrome.showPrimaryNavigation) {
                    {
                        SujianCompactBottomBar(selection.current, selection.onSelected)
                    }
                } else {
                    {}
                },
            navDisplayContent = navDisplayContent,
        )
    } else {
        // #628 验收点 6：大屏 Writing 顶栏归属已由 showOuterTopBar 统一判定。
        // SujianWideNavScaffold 复用同一份 topBarInfo + showOuterTopBar，不在此处再算一次。
        SujianWideNavScaffold(
            modifier = modifier,
            topBarInfo = scaffoldChrome.topBarInfo,
            showTopBar = scaffoldChrome.showOuterTopBar,
            snackbarHostState = scaffoldChrome.snackbarHostState,
            // #597 正文一：宽窗口同一套规则 — Settings/Editor 不创建 NavigationRail。
            rail =
                if (chrome.showPrimaryNavigation) {
                    {
                        SujianWideRail(selection.current, selection.onSelected)
                    }
                } else {
                    null
                },
            navDisplayContent = navDisplayContent,
        )
    }
}

private fun Modifier.navItemModifier(destination: SujianDestination): Modifier {
    val semanticTag =
        when (destination) {
            SujianDestination.Works -> SujianSemanticIds.NavigationWorks
            SujianDestination.StarMap -> SujianSemanticIds.NavigationStarMap
            SujianDestination.Stats -> SujianSemanticIds.NavigationStats
        }
    // 枚举覆盖全部分支，semanticTag 恒非空；testTag 需要非空 tag。
    return this.testTag(semanticTag)
}
