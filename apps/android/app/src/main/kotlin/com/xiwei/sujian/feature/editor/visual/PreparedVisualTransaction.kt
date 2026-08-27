package com.xiwei.sujian.feature.editor.visual

import com.xiwei.sujian.feature.editor.layout.AndroidLineSnapshot
import com.xiwei.sujian.feature.editor.layout.LayoutRevisionSource
import uniffi.writer_core.EditorOperationKindDto

data class PreparedVisualTransaction(
    val transactionId: Long,
    val oldRevision: LayoutRevisionSource?,
    val newRevision: LayoutRevisionSource?,
    val staticPatches: List<StaticPatch>,
    val animatedSlices: List<AnimatedSlice>,
    val ownedSnapshotIds: Set<Long>,
    val referencedSnapshotIds: Set<Long>,
    val selectionDecoration: SelectionDecoration?,
    val preeditDecoration: PreeditDecoration?,
    val cursorTransition: CursorTransition?,
    val durationMs: Long,
    val blockShifts: List<BlockShift> = emptyList(),
    val operationKind: EditorOperationKindDto = EditorOperationKindDto.INSERT,
    /** #637 评论 5386066978 项3：本事务是否协同文字与光标。
     *  submit 时由引擎根据 coordinatedEnabled && hasTextMotion 设置；渲染/终态
     *  判断用这个事务级标记，不靠是否存在独立 cursorTimeline 反推。
     *  coordinated=true 时文字和光标共用同一个 visual completion（同一 timeline
     *  progress、同一 frame timestamp），textFinished == cursorFinished。 */
    val coordinated: Boolean = false,
) {
    data class StaticPatch(
        val newSnapshotId: Long,
        val lineIndex: Int,
        val destinationRect: android.graphics.RectF,
        /** Sub-regions of the line Bitmap that should be drawn as static (non-animated)
         *  content. Used when the animation's hole-punching removed regions that are not
         *  covered by any animated slice but still need to be visible — e.g. a line where
         *  only some clusters are animated and the rest must be redrawn from the snapshot
         *  because the base static draw was clipped out. Each rect is in Bitmap pixel
         *  coordinates relative to the snapshot's [sourceRect] origin. */
        val visibleSourceRects: List<android.graphics.Rect>,
    )

    /**
     * A single animated visual unit within a transaction.
     *
     * Coordinate contract:
     * - [sourceRect]: crop region inside the snapshot's line bitmap (pixel coords).
     * - [destinationRect]: final position in document coordinates (no scroll offset).
     * - [fromDestinationRect]: starting position for Move slices; null means start equals
     *   [destinationRect] (no movement) or the role uses alpha-only animation.
     *
     * Byte range contract (half-open intervals):
     * - [clusterByteStart] inclusive, [clusterByteEndExclusive] exclusive.
     * - Used for rebase matching and cross-line Move deduplication; -1 means untracked.
     */
    data class AnimatedSlice(
        val role: SliceRole,
        val snapshot: AndroidLineSnapshot?,
        val sourceRect: android.graphics.Rect,
        val destinationRect: android.graphics.RectF,
        val startAlpha: Float,
        val endAlpha: Float,
        val fromDestinationRect: android.graphics.RectF? = null,
        val clusterByteStart: Int = -1,
        val clusterByteEndExclusive: Int = -1,
        /** Reveal/swallow spec for Insert/Delete slices. null for Move/Crossfade/Static.
         *  When non-null, the renderer uses clip-rect drawing instead of alpha. */
        val revealSpec: TextRevealSpec? = null,
        /** #639 评论 5425871530 第一部分：本 slice 的 caret/reveal 几何，与 [role] 无关。
         *
         *  planner 创建 slice 时把对应 cluster/run 的 [LineClusterSnapshot.visualRectInDocument]
         *  + [LineClusterSnapshot.caretStartX] + [LineClusterSnapshot.caretEndX] 直接写进来。
         *  rebase 时不再从 [AndroidLineSnapshot.clusters] 按 byte range 反查 — 这同时修掉
         *  RunAnimation 的真实漏洞：[InsertDeletePlanner.groupClustersIntoRuns] 多字 run
         *  创建 synthetic [LineClusterSnapshot]，但合并对象不在原始
         *  [AndroidLineSnapshot.clusters] 里，按 byte range 反查必然找不到。
         *
         *  - 新位置 slice（Insert/CrossfadeNew/Move 的 newCluster）：写 new cluster 的几何。
         *  - 旧位置 slice（Delete/CrossfadeOld/Move 的 oldCluster）：写 old cluster 的几何。
         *
         *  rebase 用 [SliceVisualState.caretRevealGeometry] 把外观状态一起带进下一次 rebase，
         *  不再依赖 snapshot.clusters 精确反查。 */
        val caretRevealGeometry: CaretRevealGeometry? = null,
        /** #637 评论 5386066978 项2：本 slice 在事务内的剩余时间窗口。
         *  新事务首次播放为 [VisualProgressWindow.Full]；rebase continuation 时
         *  end = 1 - consumedFraction，让已走部分不重新计时。 */
        val progressWindow: VisualProgressWindow = VisualProgressWindow.Full,
        /** #639 评论 5421085782 问题2：rebase 专用的固定裁剪 rect（document-space）。
         *
         *  旧 Insert 可能只 reveal 到一半，rebase 成 CrossfadeOld 时不能把半个字
         *  突然变成完整字再淡出。`RebasePlanner` 在旧状态有 `revealFraction` 且能从
         *  旧 snapshot 找到匹配 cluster 时，用 cluster 的 caretStartX/caretEndX +
         *  revealFraction 经 [TextRevealGeometry.computeRevealClipRect] 算出
         *  document-space clip rect，写进此字段；`AndroidTextAnimationRenderer`
         *  画 CrossfadeOld 时若此值非空，canvas.save()+clipRect(fixedRevealClipRect)
         *  +drawBitmap(完整 bitmap, sourceRect, destinationRect)+restoreToCount()，
         *  再做 alpha 淡出。本次 CrossfadeOld 期间 clip rect 保持不动，只让 alpha 变化。
         *
         *  这与正常 Insert/Delete 的 [computeRevealClipRect] 共用同一份 caret reveal
         *  几何，不再有第二套"按 bitmap 宽度乘 fraction"的近似 — 字形 overhang
         *  （bitmap 宽度 > caret 宽度）和 RTL（caret 从右往左 reveal）都自动正确。
         *  不拿 [TextRevealSpec.initialFraction] 硬凑，因为 [TextRevealSpec.fraction]
         *  会继续向 1 推，不是"冻结当前可见部分"。 */
        val fixedRevealClipRect: android.graphics.RectF? = null,
        /** #639 评论 5427812180 缺陷4：本 slice 的静态底图 suppression 模式，与 [role] 正交。
         *
         *  planner 初次创建时按 role 设定（[defaultStaticSuppressionModeForRole]）；
         *  mapped/unmapped rebase continuation 都继续旧 [SliceVisualState.staticSuppressionMode]，
         *  不因新 role 变了瞬间切换底图 ownership。renderer [computeStaticSuppressionRegions]
         *  改按此字段判断，不再 when(slice.role)。
         *
         *  - [StaticSuppressionMode.NONE]：不 suppress（底图画完整字，动画 slice alpha 混合）。
         *  - [StaticSuppressionMode.DESTINATION_RECT]：suppress [destinationRect]（新 Layout 完整静态像素位置）。
         *  - [StaticSuppressionMode.VISIBLE_CLIP]：suppress 当前可见 clip（有 fixedRevealClipRect 用 fixed clip，
         *    否则有 revealSpec 算当前 reveal clip，否则无 suppression）。
         *
         *  null 仅作向后兼容 fallback（旧 slice 没这字段时 renderer 按 role 推断）。 */
        val staticSuppressionMode: StaticSuppressionMode? = null,
        /** #639 评论 5427812180 缺陷5：fixed clip 的 base rect（mapped 时的旧 currentRect）。
         *
         *  [fixedRevealClipRect] 是相对于 [fixedClipBaseRect] 的 document-space clip。
         *  mapped rebase 后若 slice 位置会移动（fromDestinationRect != null），
         *  renderer 每帧用 currentRect - fixedClipBaseRect 平移 fixedRevealClipRect，
         *  让 clip 跟 bitmap 一起移动，不钉在绝对坐标。
         *
         *  null 表示 fixedRevealClipRect 是绝对 document-space（位置不动或未 mapped）。
         *  unmapped continuation 原样继承旧 state 的 fixedClipBaseRect。 */
        val fixedClipBaseRect: android.graphics.RectF? = null,
    ) {
        /**
         * #639 评论 5422606865 问题2：当前视觉几何的单一入口。
         *
         * renderer 和 engine.computeSliceVisualStates 都调用这一份，保证 captureFrame
         * 记录的 slice 位置就是 renderer 真正画在屏幕上的位置。fromDestinationRect 非 null
         * 时（rebase 把 Insert/CrossfadeNew 接到旧 Move 当前位置）做位置插值，否则返回
         * destinationRect（alpha-only 动画）。
         *
         * 几何：先 [VisualProgressWindow.map] 得到 localProgress，再从 from→destination
         * 线性插值四条边。fromDestinationRect 为 null 时 from=destinationRect，插值
         * 退化为常量 destinationRect，与原有 alpha-only 行为完全一致。
         */
        fun visualDestinationRectAt(globalProgress: Float): android.graphics.RectF {
            val p = progressWindow.map(globalProgress)
            val from = fromDestinationRect ?: destinationRect
            return android.graphics.RectF(
                from.left + (destinationRect.left - from.left) * p,
                from.top + (destinationRect.top - from.top) * p,
                from.right + (destinationRect.right - from.right) * p,
                from.bottom + (destinationRect.bottom - from.bottom) * p,
            )
        }
    }

    data class SelectionDecoration(
        val startUtf16: Int,
        val endUtf16: Int,
    )

    data class PreeditDecoration(
        val startUtf16: Int,
        val endUtf16: Int,
        val underlineColor: Int,
    )

    data class CursorTransition(
        val fromX: Float,
        val fromY: Float,
        val fromHeight: Float,
        val toX: Float,
        val toY: Float,
        val toHeight: Float,
        val shouldAnimate: Boolean,
        /** #637 评论 5386066978 项2：光标过渡的剩余时间窗口。
         *  rebase continuation 时 end = 1 - consumedFraction，让光标已走部分
         *  不重新计时，与文字 slice 保持同一运动速度。 */
        val progressWindow: VisualProgressWindow = VisualProgressWindow.Full,
    ) {
        /**
         * #637 评论 5389230907：给定全局事务 [globalProgress]，返回当前光标 RectF。
         *
         * 单一事实来源 — renderer (`AndroidTextAnimationRenderer.drawAnimatedCursor`)
         * 和 engine (`AndroidTextAnimationEngine.computeCurrentCursorRect`) 都调用
         * 这个 helper，避免两边各维护一份插值公式导致 rebase 后 progressWindow
         * 不一致（renderer 用 localProgress 画在 0.5 处，engine 却用全局 progress=0.2
         * 记成更靠后的位置，下一次 rebase 光标回跳）。
         *
         * 几何：先 [VisualProgressWindow.map] 得到 localProgress，再线性插值
         * X/Y/height；宽度固定 2px（视觉光标条宽，不来自布局）。
         */
        fun rectAt(globalProgress: Float): android.graphics.RectF {
            val localProgress = progressWindow.map(globalProgress)
            val currentX = fromX + (toX - fromX) * localProgress
            val currentY = fromY + (toY - fromY) * localProgress
            val currentHeight = fromHeight + (toHeight - fromHeight) * localProgress
            return android.graphics.RectF(currentX, currentY, currentX + 2f, currentY + currentHeight)
        }
    }

    /**
     * Block-level vertical shift for a contiguous range of paragraphs after the edit
     * paragraph group whose Y geometry shifted but whose text content is identical.
     *
     * Line range convention: [startLineIndex] inclusive, [endLineIndexExclusive] exclusive
     * (half-open). The renderer uses these indices directly to clip and translate the
     * static new-layout text, avoiding per-frame UTF-8→UTF-16 offset conversion that
     * could land on the wrong line when the exclusive end coincides with a paragraph boundary.
     *
     * [deltaY] is positive when the block moved downward (newTop > oldTop).
     * The renderer interpolates: translateY = deltaY * (progress - 1), so at progress=0
     * the text is at its old position (shifted by -deltaY from the new layout) and at
     * progress=1 it rests at the new layout position (no shift).
     *
     * Merging: [AndroidVisualPlanner.mergeAdjacentBlockShifts] merges consecutive
     * BlockShifts whose line ranges are adjacent and whose deltaY is identical into a
     * single entry. This ensures the renderer performs at most one [layout.draw] per
     * merged block per frame, not one per paragraph — critical for long documents where
     * many paragraphs shift by the same amount. Geometric bounds (left/right) use the
     * min/max across all merged lines to ensure the clip rect covers every intermediate
     * line regardless of varying line widths.
     */
    data class BlockShift(
        val startLineIndex: Int,
        val endLineIndexExclusive: Int,
        val top: Float,
        val bottom: Float,
        val left: Float,
        val right: Float,
        val deltaY: Float,
        /** UTF-8 byte offset of the first line in this block. Used by rebase matching
         *  instead of [startLineIndex] because line indices shift across revisions when
         *  hard breaks are inserted/deleted — the old transaction's line N may become
         *  line N+1 in the new revision, causing line-index-based matching to fail.
         *
         *  Rebase continuity: [applyRebaseToBlockShifts] matches old/new BlockShifts by
         *  [startUtf8] and adjusts deltaY to (newDeltaY - oldCurrentTranslateY). This
         *  ensures the suffix text starts from the on-screen position of the old animation
         *  rather than jumping back to the full -newDeltaY offset. Without [startUtf8],
         *  line-index-based matching would pair the wrong BlockShifts after hard-break
         *  insertion/deletion, producing incorrect rebase adjustments and visible jumps. */
        val startUtf8: Int = -1,
        /** Exclusive UTF-8 byte offset of the last line in this block. Provides a stable
         *  document range [startUtf8, endUtf8Exclusive) for cross-revision matching.
         *  When [startUtf8] alone matches but [endUtf8Exclusive] differs, the match is
         *  downgraded from exact to approximate — this prevents pairing BlockShifts that
         *  cover different document regions after hard-break insertion/deletion, which
         *  would produce incorrect deltaY adjustments and visible jumps in the suffix text. */
        val endUtf8Exclusive: Int = -1,
        /** #637 评论 5386066978 项2：BlockShift 的剩余时间窗口。
         *  rebase continuation 时 end = 1 - consumedFraction，让后缀块已走部分
         *  不重新计时，保持匀速。 */
        val progressWindow: VisualProgressWindow = VisualProgressWindow.Full,
    )

    /**
     * #639 评论 5425871530 第一部分：slice 自带的 caret/reveal 几何。
     *
     * 轻 role rebase 外观续播要闭环，slice 必须自己持有 caret 几何，不能 rebase 时再从
     * [AndroidLineSnapshot.clusters] 按 byte range 反查 — RunAnimation 的 synthetic run
     * 不在原始 clusters 里，反查必然失败。
     *
     * - [visualRect]：cluster/run 在 document-space 的 visualRect（来自
     *   [LineClusterSnapshot.visualRectInDocument]）。
     * - [caretStartX]/[caretEndX]：cluster/run 的 logical caret X（来自
     *   [LineClusterSnapshot.caretStartX]/[caretEndX]）。
     *
     * rebase 时 [RebasePlanner] 直接消费这份几何重建 REVEAL continuation，不再按
     * [SliceRole] 分支处理；renderer 三条轨（位置/alpha/reveal）正交绘制也用这份几何
     * 算 clip，不再按 role 决定是否裁剪。
     */
    data class CaretRevealGeometry(
        val visualRect: android.graphics.RectF,
        val caretStartX: Float,
        val caretEndX: Float,
    )
}

