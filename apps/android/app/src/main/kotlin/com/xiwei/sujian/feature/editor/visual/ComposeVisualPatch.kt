package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy
import uniffi.writer_core.AnimationModeDto

/**
 * #689 评论 5674631257 步骤1：retained move —
 * 自动折行/手动换行时被挤到下一行的"保留文字"。
 *
 * [oldRange] 和 [newRange] 表示同一逻辑文本范围在旧布局和新布局中的位置。
 * 位置只从 [androidx.compose.ui.text.TextLayoutResult] 读，动画只负责画。
 *
 * #689：从 [ComposeVisualTransaction]（已删除）搬到这里。
 * 持续 timeline 下 retained move 只描述"几何位移"，不再携带事务身份。
 */
data class RetainedMove(
    val oldRange: TextRange,
    val newRange: TextRange,
)

/**
 * #689 评论 5674631257 步骤1：屏幕 diff —
 * "这一帧正文和上一帧相比发生了什么"。
 *
 * 这个文件只描述屏幕帧之间的差异，**不带动画进度**，也**不带上一笔动画状态**。
 * 真正长期存在的屏幕动画状态由 [ComposeVisualTimeline] 持有。
 *
 * 创建以后也不需要 `startFrame / suppressedCurrentRanges / textAnimationActive /
 * cursorAnimationActive / masterProgress` 这些事务状态 —
 * 持续 timeline 下每个文字单元自己保存时间。
 *
 * Issue #725 评论 5750735497：停止自绘屏幕 caret 后，patch 不再携带
 * `cursorMotionPath` / `originCursorRect` — 屏幕光标始终由 BasicTextField 自己画，
 * 文字吞吐动画改由纯文字时间线驱动。
 *
 * Issue #728 评论 5754045689：重新把 caret 几何放回 patch，但不恢复旧 affinity/wedge 逻辑。
 * 加 [originCaretRect] / [targetCaretRect]，由 [ComposeEditMotion] 统一驱动 caret 移动和文字吞吐。
 * patch 不保存自己的动画进度，进度统一交给 [ComposeEditMotion]。
 * Enter 本身没有 glyph，所以一笔 Enter patch 可以没有 inserted unit，但必须有 old/new caret rect。
 *
 * Issue #728 评论 5754839786：[originCaretRect] / [targetCaretRect] 改成必填参数（去掉 Rect.Zero 默认值）。
 * 旧默认值让 [ComposeEditorVisualState.buildLocalInputPatch] 漏传 caret rect 时静默拿到 Rect.Zero，
 * 本地 patch 的 caret 两端变成 (0,0,0,0)，[ComposeEditMotion] 从原点插值到原点，屏幕 caret 不动。
 * 现在构造时必须显式提供真实 caret rect，主源码所有构造点（[ComposeVisualFrameCoordinator.tryBuildPatch] /
 * [ComposeVisualPatchBatch.compose] / [ComposeEditorVisualState.buildLocalInputPatch]）都从
 * [ComposeLayoutSnapshot.cursorRect] 算出真实几何。
 *
 * @param id patch ID — 单调递增，overlay 据此判断是否需要推进 timeline。
 * @param coreTransactionIds Core 事务 ID 列表 — 一笔 patch 可能对应多笔 Core 事务。
 * @param oldLayout 旧布局快照 — 上一帧真正呈现过的 [ComposeLayoutSnapshot]。
 * @param newLayout 新布局快照 — 当前帧真正呈现出来的 [ComposeLayoutSnapshot]。
 * @param offsetMap 整条 chain 合成后的 T0→Tn offset map — timeline 用它把旧 unit 接到新 layout。
 *   null 表示 chain 中有笔没有 offset map（回退到不映射）。
 * @param insertedUnits 新插入的 UTF-16 ranges（Insert/Move 的 newRanges）—
 *   timeline 为每个 unit 新建 alpha 0→1 通道。
 * @param deletedUnits 被删除的 UTF-16 ranges（Delete/Move 的 oldRanges）—
 *   timeline 把对应 unit 转成 ghost，alpha 从当前值继续到 0。
 * @param retainedMoves 被挤到下一行的"保留文字"的 old/new range —
 *   只给真正发生 oldRect → newRect 位移的存活 unit 新建/重定向 position 通道。
 *   删除换行时几何没变的文字就没有 position track，绝对不会跟着抽一下。
 * @param originCaretRect 编辑前 caret rect（old caret rect）— 供 [ComposeEditMotion] 算 caret 插值起点。
 *   Issue #728 评论 5754839786：必填，由 [ComposeLayoutSnapshot.cursorRect] 从 oldLayout + oldSelection.end 算出。
 * @param targetCaretRect 编辑后 caret rect（new caret rect）— 供 [ComposeEditMotion] 算 caret 插值终点。
 *   Issue #728 评论 5754839786：必填，由 [ComposeLayoutSnapshot.cursorRect] 从 newLayout + newSelection.end 算出。
 * @param durationMs 动画时长（ms）— Core 建议，timeline 据此设置通道 durationNanos。
 * @param animationMode Core 动画模式 — SYSTEM_SUPPRESSED 时 timeline 不新建文字通道。
 * @param motionPolicy 动画策略 — effective 后的策略，overlay 据此决定 text timeline。
 * @param intent 原始 Core intent — 用于 offsetMap==null 时根据 replaceBounds
 *   生成 fallback survival map，防止等长替换时旧 unit 被错认成新 unit。
 */
data class ComposeVisualPatch(
    val id: Long,
    val coreTransactionIds: List<Long>,
    val oldLayout: ComposeLayoutSnapshot,
    val newLayout: ComposeLayoutSnapshot,
    val offsetMap: List<VisualOffsetMapEntry>?,
    val insertedUnits: List<TextRange>,
    val deletedUnits: List<TextRange>,
    val retainedMoves: List<RetainedMove>,
    val originCaretRect: Rect,
    val targetCaretRect: Rect,
    val durationMs: Long,
    val animationMode: AnimationModeDto,
    val motionPolicy: EditorMotionPolicy,
    val intent: EditorVisualIntent? = null,
)
