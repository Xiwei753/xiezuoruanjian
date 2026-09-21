package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot

/**
 * #708 评论 5723410606：draw 阶段原子快照 —
 * 只把 draw 层每帧需要的状态打包成不可变快照，由 [ComposeEditorVisualState.drawSnapshot]
 * 在 drawWithContent 里一次性取走。
 *
 * 关键约束（评论 5723410606 第一节）：
 * - 这个 State **只能在 `drawWithContent` 里读**。
 * - 动画每帧更新时，Compose 只重跑 Draw，不准重新执行 BasicTextField 的 Composition/Layout。
 * - 不再保存"上一整屏"（[ComposeLocalFrameBarrier] 已删除）；
 *   只保存当前应绘制的 scene、layout、caretRect 三项原子值。
 *
 * Issue #728 评论 5754045689：重新由 [ComposeEditMotion] 统一画 caret —
 * [caretRect] 非 null 时 draw 层画统一 motion 的 caret（系统 caret 已透明）；
 * null 时不画 caret（无 active motion，如首帧/动画完成）。
 *
 * @param scene 当前视觉场景 — overlay 读取绘制。
 * @param layout 当前 layout 快照 — 供 buildHiddenPath 使用。
 * @param caretRect 当前帧的 caret rect — 由 [ComposeEditMotion.Sample.caretRect] 产生。
 *   null 表示无 active motion，draw 层不画 caret。
 */
internal data class ComposeEditorDrawSnapshot(
    val scene: ComposeVisualScene = ComposeVisualScene.Empty,
    val layout: ComposeLayoutSnapshot? = null,
    val caretRect: Rect? = null,
)