/** Animation slice roles.
 *  Insert = caret reveal (clip from anchor toward boundary).
 *  Delete = caret swallow (clip from boundary toward anchor).
 *  Move = geometric position shift (alpha stays 1).
 *  CrossfadeOld/New = shaping changed, paired fade-out + fade-in.
 *  Static = no animation. */
enum class SliceRole {
    Insert,
    Delete,
    Move,
    CrossfadeOld,
    CrossfadeNew,
    Static,
}

/** #639 评论 5427812180 缺陷4：静态底图 suppression 模式，与 [SliceRole] 正交。
 *
 *  mapped rebase 继续旧视觉轨后 role 和"静态底图怎么挖洞"会不一致，所以 suppression
 *  不能按 [SliceRole] 判断，必须按独立 mode。planner 初次创建时按 role 设定
 *  （[defaultStaticSuppressionModeForRole]），continuation 继承旧 state 的 mode。 */
enum class StaticSuppressionMode {
    /** 不 suppress：底图画完整字，动画 slice alpha 混合（CrossfadeOld 语义）。 */
    NONE,

    /** suppress [PreparedVisualTransaction.AnimatedSlice.destinationRect]（新 Layout 完整静态像素位置）。 */
    DESTINATION_RECT,

    /** suppress 当前可见 clip：有 fixedRevealClipRect 用 fixed clip，否则有 revealSpec 算当前 reveal clip。 */
    VISIBLE_CLIP,
}

/** #639 评论 5427812180 缺陷4：按 [SliceRole] 推断默认 [StaticSuppressionMode]。
 *
 *  planner 初次创建 slice 时调用，mapped/unmapped continuation 继承旧 state 的 mode
 *  而非重新按 role 推断。renderer fallback（slice.staticSuppressionMode == null 时）也用此函数。 */
fun defaultStaticSuppressionModeForRole(role: SliceRole): StaticSuppressionMode {
    return when (role) {
        SliceRole.Insert, SliceRole.Move, SliceRole.CrossfadeNew, SliceRole.Static ->
            StaticSuppressionMode.DESTINATION_RECT
        SliceRole.Delete ->
            StaticSuppressionMode.VISIBLE_CLIP
        SliceRole.CrossfadeOld ->
            StaticSuppressionMode.NONE
    }
}

/** Lifecycle states of a visual transaction. Only Rendering/Paused produce frames. */
enum class TransactionState {
    Pending,
    Prepared,
    Rendering,
    Paused,
    Completed,
    Cancelled,
}
