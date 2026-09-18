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
 *   只保存当前应绘制的 scene、layout、restingCursorRect 三项原子值。
 *
 * @param scene 当前视觉场景 — overlay 读取绘制。
 * @param layout 当前 layout 快照 — 供 buildHiddenPath / computeRestingCursorRect 使用。
 * @param restingCursorRect 静止光标 rect — 无活动动画时的最终真实位置。
 */
internal data class ComposeEditorDrawSnapshot(
    val scene: ComposeVisualScene = ComposeVisualScene.Empty,
    val layout: ComposeLayoutSnapshot? = null,
    val restingCursorRect: Rect? = null,
)
