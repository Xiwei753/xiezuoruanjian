package com.xiwei.sujian.feature.editor.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import com.xiwei.sujian.feature.editor.presentation.EditorUiState

/**
 * 正文编辑窗格布局 — 纯展示层（#624 评论17 第2部分）。
 *
 * #635 评论 5384780619：正文槽从第一帧起就占最终 fillMaxSize()，
 * 不再用 Column 先画章节标题/字数再被真实编辑器替换 — 避免正文可用高度
 * 在初始化期间换一次导致写作区首帧跳动。保存状态/字数改为 overlay，
 * 不参与正文高度计算。
 *
 * 编辑器表面由 [editorContent] slot 提供 — Route 层负责收集
 * targetDecorationsVersionFlow 后注入 WritingEditorSurface(coordinator, targetId, modifier)。
 *
 * #595 一：输入窗口防护 — 只有 ViewModel 当前已提交章节（showEditor=true）
 * 才显示编辑器；切换事务提交后、导航落地前，旧 pane 不显示 View、
 * 不安装输入回调，旧章节最后一次输入不可能写进新章节。
 */
@Composable
internal fun WritingPaneLayout(
    modifier: Modifier,
    uiState: EditorUiState,
    showEditor: Boolean,
    editorContent: @Composable (Modifier) -> Unit,
) {
    Box(modifier = modifier.fillMaxSize()) {
        if (showEditor) {
            editorContent(Modifier.fillMaxSize())
        } else {
            Box(
                Modifier.fillMaxSize(),
                contentAlignment = Alignment.Center,
            ) {
                CircularProgressIndicator()
            }
        }

        // #635 评论 5384780619：保存状态/字数只做 overlay，
        // 不再插在正文上面参与 Column 高度计算。
        WritingStatusOverlay(
            saveStatus = uiState.saveStatus,
            wordCount = uiState.wordCount,
            modifier = Modifier.align(Alignment.TopEnd),
        )
    }
}
