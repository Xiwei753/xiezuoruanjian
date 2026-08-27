package com.xiwei.sujian.feature.project.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fitInside
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.WindowInsetsRulers
import androidx.compose.ui.res.stringResource
import com.xiwei.sujian.R
import com.xiwei.sujian.app.presentation.screen.SujianChromeAction
import com.xiwei.sujian.core.designsystem.component.SujianIconButton
import com.xiwei.sujian.core.designsystem.icon.SujianIcons
import com.xiwei.sujian.core.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.feature.sync.data.model.SyncIndicatorState

/*
 * 写作工作台顶部工具栏三组容器（#625 评论项2 / #628 评论 5301021120 第 4 步）—
 * 按 Rust AndroidWorkbenchLayoutPlan 的 ToolbarLeading/Center/Trailing bounds 分别放置。
 *
 * #628 评论 5301021120 第 4 步：删除整体的 `WritingWorkspaceToolbar`（被 WideWritingWorkspace
 * 的自定义 Compose Layout 按 plan 放 slot 替代，不加 fallback/兼容旁路）。
 * 三组容器暴露为 internal，供 WideWritingWorkspace 直接按 bounds 放置。
 *
 * - 左组：返回 + 撤销 + 重做 + 章节栏收起/展开（用户主动收起，不按设备尺寸自动收）；
 * - 中组：WritingToolbarCenterSlot 正文工具区域 content slot，当前无真实功能时保持空 slot；
 * - 右组：同步 + 搜索 + 设置，复用 SujianChromeSpec.actions 顺序。
 */

/** 左组回调 — 打包传递，避免 [WritingToolbarLeadingGroup] 参数超出门禁阈值。 */
internal data class WritingToolbarLeadingCallbacks(
    val onBack: () -> Unit,
    val onUndo: () -> Unit,
    val onRedo: () -> Unit,
    val onToggleChapterTree: () -> Unit,
)

/** 左组 — 返回 + 撤销 + 重做 + 章节栏收起/展开。 */
@Composable
internal fun WritingToolbarLeadingGroup(
    showBack: Boolean,
    chapterTreeCollapsed: Boolean,
    callbacks: WritingToolbarLeadingCallbacks,
    modifier: Modifier = Modifier,
) {
    // #640 评论 5443102488：Surface 占 Rust toolbar slot 完整 rect（背景画到系统栏），
    // fitInside(SafeDrawing) 挂在内部 Row 上让内容避开 safe region（状态栏/刘海/IME），
    // 不缩掉 toolbar 背景，避免状态栏区域露出下面透明 Scaffold。
    val safeDrawingRulers = WindowInsetsRulers.SafeDrawing.current
    Surface(
        modifier = modifier,
        color = MaterialTheme.colorScheme.surface,
    ) {
        Row(
            modifier = Modifier.fitInside(safeDrawingRulers),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (showBack) {
                SujianIconButton(
                    onClick = callbacks.onBack,
                    icon = SujianIcons.ArrowBack,
                    contentDescription = stringResource(id = R.string.cd_back),
                )
            }
            SujianIconButton(
                onClick = callbacks.onUndo,
                icon = SujianIcons.Undo,
                contentDescription = stringResource(id = R.string.action_undo),
            )
            SujianIconButton(
                onClick = callbacks.onRedo,
                icon = SujianIcons.Redo,
                contentDescription = stringResource(id = R.string.action_redo),
            )
            // 章节栏收起/展开 — 用户主动收起，不按设备尺寸自动收。
            SujianIconButton(
                onClick = callbacks.onToggleChapterTree,
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
}

/** 中组 — 正文工具区域 content slot（空时不放伪按钮）。 */
@Composable
internal fun WritingToolbarCenterSlot(modifier: Modifier = Modifier) {
    // #640 评论 5443102488：Surface 占完整 rect，fitInside 挂在内部 Box 上。
    val safeDrawingRulers = WindowInsetsRulers.SafeDrawing.current
    Surface(
        modifier = modifier,
        color = Color.Transparent,
    ) {
        Box(
            modifier = Modifier.fillMaxSize().fitInside(safeDrawingRulers),
            contentAlignment = Alignment.Center,
        ) {
            // 当前无真实正文工具功能，保持空 slot（不放"点了没反应"的假按钮）。
        }
    }
}

/** 右组回调 — 打包传递，避免 [WritingToolbarTrailingGroup] 参数超出门禁阈值。 */
internal data class WritingToolbarTrailingCallbacks(
    val onSync: () -> Unit,
    val onSearch: () -> Unit,
    val onSettings: () -> Unit,
)

/** 右组 — 同步 + 搜索 + 设置，按 [actions] 顺序。 */
@Composable
internal fun WritingToolbarTrailingGroup(
    actions: List<SujianChromeAction>,
    syncState: SyncIndicatorState,
    callbacks: WritingToolbarTrailingCallbacks,
    modifier: Modifier = Modifier,
) {
    // #640 评论 5443102488：Surface 占 Rust toolbar slot 完整 rect，fitInside 挂在内部 Row 上。
    val safeDrawingRulers = WindowInsetsRulers.SafeDrawing.current
    Surface(
        modifier = modifier,
        color = MaterialTheme.colorScheme.surface,
    ) {
        Row(
            modifier = Modifier.fitInside(safeDrawingRulers),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            actions.forEach { action ->
                WritingToolbarTrailingAction(
                    action = action,
                    syncState = syncState,
                    callbacks = callbacks,
                )
            }
        }
    }
}

/** 右组单个动作 — 提取以降低 [WritingToolbarTrailingGroup] 圈复杂度。 */
@Composable
private fun WritingToolbarTrailingAction(
    action: SujianChromeAction,
    syncState: SyncIndicatorState,
    callbacks: WritingToolbarTrailingCallbacks,
) {
    when (action) {
        SujianChromeAction.Sync ->
            SujianIconButton(
                onClick = callbacks.onSync,
                icon = syncIconFor(syncState),
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

/** 同步状态 → 图标（参考 SujianNavigationSuite 顶栏同步图标映射）。 */
private fun syncIconFor(syncState: SyncIndicatorState) =
    when (syncState) {
        SyncIndicatorState.Unconfigured -> SujianIcons.CloudOff
        SyncIndicatorState.Syncing -> SujianIcons.CloudSync
        SyncIndicatorState.Synced -> SujianIcons.CloudDone
        SyncIndicatorState.Failed -> SujianIcons.CloudError
    }
