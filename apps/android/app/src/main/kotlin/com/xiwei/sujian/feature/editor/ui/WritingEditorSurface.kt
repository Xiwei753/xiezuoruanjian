package com.xiwei.sujian.feature.editor.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.input.OutputTransformation
import androidx.compose.foundation.text.input.TextFieldLineLimits
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextStyle
import com.xiwei.sujian.feature.editor.input.EditorTextFieldStateBridge
import com.xiwei.sujian.feature.editor.layout.EditorViewportState
import com.xiwei.sujian.feature.editor.projection.TextRange
import com.xiwei.sujian.feature.editor.session.WindowBindingState
import com.xiwei.sujian.feature.editor.visual.ComposeEditorVisualState
import com.xiwei.sujian.feature.editor.visual.EditorTextFieldDrawLayer
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch

/** 正文编辑器内容参数 — 提取以降低 [WritingEditorContent] 参数列表长度。 */
data class WritingEditorContentParams(
    val bridge: EditorTextFieldStateBridge,
    val visualState: ComposeEditorVisualState,
    val viewportState: EditorViewportState,
    val textStyle: TextStyle,
    val textColor: Color,
    val cursorColor: Color,
    val inputEnabled: Boolean,
    val onSurfaceReady: () -> Boolean,
    val searchHighlights: List<TextRange>,
    val searchHighlightColor: Color,
    val modifier: Modifier,
)

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
 * Issue #737：保留平台自动换行 — BasicTextField 直接消费 [TextLayoutResult]，
 * 不引入软换行占位字符（U+200B）或正文投影（projection）。
 * 动画层（[EditorTextFieldDrawLayer] + [CoordinatedEditMotion]）只叠加本次编辑涉及的
 * glyph 和 caret，不复制整段正文布局。
 *
 * #698 评论 5698296237 / 5697612595 / 5699401353：编辑器绘制链根改 —
 * 不再通过 [OutputTransformation] 把动画 range 设 `Color.Transparent` 改变 BasicTextField 输出表示。
 * BasicTextField 始终画完整真实正文，[EditorTextFieldDrawLayer] 真正包住 BasicTextField
 * （content lambda），用 `drawWithContent` + `rememberGraphicsLayer` 记录 BasicTextField
 * 的完整绘制，再对 `hiddenRanges` 做 `ClipOp.Difference` 裁切后重画原正文
 * （只在绘制阶段排除动画接管区域），然后画动画字和视觉光标。
 * 这样 BasicTextField 的 onTextLayout 只因真实正文/几何变化触发，
 * 不再因 hiddenRanges 变化触发二次 layout，断开
 * "动画 hiddenRanges -> OutputTransformation 改正文显示 -> BasicTextField 再 layout -> VisualState 再消费 layout"
 * 回路。不再用背景色盖正文 — 那会盖掉 selection/search highlight 且背景非纯 surface 时画错底色。
 *
 * #708 评论 5723410606 第一节：删除整屏旧帧缓存（stableFrameLayer + ComposeLocalFrameBarrier）—
 * EditorTextFieldDrawLayer 不再记录上一稳定帧、不再有 local barrier 重放分支。
 * 每一帧都先画当前 BasicTextField，只对真正由动画接管的 range 做 Difference clip，
 * 再画局部动画层。OutputTransformation 继续只做搜索高亮。
 *
 * #641 评论 问题4b：[inputEnabled] 是 [EditorViewModel.inputFrozen] 之外的第二层门控 —
 * BasicTextField 的 readOnly = !inputEnabled，章节切换冻结期间禁止 IME 写入 TextFieldState。
 *
 * #644 评论 5462826712 第2节：viewportState 管滚动/视口，onSurfaceReady 完成 attach。
 */
@Composable
@Suppress("LongParameterList")
fun WritingEditorSurface(
    bridge: EditorTextFieldStateBridge,
    visualState: ComposeEditorVisualState,
    viewportState: EditorViewportState,
    textStyle: TextStyle,
    textColor: Color,
    cursorColor: Color,
    inputEnabled: Boolean,
    onSurfaceReady: () -> Boolean,
    searchHighlights: List<TextRange> = emptyList(),
    searchHighlightColor: Color =
        androidx.compose.material3.MaterialTheme.colorScheme.secondaryContainer,
    modifier: Modifier = Modifier,
) {
    WritingEditorContent(
        params =
            WritingEditorContentParams(
                bridge = bridge,
                visualState = visualState,
                viewportState = viewportState,
                textStyle = textStyle,
                textColor = textColor,
                cursorColor = cursorColor,
                inputEnabled = inputEnabled,
                onSurfaceReady = onSurfaceReady,
                searchHighlights = searchHighlights,
                searchHighlightColor = searchHighlightColor,
                modifier = modifier,
            ),
    )
}

/**
 * 正文编辑器内容 — 提取以降低 [WritingEditorSurface] 的认知复杂度。
 */
