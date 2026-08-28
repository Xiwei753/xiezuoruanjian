package com.xiwei.sujian.feature.project.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import com.xiwei.sujian.R
import com.xiwei.sujian.core.designsystem.component.SujianIconButton
import com.xiwei.sujian.core.designsystem.component.SujianTopAppBar
import com.xiwei.sujian.core.designsystem.icon.SujianIcons
import com.xiwei.sujian.feature.editor.ui.SujianEditorHost

/**
 * #641 评论 问题6：窄屏（SinglePane）写作工作区 — 完整写作顶栏 + 唯一 SujianEditorHost。
 *
 * 之前 SinglePaneContent.Editor 分支只画 SujianEditorHost，没有完整写作顶栏（返回/搜索/同步/设置），
 * 窄屏正文 chrome 被拆掉。本组件恢复完整顶栏，中央编辑器仍走新的 BasicTextField 路线
 * （SujianEditorHost → WritingPane → BasicTextField），不恢复旧 EditorPresentationHost。
 *
 * 顶栏用 Material3 [SujianTopAppBar]：返回按钮 + 章节标题 + 搜索/同步/设置图标。
 * 同步/搜索/设置回调由调用方注入（窄屏暂用空实现占位，真实导航由上层 workspaceNavState 接管）。
 */
@Composable
@Suppress("LongParameterList") // #641 评论 问题6：写作顶栏回调 + 编辑器参数，函数级 suppress（既有先例）
internal fun CompactWritingWorkspace(
    projectId: String,
    volumeId: String,
    chapterId: String,
    chapterTitle: String,
    onBack: () -> Unit,
    onSearch: () -> Unit,
    onSync: () -> Unit,
    onSettings: () -> Unit,
    onChapterSwitchFailed: ((String, String?, String?, String) -> Unit)?,
    modifier: Modifier = Modifier,
) {
    Surface(
        color = MaterialTheme.colorScheme.surface,
        modifier = modifier,
    ) {
        Column(modifier = Modifier.fillMaxSize()) {
            CompactWritingTopBar(
                chapterTitle = chapterTitle,
                onBack = onBack,
                onSearch = onSearch,
                onSync = onSync,
                onSettings = onSettings,
            )
            SujianEditorHost(
                projectId = projectId,
                volumeId = volumeId,
                chapterId = chapterId,
                chapterTitle = chapterTitle,
                modifier = Modifier.fillMaxSize(),
                onChapterSwitchFailed = onChapterSwitchFailed,
            )
        }
    }
}

/**
 * 窄屏写作顶栏 — 返回 + 章节标题 + 搜索/同步/设置图标。
 * 提取以降低 [CompactWritingWorkspace] 认知复杂度。
 */
@Composable
private fun CompactWritingTopBar(
    chapterTitle: String,
    onBack: () -> Unit,
    onSearch: () -> Unit,
    onSync: () -> Unit,
    onSettings: () -> Unit,
) {
    SujianTopAppBar(
        title = chapterTitle,
        navigationIcon = SujianIcons.ArrowBack,
        onNavigationClick = onBack,
        actions = {
            SujianIconButton(
                onClick = onSync,
                icon = SujianIcons.CloudSync,
                contentDescription = stringResource(id = R.string.cd_sync_manual),
            )
            SujianIconButton(
                onClick = onSearch,
                icon = SujianIcons.Search,
                contentDescription = stringResource(id = R.string.cd_search_dev),
            )
            SujianIconButton(
                onClick = onSettings,
                icon = SujianIcons.Settings,
                contentDescription = stringResource(id = R.string.action_settings),
            )
        },
    )
}
