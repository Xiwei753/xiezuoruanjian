package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot

/**
 * Issue #737：draw 阶段原子快照 —
 * 只把 draw 层每帧需要的状态打包成不可变快照，由 [ComposeEditorVisualState.drawSnapshot]
 * 在 drawWithContent 里一次性取走。
 *
 * 关键约束（继承自 #708 评论 5723410606 第一节）：
 * - 这个 State **只能在 `drawWithContent` 里读**。
 * - 动画每帧更新时，Compose 只重跑 Draw，不准重新执行 BasicTextField 的 Composition/Layout。
 * - 不再保存"上一整屏"；只保存当前应绘制的 motionSample、layout、restingCaretRect 三项原子值。
 *
 * Issue #737 重写：删除旧 [ComposeVisualScene] + caretRect 两份分离状态，
 * 改成单一 [motionSample]（[CoordinatedEditMotion.Sample]）+ [restingCaretRect]。
 * 一笔编辑只有一个 motion — 不再分别维护"文字动画是否 active"和"光标动画是否 active"。
 *
 * Issue #737 评论 5781084709 修复点 4：[motionSample] 只暴露 current-layout hidden ranges
 * （[CoordinatedEditMotion.Sample.hiddenRanges] 只含 Inserted 角色的 newLayout ranges）。
 * Deleted ghost 自己携带 old layout（通过 [CoordinatedEditMotion.Sample.glyphOverlays]），
 * draw 层用 overlay 自带的 layout 画 ghost，不裁 BasicTextField 当前正文。
 *
 * @param layout 当前 layout 快照 — 供 buildHiddenPath 使用。
 * @param motionSample 当前帧的 motion 采样结果 — 由 [CoordinatedEditMotion.sample] 产生。
 *   非 null 且 [CoordinatedEditMotion.Sample.isValid] 时 draw 层画 animated caret + glyph overlay；
 *   null 或无效时 draw 层画平台最终正文 + [restingCaretRect]。
 * @param restingCaretRect 静止 caret rect — 无 active motion 时 draw 层画这个 caret。
 *   null 表示无 active motion 且无已知 caret 位置，draw 层不画 caret。
 */
internal data class ComposeEditorDrawSnapshot(
    val layout: ComposeLayoutSnapshot? = null,
    val motionSample: CoordinatedEditMotion.Sample? = null,
    val restingCaretRect: Rect? = null,
)
