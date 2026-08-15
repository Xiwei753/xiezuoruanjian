package com.xiwei.sujian.feature.project.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.R
import com.xiwei.sujian.core.designsystem.component.SujianIconButton
import com.xiwei.sujian.core.designsystem.component.SujianIconToggleButton
import com.xiwei.sujian.core.designsystem.icon.SujianIcons

/**
 * #625 评论项5：写作工作台工具项模型 — rail 负责"选哪个工具"，
 * pane 承载选中的 content slot。星图/AI 真正内容分别归 #373 / #506，
 * 这里只定义工具壳契约，不实现业务。
 */
internal data class WritingToolItem(
    val id: String,
    val icon: ImageVector,
    val labelResId: Int,
)

/**
 * 写作工作台最右图标列（#625 评论项5）— 工具壳。
 *
 * 宽度由调用方 [WideWritingWorkspace] 通过 modifier 传入（来自 Rust
 * `LayoutMetrics.toolRailWidthDp`，#628 验收点 4）。画工具图标列 + pane 收起/展开按钮。
 * - rail 负责"选哪个工具"（[onSelect]）；
 * - pane 收起/展开由 [onTogglePane] 控制，[paneCollapsed] 决定按钮图标方向；
 * - 星图/AI 真正内容归 #373 / #506，这里不实现业务；
 * - 当前没有可用工具内容时 [tools] 为空，rail 只显示收起/展开按钮，不放伪功能按钮。
 * - [selectedToolId] 映射成 M3 [androidx.compose.material3.IconToggleButton] 选中态
 *   （checked = tool.id == selectedToolId），选中配色由 M3 defaults 决定，不在业务层手写颜色。
 *
 * #625 评论：用户主动收起 — 不按设备尺寸/方向自动多档收 pane。
 * 收起状态由 [WideWritingWorkspace] 用 rememberSaveable 持有。
 */
@Composable
internal fun WritingToolRail(
    tools: List<WritingToolItem>,
    selectedToolId: String?,
    onSelect: (String) -> Unit,
    onTogglePane: () -> Unit,
    paneCollapsed: Boolean,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier = modifier.fillMaxHeight().padding(vertical = 8.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        tools.forEach { tool ->
            SujianIconToggleButton(
                checked = tool.id == selectedToolId,
                onCheckedChange = { onSelect(tool.id) },
                icon = tool.icon,
                contentDescription = stringResource(id = tool.labelResId),
            )
        }
        // pane 收起/展开按钮 — 用户主动收起，不按设备尺寸自动收。
        SujianIconButton(
            onClick = onTogglePane,
            icon = if (paneCollapsed) SujianIcons.ExpandMore else SujianIcons.ExpandLess,
            contentDescription =
                stringResource(
                    id =
                        if (paneCollapsed) {
                            R.string.cd_writing_tool_pane_expand
                        } else {
                            R.string.cd_writing_tool_pane_collapse
                        },
                ),
        )
    }
}
