package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextRange

/**
 * #708 评论 5723410606 第二节：本地编辑首帧交接 —
 * 只保存"这一笔编辑哪些局部区域暂时由 overlay 接管"，**不保存上一整屏**。
 *
 * 取代了已删除的 [ComposeLocalFrameBarrier]（那个类保存 baseScene/baseLayout，
 * 配合 stableFrameLayer 把整个编辑器上一帧冻结，是"整行闪、全工作区闪、旧字残留"的来源）。
 *
 * 由 [ComposeEditorVisualState.recordLocalInput] 在 InputTransformation 阶段记录，
 * [ComposeEditorVisualState.onAuthoritativeLayout] 在 layout 阶段配对出 local patch 后消费。
 * 同一个 layout 阶段发布"局部 handoff scene"，drawWithContent 在后面的 Draw 阶段读它。
 *
 * @param patchId 配对出的 local patch id；null 表示尚未配对。
 * @param insertedRanges 本笔输入新增的 UTF-16 ranges — 首帧 scene 把这些范围加进 hiddenRanges。
 * @param deletedRanges 本笔输入删除的 UTF-16 ranges — 首帧 scene 为这些范围建 ghost。
 * @param originCursorRect 本笔编辑的明确 T0 caret rect — 首帧 scene.cursorRect 先放真实旧 caret。
 */
internal data class ComposeLocalEditHandoff(
    val patchId: Long? = null,
    val insertedRanges: List<TextRange> = emptyList(),
    val deletedRanges: List<TextRange> = emptyList(),
    val originCursorRect: Rect? = null,
)
