package com.xiwei.sujian.feature.editor.projection

import androidx.compose.runtime.Immutable

/**
 * #595 九：非活动章节预览的纯静态状态 — 不含动画引擎、Bitmap 或 VisualRuntime。
 *
 * #641：活动编辑由 [WritingEditorSurface] 的 state-based [BasicTextField] 持有
 * 输入/排版，动画由 [ComposeEditorVisualState] 消费 [TextLayoutResult] 做显示。
 * 非活动预览只需不可变的预览数据和静态 layout。
 */
@Immutable
data class ChapterPreviewState(
    val text: String,
    val revision: Long,
    val selection: TextRange? = null,
    val searchHighlights: List<TextRange> = emptyList(),
    val style: PreviewStyle = PreviewStyle(),
)

@Immutable
data class TextRange(
    val start: Int,
    val end: Int,
)

@Immutable
data class PreviewStyle(
    val fontSizeSp: Float = 16f,
    val lineSpacingMultiplier: Float = 1.5f,
)
