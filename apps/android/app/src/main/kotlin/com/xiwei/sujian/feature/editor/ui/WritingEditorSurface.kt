package com.xiwei.sujian.feature.editor.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.input.InputTransformation
import androidx.compose.foundation.text.input.OutputTransformation
import androidx.compose.foundation.text.input.TextFieldLineLimits
import androidx.compose.foundation.text.input.forEachChange
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
import androidx.compose.ui.unit.isSpecified
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.xiwei.sujian.feature.editor.input.EditorTextFieldStateBridge
import com.xiwei.sujian.feature.editor.layout.EditorSoftBreakLayoutBinding
import com.xiwei.sujian.feature.editor.layout.EditorSoftBreakProjection
import com.xiwei.sujian.feature.editor.layout.EditorViewportState
import com.xiwei.sujian.feature.editor.projection.TextRange
import com.xiwei.sujian.feature.editor.session.WindowBindingState
import com.xiwei.sujian.feature.editor.visual.ComposeEditorVisualState
import com.xiwei.sujian.feature.editor.visual.EditorTextFieldDrawLayer
import com.xiwei.sujian.feature.editor.visual.LocalInputChange
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
    val drawsVisualCursor: Boolean,
    val cursorOwnedByVisual: Boolean,
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
    val drawsVisualCursor by visualState.drawsVisualCursor.collectAsStateWithLifecycle()
    // Issue #723 评论 5749023316 缺口1：当前是否真的由 visual timeline 持有 caret。
    // 只在 ownership 边沿更新（StateFlow distinctUntilChanged），不每帧驱动 Compose 重组。
    val cursorOwnedByVisual by visualState.cursorOwnedByVisual.collectAsStateWithLifecycle()

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
                drawsVisualCursor = drawsVisualCursor,
                cursorOwnedByVisual = cursorOwnedByVisual,
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
    val drawsVisualCursor = params.drawsVisualCursor
    val cursorOwnedByVisual = params.cursorOwnedByVisual
    val searchHighlights = params.searchHighlights
    val searchHighlightColor = params.searchHighlightColor
    val modifier = params.modifier
    val scope = rememberCoroutineScope()
    // Issue #717 评论 5743443030 修复1：OutputTransformation 与 onTextLayout 的同版本绑定 holder。
    // 普通 holder（非 Compose State），环形缓冲区记录最近若干次 transformation 的绑定。
    // Issue #717 评论 5743988019：layoutBinding 绑定到 bridge 生命周期 — bridge 随 target 切换而变化
    // （viewModel.bridgeForTarget 按 targetId 返回不同 bridge），切章节时重建，避免旧 bridge 被闭包保留导致跨 target 污染。
    val layoutBinding = remember(bridge) { EditorSoftBreakLayoutBinding() }

    // #644 评论 #684：OutputTransformation 整个编辑器生命周期只创建一次，
    // 动态值通过 rememberUpdatedState 读取，不再因 ranges 切换而重启输入会话。
    // #698 评论 5697612595：OutputTransformation 只保留 searchHighlights 部分，
    // 不再把动画 range 设 Color.Transparent — 动画字的遮罩改由 EditorTextFieldDrawLayer
    // 在 draw 层用 ClipOp.Difference 裁切完成，断开 hiddenRanges 回流回路。
    // Issue #723 评论 5748592923：空段落缩进进入显示布局本身 — OutputTransformation
    // 在启用首行缩进时对空段落插入零宽占位符，让 TextIndent 自己决定行首几何。
    val latestSearchHighlights = rememberUpdatedState(searchHighlights)
    val latestSearchHighlightColor = rememberUpdatedState(searchHighlightColor)
    // Issue #723 评论 5748592923：空段落缩进进入显示布局本身 — OutputTransformation
    // 在启用首行缩进时对空段落插入零宽占位符，让 TextIndent 自己决定行首几何。
    val latestAutoIndentEnabled =
        rememberUpdatedState(
            textStyle.textIndent?.firstLine?.let { it.isSpecified && it.value != 0f } == true,
        )

    // Issue #717 评论 5743988019：bridge 随 target 切换而变化（viewModel.bridgeForTarget 按 targetId 返回不同 bridge），
    // layoutBinding 已绑定到 bridge 生命周期，切章节时重建；OutputTransformation 跟随 layoutBinding 重建即可。
    val outputTransformation =
        remember(layoutBinding) {
            OutputTransformation {
                applyOutputTransformation(
                    searchHighlights = latestSearchHighlights.value,
                    searchHighlightColor = latestSearchHighlightColor.value,
                    layoutBinding = layoutBinding,
                    bridge = bridge,
                    autoIndentEnabled = latestAutoIndentEnabled.value,
                )
            }
        }

    // #694 评论第 2 步：给 BasicTextField 加稳定的 InputTransformation —
    // 只把输入事实塞进 visualState 的普通 tracker，不等 Core，不直接开始动画。
    // 它不是 Compose State，不在 InputTransformation 里改 StateFlow/mutableStateOf。
    val visualInputTransformation =
        remember(visualState, bridge) {
            InputTransformation {
                val changesSnapshot =
                    buildList {
                        changes.forEachChange { range, originalRange ->
                            add(LocalInputChange(newRange = range, oldRange = originalRange))
                        }
                    }
                if (changesSnapshot.isNotEmpty()) {
                    visualState.recordLocalInput(
                        oldText = originalText.toString(),
                        newText = asCharSequence().toString(),
                        oldSelection = originalSelection,
                        newSelection = selection,
                        changes = changesSnapshot,
                        // #706 评论 5718984286 修复：不在 InputTransformation 里读 bridge.state.composition —
                        // 这里拿到的是本次 InputTransformation 开始前的旧 TextFieldState，
                        // 判断不了本次新 composition。每次本地文字变更都先武装 barrier，
                        // 真正 composition 收口由 onAuthoritativeLayout 用本次真实 compositionActive 决定。
                    )
                }
            }
        }

    // #708 评论 5723410606 第三节：空段落缩进判定不再订阅 latestLayout —
    // Issue #723 评论 5748592923：空段落缩进进入显示布局本身（OutputTransformation +
    // projection 零宽占位符），不再需要 caret-only 特判。系统 caret 与自绘 caret
    // 消费同一份 transformed TextLayoutResult，不再把系统 caret 透明掉。

    // #698 评论 5698296237 / 5697612595 / 5699401353：统一 draw 层 —
    // EditorTextFieldDrawLayer 真正包住 BasicTextField（content lambda），
    // 用 drawWithContent + rememberGraphicsLayer 记录 BasicTextField 的完整绘制，
    // 再对 hiddenRanges 做 ClipOp.Difference 裁切后重画原正文，然后画动画字和视觉光标。
    // 不再通过 OutputTransformation 改变 BasicTextField 输出表示，断开 hiddenRanges 回流回路。
    // 不再用背景色盖正文 — 那会盖掉 selection/search highlight 且背景非纯 surface 时画错底色。
    EditorTextFieldDrawLayer(
        visualState = visualState,
        scrollY = viewportState.scrollState.value,
        textColor = textColor,
        cursorColor = cursorColor,
        // #684 评论 5663032418 断点3：直接读 live TextFieldState.selection，
        // 不再依赖 latestLayout.selection（只在 onTextLayout 时更新，纯 selection 变化会过期）。
        // TextFieldState.selection 本身是 Compose 可观察状态，selection 变化会驱动 recomposition。
        liveSelection = bridge.state.selection,
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
            inputTransformation = visualInputTransformation,
            // #644 评论 #684：smooth cursor 开启时系统光标一直透明，始终由 draw 层画。
            // smooth cursor 关闭时始终由系统画，draw 层永远不接管。
            // Issue #723 评论 5748592923：空段落缩进已进入显示布局本身，
            // 不再用 needsIndentedEmptyParagraphCaret 把系统 cursor 设透明。
            // Issue #723 评论 5749023316 缺口1：只有 smooth cursor 开启（drawsVisualCursor）
            // 且当前 visual 真正持有 caret（cursorOwnedByVisual）时才透明掉系统 caret；
            // 动画结束/纯点击/拖动时 cursorOwnedByVisual=false → 系统 caret 正常显示，
            // draw 层不自绘，手柄与光标同源。
            cursorBrush =
                if (drawsVisualCursor && cursorOwnedByVisual) {
                    SolidColor(Color.Transparent)
                } else {
                    SolidColor(cursorColor)
                },
            onTextLayout = { getResult ->
                getResult()?.let { result ->
                    onTextLayoutResult(
                        result = result,
                        viewportState = viewportState,
                        visualState = visualState,
                        bridge = bridge,
                        scope = scope,
                        onSurfaceReady = onSurfaceReady,
                        layoutBinding = layoutBinding,
                        autoIndentEnabled = latestAutoIndentEnabled.value,
                    )
                }
            },
        )
    }
}

