package com.xiwei.sujian.feature.editor.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import com.xiwei.sujian.feature.editor.presentation.EditorUiState

/**
 * #641 评论2 第6节：IME / inset — Compose 只消费一次，不再建立 `presentationReady`。
 *
 * [WritingPaneLayout] 正文区域只保留一个 inset owner：
 *
 * ```kotlin
 * Box(
 *     Modifier
 *         .fillMaxSize()
 *         .windowInsetsPadding(
 *             WindowInsets.safeDrawing.only(
 *                 WindowInsetsSides.Horizontal + WindowInsetsSides.Bottom,
 *             ),
 *         ),
 * ) {
 *     editorContent()
 * }
 * ```
 *
 * Android 官方定义 `safeDrawing` 本来就包含 IME；Compose 会随 IME 动画逐帧更新 inset。
 * 高度逐帧变化是正常布局，不应该被解释成"Editor 几何失效"。
 *
 * 所以从 [EditorWindowHost] 删除：
 * - [PresentationReadinessGate]
 * - [presentationReady]
 * - [presentationReadyGeneration]
 * - [awaitPresentationReady()]
 * - [registerPresentationReadyCallback()]
 * - [invalidatePresentationReady()]
 *
 * 也不再需要 [SujianEditorView.onSizeChanged()] 里的 [notifyPresentationGeometryInvalidated()] /
 * [dispatchPresentationReadyIfPossible()]。
 */
@Composable
internal fun WritingPaneLayout(
    modifier: Modifier,
    uiState: EditorUiState,
    showEditor: Boolean,
    editorContent: @Composable (Modifier) -> Unit,
) {
    Box(
        modifier =
            modifier
                .fillMaxSize()
                .windowInsetsPadding(
                    WindowInsets.safeDrawing.only(
                        androidx.compose.foundation.layout.WindowInsetsSides.Horizontal +
                            androidx.compose.foundation.layout.WindowInsetsSides.Bottom,
                    ),
                ),
    ) {
        // #640 B.11：只要 target 有效就必须始终组合 editorContent；
        // showEditor 只控制 loading overlay，不再作为 AndroidView 存在门控。
        // 编辑器可见性由 WritingEditorSurface 管理（OutputTransformation 隐藏动画 range）。
        editorContent(Modifier.fillMaxSize())

        if (!showEditor) {
            Box(
                Modifier.fillMaxSize(),
                contentAlignment = Alignment.Center,
            ) {
                CircularProgressIndicator()
            }
        }

        // #635 评论 5384780619：保存状态/字数只做 overlay，
        // 不再插在正文上面参与 Column 高度计算。
        // #639 评论 5419182722：状态带固定在右下角（BottomEnd），
        // IME 消费由 Scaffold 统一处理（#640 B.11）。
        WritingStatusOverlay(
            saveStatus = uiState.saveStatus,
            wordCount = uiState.wordCount,
            modifier = Modifier.align(Alignment.BottomEnd),
        )
    }
}
