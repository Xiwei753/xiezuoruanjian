package com.xiwei.sujian.feature.editor.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import com.xiwei.sujian.R
import com.xiwei.sujian.core.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.core.designsystem.theme.LocalSujianDimensions
import com.xiwei.sujian.feature.editor.presentation.EditorUiState
import com.xiwei.sujian.feature.editor.presentation.SaveStatus

/**
 * 正文编辑窗格布局 — 纯展示层（#624 评论17 第2部分）。
 *
 * 从 [WritingPaneRoute] 拆出：只接纯 UiState/参数/slot/callback，
 * 不接 [com.xiwei.sujian.feature.editor.presentation.EditorViewModel]/
 * [com.xiwei.sujian.feature.editor.window.EditorWindowHost]/targetId，
 * 不自己 collect session/window flow。
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
    chapterTitle: String,
    uiState: EditorUiState,
    showEditor: Boolean,
    editorContent: @Composable (Modifier) -> Unit,
) {
    val dims = LocalSujianDimensions.current
    Column(modifier = modifier.fillMaxSize()) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = dims.space16, vertical = dims.space4),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                chapterTitle,
                style = MaterialTheme.typography.titleSmall,
                modifier = Modifier.weight(1f),
            )
            val statusSemanticValue =
                when (uiState.saveStatus) {
                    SaveStatus.Idle -> "idle"
                    SaveStatus.Unsaved -> "unsaved"
                    SaveStatus.Saving -> "saving"
                    SaveStatus.Saved -> "saved"
                    SaveStatus.SaveFailed -> "failed"
                }
            val statusText =
                when (uiState.saveStatus) {
                    SaveStatus.Idle -> ""
                    SaveStatus.Unsaved -> stringResource(id = R.string.status_unsaved)
                    SaveStatus.Saving -> stringResource(id = R.string.status_saving)
                    SaveStatus.Saved -> stringResource(id = R.string.status_saved)
                    SaveStatus.SaveFailed -> stringResource(id = R.string.status_save_failed)
                }
            Text(
                text = statusText,
                style = MaterialTheme.typography.bodySmall,
                modifier =
                    Modifier
                        .testTag(SujianSemanticIds.EditorSaveStatus)
                        .semantics { this.stateDescription = statusSemanticValue },
            )
        }

        if (uiState.wordCount > 0) {
            Text(
                stringResource(R.string.word_count_format, uiState.wordCount),
                style = MaterialTheme.typography.bodySmall,
                modifier = Modifier.padding(horizontal = dims.space16, vertical = dims.space2),
            )
        }

        // #595 一：只有 ViewModel 当前已提交章节才显示编辑器 —
        // 切换事务提交后、导航落地前，旧 pane 不显示 View（View 不在组合中，
        // 已安装的输入回调随 onRelease 清除），旧章节最后一次输入不可能写进新章节。
        if (!showEditor) {
            Box(modifier = Modifier.weight(1f).fillMaxWidth(), contentAlignment = Alignment.Center) {
                CircularProgressIndicator()
            }
        } else {
            editorContent(Modifier.weight(1f).fillMaxWidth())
        }
    }
}
