package com.xiwei.sujian.feature.editor.ui

import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import com.xiwei.sujian.R
import com.xiwei.sujian.core.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.core.designsystem.theme.LocalSujianDimensions
import com.xiwei.sujian.feature.editor.presentation.SaveStatus

/**
 * #635 评论 5384780619：写作区保存状态/字数 overlay — 只做 overlay，
 * 不改变编辑器 bounds，不参与正文高度计算。
 *
 * 保留 [SujianSemanticIds.EditorSaveStatus] testTag + stateDescription 语义
 * （ComposeWait.waitForSaveStatus 依赖）。
 *
 * #639 评论 5419182722：改成单行 Row（"已保存 · 123 字"），不再竖着占两行。
 * testTag/stateDescription 挂在 Row 上，保存状态为空时不渲染对应 Text，
 * 不保留空 Text 占高度 — overlay 高度不会因状态切换上下变化。
 */
@Composable
internal fun WritingStatusOverlay(
    saveStatus: SaveStatus,
    wordCount: Int,
    modifier: Modifier = Modifier,
) {
    val dims = LocalSujianDimensions.current
    val statusSemanticValue = saveStatus.semanticValue()
    val statusText = saveStatus.displayText()
    // #639 评论 5420317382：水平/垂直 padding 提到 Row 自己身上一次，子 Text
    // 不再各自带 horizontal space16 padding，避免状态、点号、字数之间各夹额外 16dp
    // 空隙，视觉上紧凑呈现 "已保存 · 123 字"。空状态仍不生成空 Text。
    Row(
        modifier =
            modifier
                .padding(horizontal = dims.space16, vertical = dims.space4)
                .testTag(SujianSemanticIds.EditorSaveStatus)
                .semantics { this.stateDescription = statusSemanticValue },
    ) {
        if (statusText.isNotEmpty()) {
            Text(
                text = statusText,
                style = MaterialTheme.typography.bodySmall,
            )
        }
        if (statusText.isNotEmpty() && wordCount > 0) {
            Text(
                text = " · ",
                style = MaterialTheme.typography.bodySmall,
            )
        }
        if (wordCount > 0) {
            Text(
                stringResource(R.string.word_count_format, wordCount),
                style = MaterialTheme.typography.bodySmall,
            )
        }
    }
}

private fun SaveStatus.semanticValue(): String =
    when (this) {
        SaveStatus.Idle -> "idle"
        SaveStatus.Unsaved -> "unsaved"
        SaveStatus.Saving -> "saving"
        SaveStatus.Saved -> "saved"
        SaveStatus.SaveFailed -> "failed"
    }

@Composable
private fun SaveStatus.displayText(): String =
    when (this) {
        SaveStatus.Idle -> ""
        SaveStatus.Unsaved -> stringResource(id = R.string.status_unsaved)
        SaveStatus.Saving -> stringResource(id = R.string.status_saving)
        SaveStatus.Saved -> stringResource(id = R.string.status_saved)
        SaveStatus.SaveFailed -> stringResource(id = R.string.status_save_failed)
    }
