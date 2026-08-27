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
 * 编辑器 slot 始终组合；[showEditor] 只决定 loading overlay 是否显示。
 * [presentationVisible] 只决定 loading/status overlay 是否进入展示层，
 * 编辑器 View 自身的可见性由 WritingEditorSurface 管理。
 *
 * #640 B.11：删除正文自己的 imePadding — IME 只由 Scaffold 消费，
 * 根 Box 回到 fillMaxSize()，状态带跟随正文真实可用高度。
 */
@Composable
internal fun WritingPaneLayout(
    modifier: Modifier,
    uiState: EditorUiState,
    showEditor: Boolean,
    presentationVisible: Boolean = true,
    editorContent: @Composable (Modifier) -> Unit,
) {
    Box(modifier = modifier.fillMaxSize()) {
        // #640 B.11：只要 target 有效就必须始终组合 editorContent；
        // showEditor 只控制 loading overlay，不再作为 AndroidView 存在门控。
        // 编辑器可见性由 presentationVisible && isPresentationReady 控制 View.INVISIBLE/VISIBLE。
        editorContent(Modifier.fillMaxSize())

        if (presentationVisible && !showEditor) {
            Box(
                Modifier.fillMaxSize(),
                contentAlignment = Alignment.Center,
            ) {
                CircularProgressIndicator()
            }
        }

        // #635 评论 5384780619：保存状态/字数只做 overlay，
        // 不再插在正文上面参与 Column 高度计算。
        // #639 评论 5419182722：状态带固定在右下角（BottomEnd），
        // IME 消费由 Scaffold 统一处理（#640 B.11）。
        if (presentationVisible) {
            WritingStatusOverlay(
                saveStatus = uiState.saveStatus,
                wordCount = uiState.wordCount,
                modifier = Modifier.align(Alignment.BottomEnd),
            )
        }
    }
}
