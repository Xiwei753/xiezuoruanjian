package com.xiwei.sujian.core.designsystem.component

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.window.DialogProperties
import com.xiwei.sujian.core.designsystem.testing.SujianSemanticIds

@Composable
fun SujianDialog(
    onDismissRequest: () -> Unit,
    title: String,
    confirmText: String,
    onConfirm: () -> Unit,
    modifier: Modifier = Modifier,
    dismissText: String? = null,
    onDismiss: (() -> Unit)? = null,
    icon: ImageVector? = null,
    body: @Composable (() -> Unit)? = null,
    dangerous: Boolean = false,
) {
    AlertDialog(
        onDismissRequest = onDismissRequest,
        modifier = modifier,
        properties = DialogProperties(usePlatformDefaultWidth = true),
        icon = if (icon != null) {
            { Icon(imageVector = icon, contentDescription = null) }
        } else null,
        title = { Text(text = title) },
        text = if (body != null) {
            { body() }
        } else null,
        confirmButton = {
            if (dangerous) {
                SujianDangerButton(text = confirmText, onClick = onConfirm, modifier = Modifier.testTag(SujianSemanticIds.DialogConfirm))
            } else {
                TextButton(onClick = onConfirm, modifier = Modifier.testTag(SujianSemanticIds.DialogConfirm)) { Text(confirmText) }
            }
        },
        dismissButton = if (dismissText != null) {
            {
                TextButton(
                    onClick = { onDismiss?.invoke() ?: onDismissRequest() },
                    modifier = Modifier.testTag(SujianSemanticIds.DialogCancel),
                ) {
                    Text(dismissText)
                }
            }
        } else null,
        containerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
        titleContentColor = MaterialTheme.colorScheme.onSurface,
        textContentColor = MaterialTheme.colorScheme.onSurfaceVariant,
    )
}
