package com.xiwei.sujian.feature.editor.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.input.OutputTransformation
import androidx.compose.foundation.text.input.TextFieldLineLimits
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextStyle
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.xiwei.sujian.feature.editor.input.EditorTextFieldStateBridge
import com.xiwei.sujian.feature.editor.projection.TextRange
import com.xiwei.sujian.feature.editor.session.WindowBindingState
import com.xiwei.sujian.feature.editor.visual.ComposeEditorVisualState
import com.xiwei.sujian.feature.editor.visual.ComposeTextAnimationOverlay

/**
 * #641 评论1 第3节：活动/非活动 target 渲染模式。
 */
enum class EditorSurfaceMode {
    /** 当前窗口绑定该 target 且状态为 Attaching/Attached/Committing/Cancelling → 真实编辑器。 */
    EditorHost,

    /** 非活动章节 → 只读预览（ReadonlyChapterPreview）。 */
    Preview,
}

/**
 * #641 评论1 第3节：正文 Surface 渲染决策 — 纯函数。
 *
 * 活动 target 画真实编辑器（EditorSurfaceMode.EditorHost），
 * 非活动 target 显示只读预览（EditorSurfaceMode.Preview）。
 */
fun editorSurfaceMode(
    bindingState: WindowBindingState,
    windowId: String,
    targetId: String,
    isActivePane: Boolean,
): EditorSurfaceMode {
    val editorMatch =
        when (bindingState) {
            is WindowBindingState.Attaching ->
                bindingState.windowId == windowId && bindingState.targetId == targetId
            is WindowBindingState.Attached ->
                bindingState.windowId == windowId && bindingState.targetId == targetId
            is WindowBindingState.Committing -> bindingState.targetId == targetId
            is WindowBindingState.Cancelling -> bindingState.targetId == targetId
            WindowBindingState.Idle,
            is WindowBindingState.Detaching,
            is WindowBindingState.Detached,
            -> false
        }
    return when {
        editorMatch -> EditorSurfaceMode.EditorHost
        isActivePane -> EditorSurfaceMode.EditorHost
        else -> EditorSurfaceMode.Preview
    }
}

/**
 * #641 评论1 第3节：正文 Surface — 唯一一个 state-based [BasicTextField]。
 *
 * Android Foundation [BasicTextField] 负责实时输入、composition、selection、
 * 光标语义、软换行、命中测试和滚动；Rust Core 继续负责文档事务与持久化；
 * 素笺自己的文字/光标动画只消费系统最终 [TextLayoutResult] 做显示，
 * 不再拥有或修改编辑器几何。
 *
 * 注意：**不要把整个 BasicTextField 正文设成透明。** 正常文字、selection、composition
 * 都继续由系统正常画。只有当前正在动画的 UTF-16 range 通过 [OutputTransformation]
 * 临时变透明，overlay 只补画这些 range；动画完成立刻从 `hiddenRanges` 删除，
 * 系统正文已经在最终位置，不会再跳一次。
 *
 * #641 评论 问题4b：[inputEnabled] 是 [EditorViewModel.inputFrozen] 之外的第二层门控 —
 * BasicTextField 的 readOnly = !inputEnabled，章节切换冻结期间禁止 IME 写入 TextFieldState。
 */
@Composable
@Suppress("LongParameterList")
fun WritingEditorSurface(
    bridge: EditorTextFieldStateBridge,
    visualState: ComposeEditorVisualState,
    textStyle: TextStyle,
    textColor: Color,
    cursorColor: Color,
    inputEnabled: Boolean,
    /**
     * #641 评论 问题6e：搜索高亮 UTF-16 ranges —
     * 由 [WritingPaneEditorContent] 从 target decorations 的 UTF-8 ranges 转换而来。
     * 当前先添加参数和 OutputTransformation 框架，实际高亮 ranges 暂传空列表。
     */
    searchHighlights: List<TextRange> = emptyList(),
    /** 搜索高亮背景色，默认用 [androidx.compose.material3.MaterialTheme] 的 secondaryContainer。 */
    searchHighlightColor: Color =
        androidx.compose.material3.MaterialTheme.colorScheme.secondaryContainer,
    modifier: Modifier = Modifier,
) {
    val scrollState = rememberScrollState()
    val hiddenRanges by visualState.hiddenRanges.collectAsStateWithLifecycle()
    val drawsVisualCursor by visualState.drawsVisualCursor.collectAsStateWithLifecycle()

    // #641 评论 问题6e：搜索高亮接回 OutputTransformation —
    // 先画搜索高亮（背景色），再画动画隐藏 range（透明）。
    // 搜索高亮 ranges 已是 UTF-16（由 WritingPaneEditorContent 转换）。
    val outputTransformation =
        remember(hiddenRanges, searchHighlights, searchHighlightColor) {
            OutputTransformation {
                // 搜索高亮 — 背景色标记匹配区间。
                searchHighlights.forEach { range ->
                    if (range.start < range.end && range.end <= length) {
                        addStyle(
                            SpanStyle(background = searchHighlightColor),
                            range.start,
                            range.end,
                        )
                    }
                }
                // 动画隐藏 range — 透明（overlay 只补画这些 range）。
                hiddenRanges.forEach { range ->
                    if (range.start < range.end && range.end <= length) {
                        addStyle(
                            SpanStyle(color = Color.Transparent),
                            range.start,
                            range.end,
                        )
                    }
                }
            }
        }

    Box(modifier = modifier) {
        BasicTextField(
            state = bridge.state,
            modifier =
                Modifier
                    .fillMaxSize()
                    .testTag(com.xiwei.sujian.core.designsystem.testing.SujianSemanticIds.EditorContent),
            readOnly = !inputEnabled,
            lineLimits = TextFieldLineLimits.MultiLine(),
            scrollState = scrollState,
            textStyle = textStyle.copy(color = textColor),
            outputTransformation = outputTransformation,
            cursorBrush =
                if (drawsVisualCursor) {
                    SolidColor(Color.Transparent)
                } else {
                    SolidColor(cursorColor)
                },
            onTextLayout = { getResult ->
                getResult()?.let { result ->
                    visualState.onAuthoritativeLayout(
                        result = result,
                        selection = bridge.state.selection,
                        scrollY = scrollState.value,
                    )
                }
            },
        )

        ComposeTextAnimationOverlay(
            visualState = visualState,
            scrollY = scrollState.value,
            textColor = textColor,
            cursorColor = cursorColor,
            modifier = Modifier.fillMaxSize(),
        )
    }
}

/**
 * #624 评论16 问题3：confirmEditorAttached 的决策 — 只有 [WindowBindingState.Attached]
 * 且 windowId + targetId 都匹配才返回 true。
 *
 * - Attached 且匹配 → true（真正编辑器已绑定，解除输入冻结）；
 * - Attaching → false（等待推进到 Attached，不解除冻结）；
 * - Idle/Detached → false（beginEdit 发起绑定，不解除冻结）；
 * - Attached 但 windowId/targetId 不匹配 → false（残留自其他窗口的绑定）。
 */
fun shouldConfirmEditorAttached(
    bindingState: WindowBindingState,
    windowId: String,
    targetId: String,
): Boolean =
    bindingState is WindowBindingState.Attached &&
        bindingState.windowId == windowId &&
        bindingState.targetId == targetId
