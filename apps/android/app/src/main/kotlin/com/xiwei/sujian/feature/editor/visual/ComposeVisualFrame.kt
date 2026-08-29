package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot

/**
 * #641 评论 5457777142 问题2 + 评论 5458283021 问题1b：视觉帧 —
 * overlay 当前已经画到哪里的纯显示数据。
 *
 * 新事务到来时先用旧 transaction + 当前 progress 物化本帧，
 * 再把它作为新事务的 [ComposeVisualTransaction.startFrame]，
 * 避免 `progress=0` 生硬重开或直接覆盖旧事务。
 *
 * 这是纯显示数据：range、当前 translate、当前 alpha、当前 cursor rect、
 * 以及每个 slice 来自哪份 layout（old/new）。
 * 不持有 [androidx.compose.ui.text.TextLayoutResult] 引用，不写正文业务状态。
 *
 * #641 评论 5458283021 问题1b：frame 必须能保存下一笔真正需要继续画的来源布局和 slice。
 * 每个 slice 至少要知道它来自哪份 layout、range、当前 translate、当前 alpha。
 * overlay 新 transaction 的第一帧先按这些数据画，再插值到新 transaction 的目标几何。
 * 不要把 frame 再降维成一个平均 progress。
 *
 * #641 评论 5458880786 问题1b：新增 [sourceOldLayout] / [sourceNewLayout] —
 * 物化时冻结上一事务的 oldLayout/newLayout。新事务的 onAuthoritativeLayout 会替换
 * active transaction 的 oldLayout/newLayout，若 startFrame 仍指向 transaction.oldLayout/newLayout，
 * slice 绘制时会从当前（新）事务 layout 取字，画面错乱。drawStartFrameLayer 只读
 * [sourceOldLayout] / [sourceNewLayout]，不用当前 transaction 的 oldLayout/newLayout。
 * 默认 null 保持向后兼容。
 */
data class ComposeVisualFrame(
    val sourceOldLayout: ComposeLayoutSnapshot? = null,
    val sourceNewLayout: ComposeLayoutSnapshot? = null,
    val slices: List<VisualFrameSlice>,
    val cursorRect: Rect? = null,
    val cursorAlpha: Float = 1f,
)

/**
 * #641 评论 5457777142 问题2 + 评论 5458283021 问题1b：单个视觉帧 slice —
 * 一段受影响文字在某一时刻的显示状态。
 *
 * @param range 该 slice 对应的 UTF-16 range（old 或 new，由 [layoutSource] 决定）。
 * @param layoutSource 该 slice 来自哪份 layout — [FrameSliceSource.OldLayout] 取
 *   [ComposeVisualTransaction.oldLayout]，[FrameSliceSource.NewLayout] 取
 *   [ComposeVisualTransaction.newLayout]。overlay 据此从 transaction 取真实
 *   [androidx.compose.ui.text.TextLayoutResult] 画第一帧，不降维成平均 alpha。
 * @param translate 当前 translate 偏移（用于 retained move 位移插值）。
 * @param alpha 当前 alpha（用于淡入/淡出插值）。
 */
data class VisualFrameSlice(
    val range: TextRange,
    val layoutSource: FrameSliceSource,
    val translate: Offset,
    val alpha: Float,
)

/**
 * #641 评论 5458283021 问题1b：视觉帧 slice 的来源布局标识。
 *
 * - [OldLayout]：该 slice 来自 [ComposeVisualTransaction.oldLayout]（previous layout），
 *   overlay 画第一帧时从 transaction.oldLayout 取 [androidx.compose.ui.text.TextLayoutResult]。
 * - [NewLayout]：该 slice 来自 [ComposeVisualTransaction.newLayout]（current layout），
 *   overlay 画第一帧时从 transaction.newLayout 取 [androidx.compose.ui.text.TextLayoutResult]。
 */
enum class FrameSliceSource { OldLayout, NewLayout }
