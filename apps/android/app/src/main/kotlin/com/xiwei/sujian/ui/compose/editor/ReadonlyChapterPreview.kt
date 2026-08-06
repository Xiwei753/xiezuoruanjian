package com.xiwei.sujian.ui.compose.editor

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.graphics.nativeCanvas
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.sp
import com.xiwei.sujian.editor.v2.projection.ChapterPreviewState
import com.xiwei.sujian.editor.v2.projection.TargetDisplayRuntime

/**
 * #595 九：非活动章节预览 — 纯静态渲染，不使用动画 runtime。
 *
 * 优先使用 [ChapterPreviewState]（纯文本 + Compose BasicText）；
 * 向后兼容 [TargetDisplayRuntime]（用于已有投影路径，但不接入 FrameClock）。
 */
@Composable
fun ReadonlyChapterPreview(
    previewState: ChapterPreviewState,
    modifier: Modifier = Modifier
) {
    Box(modifier = modifier.fillMaxSize()) {
        BasicText(
            text = previewState.text,
            style = TextStyle(
                fontSize = previewState.style.fontSizeSp.sp,
                lineHeight = (previewState.style.fontSizeSp * previewState.style.lineSpacingMultiplier).sp,
            ),
        )
    }
}

/**
 * 向后兼容重载 — 从 [TargetDisplayRuntime] 读取纯文本，不使用其动画 runtime。
 */
@Composable
fun ReadonlyChapterPreview(
    projection: TargetDisplayRuntime,
    modifier: Modifier = Modifier
) {
    val text = projection.getText()
    ReadonlyChapterPreview(
        previewState = ChapterPreviewState(
            text = text,
            revision = projection.getRevision(),
        ),
        modifier = modifier,
    )
}
