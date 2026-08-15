package com.xiwei.sujian.feature.project.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.R

/**
 * 写作工作台最右图标列（#625 第二段）— 中屏折叠态占位。
 *
 * 固定宽度 56.dp，未来放星图/AI 工具入口图标。当前为占位 Composable。
 *
 * #625 评论：用户主动收起 — 不按设备尺寸/方向自动多档收 pane。
 * 收起状态由 [WideWritingWorkspace] 用 rememberSaveable 持有。
 */
@Composable
internal fun WritingToolRail(modifier: Modifier = Modifier) {
    Column(
        modifier = modifier.fillMaxHeight().width(56.dp).padding(vertical = 8.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(
            text = stringResource(id = R.string.writing_tool_rail_placeholder),
            style = MaterialTheme.typography.labelSmall,
        )
    }
}
