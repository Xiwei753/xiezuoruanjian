package com.xiwei.sujian.feature.editor.visual

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
 *   只保存当前应绘制的 scene、layout 两项原子值。
 *
 * Issue #725 评论 5750735497：停止自绘屏幕 caret 后，draw snapshot 不再携带
 * `restingCursorRect` — 屏幕 caret 始终由 BasicTextField 自己画，
 * draw 层只负责裁切动画接管区域和重画吞字/吐字 glyph。
 *
 * @param scene 当前视觉场景 — overlay 读取绘制。
 * @param layout 当前 layout 快照 — 供 buildHiddenPath 使用。
 */
internal data class ComposeEditorDrawSnapshot(
    val scene: ComposeVisualScene = ComposeVisualScene.Empty,
    val layout: ComposeLayoutSnapshot? = null,
)
