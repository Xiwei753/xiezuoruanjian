package com.xiwei.sujian.core.designsystem.component

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.core.designsystem.theme.LocalSujianDimensions
import androidx.compose.material3.Icon
import androidx.compose.material3.IconToggleButton

@Composable
fun SujianPrimaryButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    icon: ImageVector? = null,
    loading: Boolean = false,
    enabled: Boolean = true,
) {
    Button(
        onClick = onClick,
        modifier = modifier,
        enabled = enabled && !loading,
        contentPadding = PaddingValues(horizontal = 24.dp, vertical = 12.dp),
        colors = ButtonDefaults.buttonColors(
            containerColor = MaterialTheme.colorScheme.primary,
            contentColor = MaterialTheme.colorScheme.onPrimary,
        ),
    ) {
        ButtonContent(text = text, icon = icon, loading = loading)
    }
}

@Composable
fun SujianTonalButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    icon: ImageVector? = null,
    loading: Boolean = false,
    enabled: Boolean = true,
) {
    FilledTonalButton(
        onClick = onClick,
        modifier = modifier,
        enabled = enabled && !loading,
        contentPadding = PaddingValues(horizontal = 24.dp, vertical = 12.dp),
    ) {
        ButtonContent(text = text, icon = icon, loading = loading)
    }
}

@Composable
fun SujianOutlinedButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    icon: ImageVector? = null,
    loading: Boolean = false,
    enabled: Boolean = true,
) {
    OutlinedButton(
        onClick = onClick,
        modifier = modifier,
        enabled = enabled && !loading,
        contentPadding = PaddingValues(horizontal = 24.dp, vertical = 12.dp),
    ) {
        ButtonContent(text = text, icon = icon, loading = loading)
    }
}

@Composable
fun SujianDangerButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    icon: ImageVector? = null,
    loading: Boolean = false,
    enabled: Boolean = true,
) {
    FilledTonalButton(
        onClick = onClick,
        modifier = modifier,
        enabled = enabled && !loading,
        contentPadding = PaddingValues(horizontal = 24.dp, vertical = 12.dp),
        colors = ButtonDefaults.filledTonalButtonColors(
            containerColor = MaterialTheme.colorScheme.errorContainer,
            contentColor = MaterialTheme.colorScheme.onErrorContainer,
        ),
    ) {
        ButtonContent(text = text, icon = icon, loading = loading)
    }
}

@Composable
fun SujianTextButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    icon: ImageVector? = null,
    loading: Boolean = false,
    enabled: Boolean = true,
    semanticId: String? = null,
) {
    TextButton(
        onClick = onClick,
        modifier = modifier.then(if (semanticId != null) Modifier.testTag(semanticId) else Modifier),
        enabled = enabled && !loading,
        contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
    ) {
        ButtonContent(text = text, icon = icon, loading = loading)
    }
}

@Composable
fun SujianIconButton(
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    icon: ImageVector? = null,
    contentDescription: String? = null,
    enabled: Boolean = true,
    tonal: Boolean = false,
    outlined: Boolean = false,
    semanticId: String? = null,
    iconTint: Color? = null,
) {
    val dimensions = LocalSujianDimensions.current
    val tagModifier = modifier.then(if (semanticId != null) Modifier.testTag(semanticId) else Modifier)
    val tint = iconTint ?: LocalContentColor.current
    when {
        outlined -> androidx.compose.material3.OutlinedIconButton(
            onClick = onClick,
            modifier = tagModifier.size(dimensions.minTouchTarget),
            enabled = enabled,
        ) {
            if (icon != null) {
                Icon(
                    imageVector = icon,
                    contentDescription = contentDescription,
                    modifier = Modifier.size(dimensions.iconSizeMedium),
                    tint = tint,
                )
            }
        }
        tonal -> androidx.compose.material3.FilledTonalIconButton(
            onClick = onClick,
            modifier = tagModifier.size(dimensions.minTouchTarget),
            enabled = enabled,
        ) {
            if (icon != null) {
                Icon(
                    imageVector = icon,
                    contentDescription = contentDescription,
                    modifier = Modifier.size(dimensions.iconSizeMedium),
                    tint = tint,
                )
            }
        }
        else -> androidx.compose.material3.IconButton(
            onClick = onClick,
            modifier = tagModifier.size(dimensions.minTouchTarget),
            enabled = enabled,
        ) {
            if (icon != null) {
                Icon(
                    imageVector = icon,
                    contentDescription = contentDescription,
                    modifier = Modifier.size(dimensions.iconSizeMedium),
                    tint = tint,
                )
            }
        }
    }
}

/**
 * #625 评论5301204285 问题2：图标切换按钮 — 包 Material3 [IconToggleButton]，
 * 用 M3 selected/checked 语义表达选中态，不在业务层手写颜色。
 * 尺寸约定同 [SujianIconButton]（minTouchTarget 外框 + iconSizeMedium 图标）。
 *
 * 不传 `colors`，让 M3 [IconToggleButtonDefaults] 处理 selected/unselected 配色；
 * 图标 tint 用 [LocalContentColor]，M3 会在 selected 时注入选中内容色。
 */
@Composable
fun SujianIconToggleButton(
    checked: Boolean,
    onCheckedChange: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
    icon: ImageVector? = null,
    contentDescription: String? = null,
    enabled: Boolean = true,
    semanticId: String? = null,
) {
    val dimensions = LocalSujianDimensions.current
    val tagModifier = modifier.then(if (semanticId != null) Modifier.testTag(semanticId) else Modifier)
    IconToggleButton(
        checked = checked,
        onCheckedChange = onCheckedChange,
        modifier = tagModifier.size(dimensions.minTouchTarget),
        enabled = enabled,
    ) {
        if (icon != null) {
            Icon(
                imageVector = icon,
                contentDescription = contentDescription,
                modifier = Modifier.size(dimensions.iconSizeMedium),
                tint = LocalContentColor.current,
            )
        }
    }
}

@Composable
fun SujianFab(
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    icon: ImageVector? = null,
    text: String? = null,
    extended: Boolean = false,
    contentDescription: String? = null,
) {
    if (extended && text != null) {
        androidx.compose.material3.ExtendedFloatingActionButton(
            onClick = onClick,
            modifier = modifier,
            icon = {
                if (icon != null) {
                    Icon(
                        imageVector = icon,
                        contentDescription = contentDescription,
                    )
                }
            },
            text = { Text(text) },
        )
    } else {
        androidx.compose.material3.FloatingActionButton(
            onClick = onClick,
            modifier = modifier,
        ) {
            if (icon != null) {
                Icon(
                    imageVector = icon,
                    contentDescription = contentDescription,
                )
            }
        }
    }
}

@Composable
private fun ButtonContent(
    text: String,
    icon: ImageVector?,
    loading: Boolean,
) {
    if (loading) {
        CircularProgressIndicator(
            modifier = Modifier.size(18.dp),
            strokeWidth = 2.dp,
            color = Color.Unspecified,
        )
    } else {
        if (icon != null) {
            Icon(
                imageVector = icon,
                contentDescription = null,
                modifier = Modifier.size(18.dp),
            )
            androidx.compose.foundation.layout.Spacer(modifier = Modifier.size(8.dp))
        }
        Text(text = text)
    }
}