/**
 * 处理 TextLayoutResult — 提取以降低认知复杂度。
 */
@Suppress("LongParameterList")
private fun onTextLayoutResult(
    result: TextLayoutResult,
    viewportState: EditorViewportState,
    visualState: ComposeEditorVisualState,
    bridge: EditorTextFieldStateBridge,
    scope: CoroutineScope,
    onSurfaceReady: () -> Boolean,
    layoutBinding: EditorSoftBreakLayoutBinding,
    autoIndentEnabled: Boolean,
) {
    // Issue #717 评论 5743443030 修复1 / 评论 5743745219：按 displayText 内容精确匹配同版本绑定，
    // 不再靠"长度猜版本"或"把 display 文本反解成 raw"。
    // 匹配成功时 rawText / projection / rawSelection / compositionActive 整体来自同一次 buffer 变化；
    // miss 时不把未知版本的 TextLayoutResult 和 live state 强行拼接。
    val displayText = result.layoutInput.text.text
    val match = layoutBinding.findForDisplayText(displayText)
    if (match != null) {
        // 匹配到同版本 Binding：rawText / projection / rawSelection / compositionActive 整体进入 viewport + visual pipeline。
        val restoreY = viewportState.onLayout(result, match.projection)
        if (restoreY != null) {
            scope.launch { viewportState.scrollState.scrollTo(restoreY) }
        }
        visualState.onAuthoritativeLayout(
            result = result,
            selection = match.rawSelection,
            scrollY = viewportState.scrollState.value,
            compositionActive = match.compositionActive,
            projection = match.projection,
            rawText = match.rawText,
        )
        onSurfaceReady()
        return
    }

    // Issue #717 评论 5743988019：binding miss（环形缓冲区淘汰或启动时序例外）。
    // 不把未知版本的 TextLayoutResult 和 live state 强行拼接。
    // 只有 displayText == liveRawText 且 liveProjection.insertPoints 为空（真正 identity，
    // 即按当前规则这份 raw 正文不应经过 OutputTransformation）才允许进入 viewport/visual；
    // insertPoints 非空说明按当前规则这份 raw 正文正常应该经过 OutputTransformation，
    // 既然 result 里没这些显示断点又没有 Binding 能证明来源，这份 layout 直接丢弃，等下一份匹配的 layout。
    // BasicTextField 自己仍正常显示。
    val liveRawText = bridge.state.text.toString()
    if (displayText == liveRawText) {
        val liveProjection = EditorSoftBreakProjection.fromRawText(liveRawText, autoIndentEnabled)
        if (liveProjection.insertPoints.isEmpty()) {
            val restoreY = viewportState.onLayout(result, liveProjection)
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
                projection = liveProjection,
                rawText = liveRawText,
            )
            onSurfaceReady()
        }
    }
    // displayText != liveRawText 或 liveProjection 非真正 identity：丢弃这份 layout，
    // 不进入 viewport/visual snapshot，不调用 onSurfaceReady，等下一份匹配的 onTextLayout。
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
 * Issue #717 评论 5741910919 / 评论 5742273757 修复1+5：OutputTransformation 内容 —
 * 西文软断行显示投影 + 搜索高亮 + 空段落零宽占位符。
 *
 * 1. 基于原始正文计算软断行投影，从后往前插入 U+200B（修复1：从后往前保证 raw offset 不失效）。
 * 2. 搜索高亮 range 是 raw 坐标，通过 projection.toDisplayRange 转换成 display 坐标后再 addStyle（修复5）。
 * 3. Issue #723 评论 5748592923：启用首行缩进时，对真正的空段落也插入 U+200B 占位符，
 *    让该空段落成为真实可排版的一行，TextIndent 自己决定行首几何。
 *    这个占位也纳入同一份 projection/offset 映射，不在 draw 层额外 +X。
 *
 * 不改变 TextFieldState 存储的正文（纯显示层）。
 */
