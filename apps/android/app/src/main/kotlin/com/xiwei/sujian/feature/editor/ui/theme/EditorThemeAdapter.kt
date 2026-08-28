package com.xiwei.sujian.feature.editor.ui.theme

import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toArgb

data class EditorThemeColors(
    val text: Int,
    val cursor: Int,
    val selection: Int,
    val selectedText: Int,
    val composing: Int,
    val background: Int,
    val border: Int,
    val helperText: Int,
    val preeditText: Int,
    val searchHighlight: Int,
)

object EditorThemeAdapter {
    @Composable
    fun extractColors(): EditorThemeColors {
        val colorScheme = MaterialTheme.colorScheme
        val textColor = colorScheme.onSurface
        val cursorColor = colorScheme.primary
        val borderColor = colorScheme.outline
        val helperTextColor = colorScheme.onSurfaceVariant
        val selectedTextColor = colorScheme.onPrimaryContainer
        val preeditTextColor = colorScheme.onSurface
        val backgroundColor: Color = editorSurfaceBackgroundColor()

        return remember(colorScheme) {
            EditorThemeColors(
                text = textColor.toArgb(),
                cursor = cursorColor.toArgb(),
                selection = (cursorColor.copy(alpha = 0.24f)).toArgb(),
                selectedText = selectedTextColor.toArgb(),
                composing = (cursorColor.copy(alpha = 0.70f)).toArgb(),
                background = backgroundColor.toArgb(),
                border = borderColor.toArgb(),
                helperText = helperTextColor.toArgb(),
                preeditText = preeditTextColor.toArgb(),
                searchHighlight = (colorScheme.tertiary.copy(alpha = 0.25f)).toArgb(),
            )
        }
    }
}
