package com.xiwei.sujian.feature.project.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.R
import com.xiwei.sujian.app.presentation.screen.SujianChromeAction
import com.xiwei.sujian.app.presentation.screen.SujianChromeSpec
import com.xiwei.sujian.core.designsystem.component.SujianIconButton
import com.xiwei.sujian.core.designsystem.icon.SujianIcons
import com.xiwei.sujian.core.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.feature.sync.data.model.SyncIndicatorState

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
 * 写作工作台顶部工具栏动作（#625 评论项2/3）— 打包撤销/重做与章节栏收起，
 * 避免函数参数超出门禁阈值。
 */
internal data class WritingToolbarActions(
    val onUndo: () -> Unit,
    val onRedo: () -> Unit,
    val onToggleChapterTree: () -> Unit,
    val chapterTreeCollapsed: Boolean,
)

/**
 * 写作工作台顶部工具栏（#625 评论项2）— 专门的大屏工作台工具栏，
 * 不再强套只有 title + trailing actions 的普通 [SujianTopAppBar]。
 *
 * 结构固定为三组独立容器：
 *
 * ```text
 * [返回 撤销 重做 章节栏收起/展开] | [正文工具区域 content slot] | [同步 搜索 设置]
 * ```
 *
 * - 左组：返回 + 撤销 + 重做 + 章节栏收起/展开（用户主动收起，不按设备尺寸自动收）；
 * - 中组：[content] 正文工具区域 slot，当前无真实功能时保持空 slot，不放"点了没反应"的假按钮；
 * - 右组：同步 + 搜索 + 设置，复用 [SujianChromeSpec.actions] 顺序。
 *
 * 右侧同步图标显示真实 [syncState]（CloudOff/CloudSync/CloudDone/CloudError），
 * 不再永远画固定 CloudSync。
 *
 * 撤销/重做接收 [actions.onUndo]/[actions.onRedo] 回调，由 [WideWritingWorkspace] 从
 * [com.xiwei.sujian.feature.editor.ui.LocalEditorWindowHost] 这条现有窗口链接入
 * （#625 评论项3），继续走 View → Pipeline → session 编辑链。
 *
 * @param chrome 顶栏 chrome 决策（含 showBack 与 actions 顺序）
 * @param syncState 同步状态（决定右侧同步图标）
 * @param callbacks 顶栏回调（返回/同步/搜索/设置）
 * @param actions 顶栏动作（撤销/重做/章节栏收起）
 * @param content 正文工具区域 content slot（中组），默认空
 */
@Composable
internal fun WritingWorkspaceToolbar(
    chrome: SujianChromeSpec,
    syncState: SyncIndicatorState,
    callbacks: WritingToolbarCallbacks,
    actions: WritingToolbarActions,
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit = {},
) {
    val containerColor =
        if (chrome.appBarTransparent) {
            Color.Transparent
        } else {
            MaterialTheme.colorScheme.surface
        }
    Surface(
        modifier = modifier.fillMaxWidth(),
        color = containerColor,
    ) {
        Row(
            modifier = Modifier.fillMaxWidth().height(64.dp).padding(horizontal = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            // 左组：返回 + 撤销 + 重做 + 章节栏收起/展开
            WritingToolbarLeadingGroup(
                showBack = chrome.showBack,
                onBack = callbacks.onBack,
                onUndo = actions.onUndo,
                onRedo = actions.onRedo,
                chapterTreeCollapsed = actions.chapterTreeCollapsed,
                onToggleChapterTree = actions.onToggleChapterTree,
            )
            // 中组：正文工具区域 content slot（空时不放伪按钮）
            Box(
                modifier = Modifier.weight(1f).fillMaxWidth(),
                contentAlignment = Alignment.Center,
            ) {
                content()
            }
            // 右组：同步 + 搜索 + 设置（复用 chrome.actions 顺序）
            WritingToolbarTrailingGroup(
                actions = chrome.actions,
                syncState = syncState,
                onSync = callbacks.onSync,
                onSearch = callbacks.onSearch,
                onSettings = callbacks.onSettings,
            )
        }
    }
}

/** 左组 — 返回 + 撤销 + 重做 + 章节栏收起/展开。 */
@Composable
private fun WritingToolbarLeadingGroup(
    showBack: Boolean,
    onBack: () -> Unit,
    onUndo: () -> Unit,
    onRedo: () -> Unit,
    chapterTreeCollapsed: Boolean,
    onToggleChapterTree: () -> Unit,
) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        if (showBack) {
            SujianIconButton(
                onClick = onBack,
                icon = SujianIcons.ArrowBack,
                contentDescription = stringResource(id = R.string.cd_back),
            )
        }
        SujianIconButton(
            onClick = onUndo,
            icon = SujianIcons.Undo,
            contentDescription = stringResource(id = R.string.action_undo),
        )
        SujianIconButton(
            onClick = onRedo,
            icon = SujianIcons.Redo,
            contentDescription = stringResource(id = R.string.action_redo),
        )
        // 章节栏收起/展开 — 用户主动收起，不按设备尺寸自动收。
        SujianIconButton(
            onClick = onToggleChapterTree,
            icon = if (chapterTreeCollapsed) SujianIcons.ExpandMore else SujianIcons.ExpandLess,
            contentDescription =
                stringResource(
                    id =
                        if (chapterTreeCollapsed) {
                            R.string.cd_chapter_tree_expand
                        } else {
                            R.string.cd_chapter_tree_collapse
                        },
                ),
        )
    }
}

/** 右组 — 同步 + 搜索 + 设置，按 [actions] 顺序。 */
@Composable
private fun WritingToolbarTrailingGroup(
    actions: List<SujianChromeAction>,
    syncState: SyncIndicatorState,
    onSync: () -> Unit,
    onSearch: () -> Unit,
    onSettings: () -> Unit,
) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        actions.forEach { action ->
            when (action) {
                SujianChromeAction.Sync ->
                    SujianIconButton(
                        onClick = onSync,
                        icon = syncIconFor(syncState),
                        contentDescription = stringResource(id = R.string.cd_sync_manual),
                        semanticId = SujianSemanticIds.NavigationSync,
                    )
                SujianChromeAction.Search ->
                    SujianIconButton(
                        onClick = onSearch,
                        icon = SujianIcons.Search,
                        contentDescription = stringResource(id = R.string.cd_search_dev),
                        semanticId = SujianSemanticIds.NavigationSearch,
                    )
                SujianChromeAction.Settings ->
                    SujianIconButton(
                        onClick = onSettings,
                        icon = SujianIcons.Settings,
                        contentDescription = stringResource(id = R.string.action_settings),
                        semanticId = SujianSemanticIds.NavigationSettings,
                    )
            }
        }
    }
}

/** 同步状态 → 图标（参考 SujianNavigationSuite 顶栏同步图标映射）。 */
private fun syncIconFor(syncState: SyncIndicatorState) =
    when (syncState) {
        SyncIndicatorState.Unconfigured -> SujianIcons.CloudOff
        SyncIndicatorState.Syncing -> SujianIcons.CloudSync
        SyncIndicatorState.Synced -> SujianIcons.CloudDone
        SyncIndicatorState.Failed -> SujianIcons.CloudError
    }
