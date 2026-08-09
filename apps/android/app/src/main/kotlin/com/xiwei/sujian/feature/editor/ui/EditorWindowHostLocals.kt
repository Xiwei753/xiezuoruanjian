package com.xiwei.sujian.feature.editor.ui

import androidx.compose.runtime.compositionLocalOf
import com.xiwei.sujian.feature.editor.window.EditorWindowHost

/**
 * #595 一/十：CompositionLocal 提供 [EditorWindowHost]。
 *
 * AnimatedTextEditorSlot 根部覆盖层已删除；rememberEditorWindowHost 死入口已删除。
 * EditorWindowHost 由 Activity 级 CompositionLocalProvider 提供。
 */
val LocalEditorWindowHost =
    compositionLocalOf<EditorWindowHost?> {
        null
    }
