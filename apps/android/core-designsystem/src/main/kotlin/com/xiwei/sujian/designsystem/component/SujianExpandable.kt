package com.xiwei.sujian.designsystem.component

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.expandVertically
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.designsystem.icon.SujianIcons
import com.xiwei.sujian.designsystem.theme.LocalSujianMotion

@Composable
fun SujianExpandableCard(
    title: String,
    expanded: Boolean,
    onExpandChange: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
    subtitle: String? = null,
    icon: ImageVector? = null,
    content: @Composable () -> Unit,
) {
    val motion = LocalSujianMotion.current

    SujianCard(modifier = modifier) {
        Column(modifier = Modifier.fillMaxWidth()) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .clickable { onExpandChange(!expanded) },
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (icon != null) {
                    Icon(
                        imageVector = icon,
                        contentDescription = null,
                        modifier = Modifier.size(24.dp),
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Spacer(modifier = Modifier.size(12.dp))
                }
                Column(modifier = Modifier.weight(1f)) {
                    Text(
                        text = title,
                        style = MaterialTheme.typography.titleMedium,
                        color = MaterialTheme.colorScheme.onSurface,
                    )
                    if (subtitle != null) {
                        Text(
                            text = subtitle,
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
                Icon(
                    imageVector = if (expanded) {
                        SujianIcons.KeyboardArrowUp
                    } else {
                        SujianIcons.KeyboardArrowDown
                    },
                    contentDescription = if (expanded) "收起" else "展开",
                    modifier = Modifier.size(24.dp),
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }

            AnimatedVisibility(
                visible = expanded,
                enter = expandVertically(animationSpec = androidx.compose.animation.core.tween(
                    durationMillis = motion.standardDurationMs,
                    easing = motion.standardEasing,
                )),
                exit = shrinkVertically(animationSpec = androidx.compose.animation.core.tween(
                    durationMillis = motion.quickDurationMs,
                    easing = motion.standardEasing,
                )),
            ) {
                Column(modifier = Modifier.fillMaxWidth()) {
                    Spacer(modifier = Modifier.height(8.dp))
                    content()
                }
            }
        }
    }
}

@Composable
fun SujianExpandableListItem(
    headline: String,
    expanded: Boolean,
    onExpandChange: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
    supportingText: String? = null,
    leadingIcon: ImageVector? = null,
    content: @Composable () -> Unit,
) {
    val motion = LocalSujianMotion.current

    Column(modifier = modifier.fillMaxWidth()) {
        SujianListItem(
            headline = headline,
            supportingText = supportingText,
            leadingIcon = leadingIcon,
            trailingContent = {
                Icon(
                    imageVector = if (expanded) {
                        SujianIcons.KeyboardArrowUp
                    } else {
                        SujianIcons.KeyboardArrowDown
                    },
                    contentDescription = if (expanded) "收起" else "展开",
                    modifier = Modifier.size(24.dp),
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            },
            onClick = { onExpandChange(!expanded) },
        )

        AnimatedVisibility(
            visible = expanded,
            enter = expandVertically(animationSpec = androidx.compose.animation.core.tween(
                durationMillis = motion.standardDurationMs,
                easing = motion.standardEasing,
            )),
            exit = shrinkVertically(animationSpec = androidx.compose.animation.core.tween(
                durationMillis = motion.quickDurationMs,
                easing = motion.standardEasing,
            )),
        ) {
            Column(modifier = Modifier.fillMaxWidth()) {
                content()
            }
        }
    }
}
