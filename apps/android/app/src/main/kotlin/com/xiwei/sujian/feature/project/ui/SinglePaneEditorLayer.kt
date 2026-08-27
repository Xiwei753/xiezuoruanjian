package com.xiwei.sujian.feature.project.ui

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import com.xiwei.sujian.feature.editor.ui.SujianEditorHost
import com.xiwei.sujian.feature.editor.ui.theme.editorSurfaceBackgroundColor

/**
 * 窄屏稳定编辑器层 — Issue A.1。
 *
 * 窄屏最终结构必须是稳定 Box：prepared target 非空的 SinglePaneEditorLayer 永远在同一 slot；
 * 非 Editor 时 ProjectList/ChapterTree 在上层可见，隐藏 Editor View 仍 layout 但不可绘制/不可触摸；
 * 不用 alpha/GONE/AnimatedVisibility。
 *
 * 与 SujianEditorHost 不同：本层由 [preparedEditorTarget] 驱动，始终在组合中（只要 target 非空），
 * 通过 [presentationVisible] 控制 View.INVISIBLE/VISIBLE，不参与 session 业务判断。
 * 预热阶段（location 非 Editor）View 已 layout 但不可见，切换到 Editor 时零首帧跳动。
 *
 * #640 A.1：新建稳定 layer；PreparedEditorTarget 需可被新文件使用（internal 合理放置）。
 * #640 A.4：预热阶段（presentationVisible=false）背景透明，不画 opaque editor surface；
 * 可见阶段用共享 editorSurfaceBackgroundColor，不用 MaterialTheme colorScheme background。
 */
@Composable
internal fun SinglePaneEditorLayer(
    target: PreparedEditorTarget,
    presentationVisible: Boolean,
    modifier: Modifier = Modifier,
    onChapterSwitchFailed: (
        (oldProjectId: String, oldVolumeId: String?, oldChapterId: String?, oldChapterTitle: String) -> Unit
    )? = null,
) {
    val currentOnFailed by rememberUpdatedState(onChapterSwitchFailed)
    val background =
        if (presentationVisible) {
            editorSurfaceBackgroundColor()
        } else {
            Color.Transparent
        }

    Surface(
        color = background,
        modifier = modifier.fillMaxSize(),
    ) {
        SujianEditorHost(
            projectId = target.projectId,
            volumeId = target.volumeId,
            chapterId = target.chapterId,
            chapterTitle = target.chapterTitle,
            modifier = Modifier.fillMaxSize(),
            onChapterSwitchFailed = currentOnFailed,
            presentationVisible = presentationVisible,
        )
    }
}

/**
 * 窄屏预准备的编辑器 target — Issue A.1 / A.8。
 *
 * 在 requestOpenChapter 成功后、awaitPresentationReady 之前设置，
 * 让 SinglePaneEditorLayer 提前进入组合并 layout，但 View.INVISIBLE。
 * 导航到 Editor 后 presentationVisible=true，View 立即可见，无首帧跳动。
 * 离开 Editor（location 非 Editor）时清空，释放编辑器层。
 *
 * #640 A：[projectTitle] 由 suite 层 await+navigate 消费（selectProject 需要）；
 * target 自带全部 navigate 所需字段，handoff 后不再回查 ProjectWorkspaceScreen。
 */
data class PreparedEditorTarget(
    val projectId: String,
    val projectTitle: String,
    val volumeId: String,
    val chapterId: String,
    val chapterTitle: String,
) {
    val targetId: String
        get() = "chapter-body:$projectId:$volumeId:$chapterId"
}
