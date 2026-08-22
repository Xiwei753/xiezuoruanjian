package com.xiwei.sujian.core.designsystem.component

import androidx.compose.foundation.selection.toggleable
import androidx.compose.material3.Switch
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.platform.testTag

/**
 * 开关行 — 复用 [SujianListItem] 的行几何（最小行高、padding、文字基线），
 * 避免设置页开关和普通列表项出现两套行高。
 *
 * - headline = title
 * - supportingText = supportingText
 * - leadingIcon = icon
 * - trailingContent = Switch(checked, onCheckedChange = null, enabled)
 * - 外层点击/开关语义由整行 toggleable(role = Role.Switch) 保留
 * - semanticId 加在最外层 modifier
 */
@Composable
fun SujianSwitchRow(
    title: String,
    checked: Boolean,
    onCheckedChange: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
    supportingText: String? = null,
    icon: ImageVector? = null,
    enabled: Boolean = true,
    semanticId: String? = null,
) {
    SujianListItem(
        headline = title,
        modifier = modifier
            .then(if (semanticId != null) Modifier.testTag(semanticId) else Modifier)
            .toggleable(
                value = checked,
                enabled = enabled,
                role = Role.Switch,
                onValueChange = onCheckedChange,
            )
            .semantics {
                stateDescription = if (checked) "已开启" else "已关闭"
            },
        supportingText = supportingText,
        leadingIcon = icon,
        trailingContent = {
            Switch(
                checked = checked,
                onCheckedChange = null,
                enabled = enabled,
            )
        },
        enabled = enabled,
    )
}
