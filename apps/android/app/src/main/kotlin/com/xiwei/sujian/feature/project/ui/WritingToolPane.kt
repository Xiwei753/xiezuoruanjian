package com.xiwei.sujian.feature.project.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.R

/**
 * 写作工作台右侧工具面板（#625 第二段）— 大屏展开态占位。
 *
 * 未来放星图、AI 工具等。当前为占位 Composable，等具体功能实现再填充。
 *
 * #625 评论：用户主动收起 — 不按设备尺寸/方向自动多档收 pane。
 * 收起状态由 [WideWritingWorkspace] 用 rememberSaveable 持有。
 */
@Composable
internal fun WritingToolPane(modifier: Modifier = Modifier) {
    Column(
        modifier = modifier.fillMaxSize().padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(
            text = stringResource(id = R.string.writing_tool_pane_title),
            style = MaterialTheme.typography.titleMedium,
        )
        Text(
            text = stringResource(id = R.string.writing_tool_pane_placeholder),
            style = MaterialTheme.typography.bodyMedium,
        )
    }
}
