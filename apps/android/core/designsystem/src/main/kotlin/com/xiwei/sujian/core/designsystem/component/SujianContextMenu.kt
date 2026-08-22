package com.xiwei.sujian.core.designsystem.component

import androidx.compose.foundation.layout.Box
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import com.xiwei.sujian.core.designsystem.icon.SujianIcons

/**
 * #625/#632：锚定溢出菜单触发器 + Material3 [DropdownMenu] 容器。
 *
 * 触发按钮（`MoreVert`）与 [DropdownMenu] 同处一个紧尺寸 [Box] —
 * 两者共用同一布局坐标系，菜单锚定到按钮左下角，不再用页面级 Dialog 居中弹出，
 * 也不依赖外层 Row/卡片的位置。`modifier` 放在外层 [Box]，让 [Box] 的几何范围
 * 就是触发控件这一小块；调用方无需各自计算 [androidx.compose.ui.unit.DpOffset]。
 *
 * 本组件只负责触发与展开容器：
 * - [expanded] 由调用方（行/卡片）持有，避免页面级单例状态互相覆盖；
 * - 菜单项内容由 [content] 提供，通常是一组 [SujianOverflowMenuItem]。
 *
 * 不引入任何业务类型（Project/Volume/Chapter/WorkspaceActionSpec 等），
 * 保持 `:core:designsystem` 边界。
 */
@Composable
fun SujianOverflowMenu(
    expanded: Boolean,
    onExpandedChange: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
    contentDescription: String = "",
    content: @Composable () -> Unit,
) {
    Box(modifier = modifier) {
        IconButton(
            onClick = { onExpandedChange(!expanded) },
        ) {
            Icon(
                imageVector = SujianIcons.MoreVert,
                contentDescription = contentDescription,
            )
        }
        DropdownMenu(
            expanded = expanded,
            onDismissRequest = { onExpandedChange(false) },
        ) {
            content()
        }
    }
}

/**
 * #625：[SujianOverflowMenu] 内的标准菜单项 —
 * 仅渲染文字、[enabled] 与 [onClick]，不引入业务类型。
 *
 * 菜单项顺序由调用方按 Core 契约 order 提供，本组件不重排。
 */
@Composable
fun SujianOverflowMenuItem(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
) {
    DropdownMenuItem(
        text = { Text(text) },
        onClick = onClick,
        enabled = enabled,
        modifier = modifier,
    )
}