@Composable
@OptIn(ExperimentalFoundationApi::class)
private fun WritingEditorContent(params: WritingEditorContentParams) {
    val bridge = params.bridge
    val visualState = params.visualState
    val viewportState = params.viewportState
    val textStyle = params.textStyle
    val textColor = params.textColor
    val cursorColor = params.cursorColor
    val inputEnabled = params.inputEnabled
    val onSurfaceReady = params.onSurfaceReady
    val searchHighlights = params.searchHighlights
    val searchHighlightColor = params.searchHighlightColor
    val modifier = params.modifier
    val scope = rememberCoroutineScope()

    // #644 评论 #684：OutputTransformation 整个编辑器生命周期只创建一次，
    // 动态值通过 rememberUpdatedState 读取，不再因 ranges 切换而重启输入会话。
    // Issue #728 评论 5754045689：删除西文软断行 U+200B 插入 —
    // 不再用 EditorSoftBreakProjection / EditorSoftBreakLayoutBinding 做 raw↔display 映射，
    // BasicTextField 直接消费 TextLayoutResult，正文始终是纯文本。
    // OutputTransformation 只保留 searchHighlights 部分。
    val latestSearchHighlights = rememberUpdatedState(searchHighlights)
    val latestSearchHighlightColor = rememberUpdatedState(searchHighlightColor)

    val outputTransformation =
        remember(bridge) {
            OutputTransformation {
                applyOutputTransformation(
                    searchHighlights = latestSearchHighlights.value,
                    searchHighlightColor = latestSearchHighlightColor.value,
                )
            }
        }

    // #698 评论 5698296237 / 5697612595 / 5699401353：统一 draw 层 —
    // EditorTextFieldDrawLayer 真正包住 BasicTextField（content lambda），
    // 用 drawWithContent 对 hiddenRanges 做 ClipOp.Difference 裁切后画动画字和 caret。
    // Issue #728 评论 5754045689：系统 caret 透明（cursorBrush = Color.Transparent），
    // caret 由 EditorTextFieldDrawLayer 用统一 motion 的 caretRect 画。
    EditorTextFieldDrawLayer(
        visualState = visualState,
        scrollY = viewportState.scrollState.value,
        textColor = textColor,
        cursorColor = cursorColor,
        modifier = modifier.fillMaxSize(),
    ) {
        BasicTextField(
            state = bridge.state,
            modifier =
                Modifier
                    .fillMaxSize()
                    .testTag(com.xiwei.sujian.core.designsystem.testing.SujianSemanticIds.EditorContent),
            readOnly = !inputEnabled,
            lineLimits = TextFieldLineLimits.MultiLine(),
            scrollState = viewportState.scrollState,
            textStyle = textStyle.copy(color = textColor),
            outputTransformation = outputTransformation,
            // Issue #728 评论 5754045689：系统 caret 透明 —
            // caret 由 EditorTextFieldDrawLayer 用统一 motion 的 caretRect 画，
            // 不再由 BasicTextField 自己画。
            cursorBrush = SolidColor(Color.Transparent),
            onTextLayout = { getResult ->
                getResult()?.let { result ->
                    onTextLayoutResult(
                        result = result,
                        viewportState = viewportState,
                        visualState = visualState,
                        bridge = bridge,
                        scope = scope,
                        onSurfaceReady = onSurfaceReady,
                    )
                }
            },
        )
    }
}

/**
 * 处理 TextLayoutResult — 提取以降低认知复杂度。
 *
 * Issue #728 评论 5754045689：删除 display/raw 匹配分支 —
 * 不再用 EditorSoftBreakLayoutBinding 做 displayText↔rawText 版本匹配，
 * 直接消费 TextLayoutResult：text 就是 raw 正文（不再插 U+200B），
 * selection 直接来自 bridge.state.selection。
 */
@Suppress("LongParameterList")
private fun onTextLayoutResult(
    result: TextLayoutResult,
    viewportState: EditorViewportState,
    visualState: ComposeEditorVisualState,
    bridge: EditorTextFieldStateBridge,
    scope: CoroutineScope,
    onSurfaceReady: () -> Boolean,
) {
    val restoreY = viewportState.onLayout(result)
    if (restoreY != null) {
        scope.launch { viewportState.scrollState.scrollTo(restoreY) }
    }
    visualState.onAuthoritativeLayout(
        result = result,
        selection = bridge.state.selection,
        scrollY = viewportState.scrollState.value,
        // #694 评论第 2 步：composition 活跃时只推进布局基线，不播放 preedit 的吞吐；
        // composition 结束后的最终输入再配对 LocalInputVisualEdit 生成视觉 patch。
        compositionActive = bridge.state.composition != null,
    )
    onSurfaceReady()
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

/**
 * Issue #728 评论 5754045689：OutputTransformation 内容 —
 * 只保留搜索高亮，不再插入 U+200B 西文软断行占位符。
 *
 * 删除 EditorSoftBreakProjection / EditorSoftBreakLayoutBinding 后，
 * 正文始终是纯文本，BasicTextField 直接消费 TextLayoutResult，
 * 搜索高亮 range 直接是正文坐标，不需要 raw→display 映射。
 *
 * 不改变 TextFieldState 存储的正文（纯显示层）。
 */
private fun androidx.compose.foundation.text.input.TextFieldBuffer.applyOutputTransformation(
    searchHighlights: List<TextRange>,
    searchHighlightColor: Color,
) {
    // 搜索高亮 range 直接是正文坐标（不再经过 projection 转换）。
    searchHighlights.forEach { range ->
        val start = range.start
        val end = range.end
        if (start < end && end <= length) {
            addStyle(
                SpanStyle(background = searchHighlightColor),
                start,
                end,
            )
        }
    }
}
