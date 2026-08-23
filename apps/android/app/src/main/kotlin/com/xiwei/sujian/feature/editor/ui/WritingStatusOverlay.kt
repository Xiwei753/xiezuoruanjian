package com.xiwei.sujian.feature.editor.ui

import androidx.compose.foundation.layout.Column
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
 */
@Composable
internal fun WritingStatusOverlay(
    saveStatus: SaveStatus,
    wordCount: Int,
    modifier: Modifier = Modifier,
) {
    val dims = LocalSujianDimensions.current
    Column(modifier = modifier) {
        val statusSemanticValue =
            when (saveStatus) {
                SaveStatus.Idle -> "idle"
                SaveStatus.Unsaved -> "unsaved"
                SaveStatus.Saving -> "saving"
                SaveStatus.Saved -> "saved"
                SaveStatus.SaveFailed -> "failed"
            }
        val statusText =
            when (saveStatus) {
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
                    .padding(horizontal = dims.space16, vertical = dims.space4)
                    .testTag(SujianSemanticIds.EditorSaveStatus)
                    .semantics { this.stateDescription = statusSemanticValue },
        )

        if (wordCount > 0) {
            Text(
                stringResource(R.string.word_count_format, wordCount),
                style = MaterialTheme.typography.bodySmall,
                modifier = Modifier.padding(horizontal = dims.space16, vertical = dims.space2),
            )
        }
    }
}
