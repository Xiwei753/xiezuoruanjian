package com.xiwei.sujian.feature.editor.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.sp
import com.xiwei.sujian.feature.editor.projection.ChapterPreviewState

/**
 * #595 九：非活动章节预览 — 纯静态渲染，不使用动画 runtime。
 *
 * 只接受 [ChapterPreviewState]（纯文本 + Compose BasicText）。
 * 已删除 TargetDisplayRuntime 重载 — 预览不再携带第二套动画运行时。
 */
@Composable
fun ReadonlyChapterPreview(
    previewState: ChapterPreviewState,
    modifier: Modifier = Modifier,
) {
    Box(modifier = modifier.fillMaxSize()) {
        BasicText(
            text = previewState.text,
            style =
                TextStyle(
                    fontSize = previewState.style.fontSizeSp.sp,
                    lineHeight = (previewState.style.fontSizeSp * previewState.style.lineSpacingMultiplier).sp,
                ),
        )
    }
}
