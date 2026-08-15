package com.xiwei.sujian.feature.project.ui

import androidx.compose.foundation.layout.Row
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import com.xiwei.sujian.R
import com.xiwei.sujian.app.presentation.screen.SujianChromeAction
import com.xiwei.sujian.app.presentation.screen.SujianChromeSpec
import com.xiwei.sujian.core.designsystem.component.SujianIconButton
import com.xiwei.sujian.core.designsystem.component.SujianTopAppBar
import com.xiwei.sujian.core.designsystem.icon.SujianIcons
import com.xiwei.sujian.core.designsystem.testing.SujianSemanticIds

/**
 * 写作工作台顶部工具栏回调 — 打包传递，避免函数参数超出门禁阈值。
 */
internal data class WritingToolbarCallbacks(
    val onBack: () -> Unit,
    val onSync: () -> Unit,
    val onSearch: () -> Unit,
    val onSettings: () -> Unit,
)

/**
 * 写作工作台顶部工具栏（#625 第二段）— 三组：
 * - 左：返回 + 撤销 + 重做；
 * - 中：正文工具占位（章节标题）；
 * - 右：同步 + 搜索 + 设置（复用 [SujianChromeSpec.actions]）。
 *
 * 复用 [SujianTopAppBar] 与 [SujianChromeSpec] — 不创建第二套顶栏组件。
 * 撤销/重做当前为占位（onClick 空动作），等编辑器撤销栈接入后填充。
 *
 * @param chapterTitle 当前章节标题（中间区域显示）
 * @param chrome 顶栏 chrome 决策（含 showBack 与 actions 顺序）
 * @param callbacks 顶栏回调（返回/同步/搜索/设置）
 */
@Composable
internal fun WritingWorkspaceToolbar(
    chapterTitle: String,
    chrome: SujianChromeSpec,
    callbacks: WritingToolbarCallbacks,
    modifier: Modifier = Modifier,
) {
    SujianTopAppBar(
        title = chapterTitle,
        modifier = modifier,
        navigationIcon = if (chrome.showBack) SujianIcons.ArrowBack else null,
        onNavigationClick = if (chrome.showBack) callbacks.onBack else null,
        actions = {
            Row {
                // 撤销/重做占位 — 等编辑器撤销栈接入后填充。
                SujianIconButton(
                    onClick = { },
                    icon = SujianIcons.Undo,
                    contentDescription = stringResource(id = R.string.action_undo),
                )
                SujianIconButton(
                    onClick = { },
                    icon = SujianIcons.Redo,
                    contentDescription = stringResource(id = R.string.action_redo),
                )
                // 右侧操作复用 chrome.actions 顺序（同步/搜索/设置）。
                chrome.actions.forEach { action ->
                    when (action) {
                        SujianChromeAction.Sync ->
                            SujianIconButton(
                                onClick = callbacks.onSync,
                                icon = SujianIcons.CloudSync,
                                contentDescription = stringResource(id = R.string.cd_sync_manual),
                                semanticId = SujianSemanticIds.NavigationSync,
                            )
                        SujianChromeAction.Search ->
                            SujianIconButton(
                                onClick = callbacks.onSearch,
                                icon = SujianIcons.Search,
                                contentDescription = stringResource(id = R.string.cd_search_dev),
                                semanticId = SujianSemanticIds.NavigationSearch,
                            )
                        SujianChromeAction.Settings ->
                            SujianIconButton(
                                onClick = callbacks.onSettings,
                                icon = SujianIcons.Settings,
                                contentDescription = stringResource(id = R.string.action_settings),
                                semanticId = SujianSemanticIds.NavigationSettings,
                            )
                    }
                }
            }
        },
        containerColor = Color.Transparent,
    )
}