@Suppress("CognitiveComplexMethod")
private fun androidx.compose.foundation.text.input.TextFieldBuffer.applyOutputTransformation(
    searchHighlights: List<TextRange>,
    searchHighlightColor: Color,
    layoutBinding: EditorSoftBreakLayoutBinding,
    bridge: EditorTextFieldStateBridge,
    autoIndentEnabled: Boolean,
) {
    val rawText = originalText.toString()
    val projection = EditorSoftBreakProjection.fromRawText(rawText, autoIndentEnabled)
    // 从后往前插入 U+200B，保证前面的 raw offset 不因 buffer 长度变化而失效。
    // TextFieldBuffer 没有 insert 方法，用 replace(offset, offset, text) 实现插入。
    for (insertPoint in projection.insertPoints.asReversed()) {
        if (insertPoint <= length) {
            replace(insertPoint, insertPoint, EditorSoftBreakProjection.ZERO_WIDTH_SPACE.toString())
        }
    }
    // 搜索高亮 range 是 raw 坐标，通过 projection.toDisplayRange 转换成 display 坐标后再 addStyle。
    searchHighlights.forEach { range ->
        val displayRange =
            projection.toDisplayRange(
                androidx.compose.ui.text.TextRange(range.start, range.end),
            )
        if (displayRange.start < displayRange.end && displayRange.end <= length) {
            addStyle(
                SpanStyle(background = searchHighlightColor),
                displayRange.start,
                displayRange.end,
            )
        }
    }
    // Issue #717 评论 5743443030 修复1 / 评论 5743745219：记录本次 transformation 的完整同版本输入快照。
    // 此时 buffer 内容（toString()）就是 transform 后的 displayText（含 U+200B）。
    // addStyle 不改变文本内容，所以 displayText 在 addStyle 前后一致。
    // originalSelection 是 TextFieldBuffer 公开属性（androidx.compose.ui.text.TextRange），
    // 即这次 buffer 变化前的原始 selection；bridge.state.composition 是当下 TextFieldState 的 composition。
    // 二者与 rawText / projection / displayText 严格来自同一次 buffer 变化。
    layoutBinding.record(
        rawText = rawText,
        projection = projection,
        displayText = toString(),
        rawSelection = originalSelection,
        compositionActive = bridge.state.composition != null,
    )
}
