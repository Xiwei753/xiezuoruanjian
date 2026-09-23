package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot

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
 * 真正长期存在的屏幕动画状态由 [CoordinatedEditMotion] 持有（Issue #737 重写后不再有 timeline）。
 *
 * Issue #728 评论 5754045689(取代 #725 路线)：系统 caret 已透明（cursorBrush = Color.Transparent），
 * 可见 caret 由 [EditorTextFieldDrawLayer] 用统一 motion 的 caretRect 画。
 * patch 不再携带 `cursorMotionPath`，文字吞吐动画改由纯文字时间线驱动。
 *
 * Issue #728 评论 5754045689：重新把 caret 几何放回 patch，但不恢复旧 affinity/wedge 逻辑。
 * 加 [originCaretRect] / [targetCaretRect]，由 [CoordinatedEditMotion] 统一驱动 caret 移动和文字吞吐。
 * patch 不保存自己的动画进度，进度统一交给 [CoordinatedEditMotion]。
 * Enter 本身没有 glyph，所以一笔 Enter patch 可以没有 inserted unit，但必须为 old/new caret rect。
 *
 * Issue #728 评论 5754839786：[originCaretRect] / [targetCaretRect] 改成必填参数（去掉 Rect.Zero 默认值）。
 *
 * Issue #735 评论 5771063665：[animationMode] 改为 Android 自己的 [AnimationMode]，
 * 删除 [intent] 字段 — Core 已不再返回视觉意图，patch 只保存编辑事实 + Android 推导的动画模式。
 *
 * Issue #737 评论 5781634285 修复点 3：新增 [originCaretOffset] / [targetCaretOffset] —
 * 这两个 offset 是生成 [originCaretRect] / [targetCaretRect] 时用的同一份 fact selection end
 * （[ComposeVisualFrameCoordinator] 里的 oldSelectionEnd / newSelectionEnd）。
 * [CoordinatedEditMotion.fromPatch] 构造 [CaretTraversal] 时只使用这两个 offset，
 * 不再重新读 oldLayout/newLayout 的 selection.end — 快速输入、同帧 batch、layout/selection
 * 到达次序变化时，layout.selection 不保证就是生成 caret rect 时用的 offset，
 * 可能出现"rect 是 fact 的 caret，line 却是 snapshot selection 的 line"，让 traversal 在软换行附近
 * 判错"同行/跨行"。
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
 * @param retainedMoves 被挤到下一行的"保留文字"的 old/new range。
 * @param originCaretRect 编辑前 caret rect（old caret rect）— 供 [CoordinatedEditMotion] 算 caret 插值起点。
 * @param targetCaretRect 编辑后 caret rect（new caret rect）— 供 [CoordinatedEditMotion] 算 caret 插值终点。
 * @param originCaretOffset 生成 [originCaretRect] 时用的 caret offset（fact selection end，UTF-16）—
 *   供 [CoordinatedEditMotion] 构造 [CaretTraversal] 时取行号，保证 offset 与 rect 同源。
 * @param targetCaretOffset 生成 [targetCaretRect] 时用的 caret offset（fact selection end，UTF-16）—
 *   供 [CoordinatedEditMotion] 构造 [CaretTraversal] 时取行号，保证 offset 与 rect 同源。
 * @param durationMs 动画时长（ms）— Core 建议，timeline 据此设置通道 durationNanos。
 * @param animationMode Android 自己推导的动画模式 — SYSTEM_SUPPRESSED 时 timeline 不新建文字通道。
 */
@Suppress("LongParameterList")
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
    val originCaretOffset: Int,
    val targetCaretOffset: Int,
    val durationMs: Long,
    val animationMode: AnimationMode,
)
