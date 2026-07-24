package com.xiwei.sujian.ui.compose.theme

import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toArgb
import com.xiwei.sujian.editor.v2.host.SujianEditorView

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
        return remember(colorScheme) {
            val textColor = colorScheme.onSurface
            val cursorColor = colorScheme.primary
            val backgroundColor = colorScheme.surfaceContainerLowest
            val borderColor = colorScheme.outline
            val helperTextColor = colorScheme.onSurfaceVariant
            val selectedTextColor = colorScheme.onPrimaryContainer
            val preeditTextColor = colorScheme.onSurface

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

    fun applyToView(view: SujianEditorView, colors: EditorThemeColors) {
        view.applyThemeColorsFromAdapter(colors)
    }
}

@Composable
fun BindEditorThemeColors(editorView: SujianEditorView?) {
    if (editorView == null) return
    val colors = EditorThemeAdapter.extractColors()
    DisposableEffect(colors, editorView) {
        EditorThemeAdapter.applyToView(editorView, colors)
        onDispose { }
    }
}
