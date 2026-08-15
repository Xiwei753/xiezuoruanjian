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
 * 写作工作台右侧工具面板（#625 评论项5）— 工具壳。
 *
 * pane 只负责承载选中的工具 content slot（[content]）：
 * - [content] 非空时画工具内容；
 * - [content] 为 null 时显示明确空态（i18n），不放伪功能按钮；
 * - 星图/AI 真正内容分别归 #373 / #506，以后只往 slot 填内容，不再改写作工作台布局。
 *
 * #625 评论：用户主动收起 — 不按设备尺寸/方向自动多档收 pane。
 * 收起状态由 [WideWritingWorkspace] 用 rememberSaveable 持有。
 *
 * @param content 当前选中工具的 content slot；null 表示无可用工具内容，显示空态。
 */
@Composable
internal fun WritingToolPane(
    content: (@Composable () -> Unit)?,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier = modifier.fillMaxSize().padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        if (content != null) {
            content()
        } else {
            Text(
                text = stringResource(id = R.string.writing_tool_pane_empty),
                style = MaterialTheme.typography.bodyMedium,
            )
        }
    }
}
