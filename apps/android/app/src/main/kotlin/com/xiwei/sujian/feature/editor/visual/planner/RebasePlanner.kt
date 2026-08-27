package com.xiwei.sujian.feature.editor.visual.planner

import com.xiwei.sujian.feature.editor.layout.AndroidLineSnapshot
import com.xiwei.sujian.feature.editor.layout.LineClusterSnapshot
import com.xiwei.sujian.feature.editor.visual.PreparedVisualTransaction
import com.xiwei.sujian.feature.editor.visual.SliceRole
import com.xiwei.sujian.feature.editor.visual.SliceVisualState
import com.xiwei.sujian.feature.editor.visual.StaticSuppressionMode
import com.xiwei.sujian.feature.editor.visual.TextRevealGeometry
import com.xiwei.sujian.feature.editor.visual.TextRevealMode
import com.xiwei.sujian.feature.editor.visual.TextRevealSpec
import com.xiwei.sujian.feature.editor.visual.VisualFrameSnapshot
import com.xiwei.sujian.feature.editor.visual.VisualProgressWindow
import com.xiwei.sujian.feature.editor.visual.defaultStaticSuppressionModeForRole
import uniffi.writer_core.RebaseSliceMappingDto

class RebasePlanner {
    /**
     * #639 评论 5424986783：rebase continuation 应按 [SliceVisualState.currentRect]
     * 与 [destinationRectOf] 的实际几何关系判断 slice 是否还有位置运动，而非按旧
     * [SliceRole] 判断。连续 rebase（Move → Insert/CrossfadeNew 后再次 rebase）时
     * 旧 role 已变成 Insert/CrossfadeNew，但 currentRect 仍在 from→destination 中间，
     * 必须继承 fromRect 才不跳变。这三个 helper 把几何判断收成一处，避免分散重复。
     */
    private fun currentRectOf(state: SliceVisualState): android.graphics.RectF =
        android.graphics.RectF(
            state.currentLeft,
            state.currentTop,
            state.currentRight,
            state.currentBottom,
        )

    private fun destinationRectOf(state: SliceVisualState): android.graphics.RectF =
        android.graphics.RectF(
            state.destinationLeft,
            state.destinationTop,
            state.destinationRight,
            state.destinationBottom,
        )

    /**
     * #606: 将 Core 计算的旧→新逻辑 slice 对应关系应用到新事务的 animated slices。
     *
     * 旧→新 slice 的逻辑对应关系由 Core（`compute_rebase_slice_mappings`）
     * 唯一计算并作为 [mappings] 传入（直接消费 `RebaseSliceMappingDto`，
     * 平台端不再维护本地副本）— 平台端不再使用任何本地匹配逻辑
     * （compatibleRebaseRoles / findRebaseIndexByClusterByteRange /
     * findRebaseIndexByLineAndRole / findRebaseIndexClosestByPosition 已删除）。
     * 本方法只负责：
     * 1. 对映射到的旧 slice，把旧帧当前 `RectF/alpha/revealFraction/Bitmap`
     *    填入新 slice 的 `fromDestinationRect/initialFraction/startAlpha`；
     * 2. 对 Core 无映射的旧 slice，按 Core 的继续/结束语义处理（仍在进行中的
     *    Delete/CrossfadeOld 继续吞完/淡出，Move/Insert/CrossfadeNew 继续原动画），
     *    这是旧帧动画状态的平台侧延续，不重新猜测逻辑对应。
     */
    fun applyRebaseToSlices(
        newSlices: List<PreparedVisualTransaction.AnimatedSlice>,
        rebaseSnapshot: VisualFrameSnapshot,
        snapshotLookup: Map<Long, AndroidLineSnapshot> = emptyMap(),
        mappings: List<RebaseSliceMappingDto> = emptyList(),
    ): List<PreparedVisualTransaction.AnimatedSlice> {
        if (rebaseSnapshot.sliceVisualStates.isEmpty()) return newSlices

        val usedRebaseIndices = mutableSetOf<Int>()
        val result = mutableListOf<PreparedVisualTransaction.AnimatedSlice>()

        for ((newIdx, slice) in newSlices.withIndex()) {
            val mapping = mappings.firstOrNull { it.newSliceIndex.toInt() == newIdx }
            val rebaseIdx = mapping?.oldSliceIndex?.toInt()
            if (rebaseIdx != null && rebaseIdx >= 0 && rebaseIdx < rebaseSnapshot.sliceVisualStates.size) {
                usedRebaseIndices.add(rebaseIdx)
                val rebaseState = rebaseSnapshot.sliceVisualStates[rebaseIdx]
                result.add(applyRebaseState(slice, rebaseState, snapshotLookup))
            } else {
                result.add(slice)
            }
        }

        for ((stateIdx, state) in rebaseSnapshot.sliceVisualStates.withIndex()) {
            if (stateIdx in usedRebaseIndices) continue
            handleUnmappedRebaseState(state, snapshotLookup, result)
        }

        return result
    }

    /**
     * #639 评论 5427183226：处理未映射旧 slice 的 appearance continuation。
     *
     * 不再先看 [SliceRole] 决定视觉 continuation，而是先看三条视觉轨本身：
     * - positionRemaining：currentRect != destinationRect
     * - alphaRemaining：abs(currentAlpha - targetAlpha) > EPS
     * - revealRemaining：revealFraction != null && revealFraction < 0.99f
     * - fixedClipActive：fixedRevealClipRect != null
     *
     * 只要任一轨未完成，就重建同一个 role 的 continuation。sourceRect 优先用
     * [SliceVisualState.sourceRect]（上一帧实际画的 source crop），fallback
     * matchedCluster → snapshot.sourceRect（旧状态兼容）。revealSpec 用
     * state.revealMode + state.revealFraction + caretRevealGeometry 重建，
     * 不再只认 Insert/Delete。fixedRevealClipRect 原样带下去。
     */
    private fun handleUnmappedRebaseState(
        state: SliceVisualState,
        snapshotLookup: Map<Long, AndroidLineSnapshot>,
        result: MutableList<PreparedVisualTransaction.AnimatedSlice>,
    ) {
        val snapshot = snapshotLookup[state.snapshotId]
        // matchedCluster 仅作向后兼容 fallback（旧快照没有 caretRevealGeometry 时从 snapshot 反查）。
        val matchedCluster =
            if (snapshot != null && state.caretRevealGeometry == null) {
                snapshot.clusters.firstOrNull {
                    it.documentByteStart == state.clusterByteStart &&
                        it.documentByteEndExclusive == state.clusterByteEndExclusive
                }
            } else {
                null
            }
        // #639 评论 5427183226 缺口1：sourceRect 优先用 state.sourceRect（上一帧实际
        // 画的 source crop），fallback matchedCluster（旧快照）→ snapshot.sourceRect
        // （最旧兼容）。不再把 snapshot.sourceRect 当新格式状态的正常路径。
        val sourceRect =
            state.sourceRect?.let { android.graphics.Rect(it) }
                ?: matchedCluster?.sourceRectInLineImage
                ?: snapshot?.sourceRect
                ?: android.graphics.Rect(0, 0, 0, 0)
        // #639 评论 5427183226 缺口2：按三条视觉轨判断是否需要 continuation，不再先看 SliceRole。
        val currentRect = currentRectOf(state)
        val destRect = destinationRectOf(state)
        val positionRemaining = currentRect != destRect
        val alphaRemaining = kotlin.math.abs(state.currentAlpha - state.targetAlpha) > 0.01f
        val hasCaretGeometry = state.caretRevealGeometry != null || matchedCluster != null
        // revealRemaining 只在能重建 revealSpec 时才算剩余（需要 caret 几何）。
        val revealRemaining =
            state.revealFraction != null && state.revealFraction < 0.99f && hasCaretGeometry
        // #639 评论 5428952431 缺陷3：fixed clip 只是裁剪修饰，不是 liveness 轨。
        // 只要 position/alpha/reveal 三条真正的时间轨有任意一条还没结束，continuation 就继续携带 fixed clip；
        // 三条都结束，fixed clip 一起销毁。否则会留下透明 slice 继续挖静态正文。
        if (!positionRemaining && !alphaRemaining && !revealRemaining) {
            return
        }
        result.add(
            buildVisualContinuation(
                state, snapshot, sourceRect, matchedCluster, hasCaretGeometry,
                positionRemaining, revealRemaining,
            ),
        )
    }

    /**
     * #639 评论 5427183226：统一的未映射 continuation builder。
     *
     * 不再按 SliceRole 分支（buildMoveContinuation/buildInsertRevealContinuation/
     * buildAlphaOrPositionContinuation/buildFadingOutContinuation 已合并到此）。
     * - sourceRect = state.sourceRect（已由调用方解析）
     * - destinationRect = state.destinationRect
     * - fromDestinationRect = currentRect（有位置剩余时）
     * - startAlpha = state.currentAlpha、endAlpha = state.targetAlpha（不硬抬 alpha）
     * - revealSpec 用 state.revealMode + state.revealFraction + caretRevealGeometry 重建
     * - fixedRevealClipRect 原样带下去
     */
    private fun buildVisualContinuation(
        state: SliceVisualState,
        snapshot: AndroidLineSnapshot?,
        sourceRect: android.graphics.Rect,
        matchedCluster: LineClusterSnapshot?,
        hasCaretGeometry: Boolean,
        positionRemaining: Boolean,
        revealRemaining: Boolean,
    ): PreparedVisualTransaction.AnimatedSlice {
        val currentRect = currentRectOf(state)
        val destRect = destinationRectOf(state)
        val continuedWindow = VisualProgressWindow.fromRemainingFraction(state.remainingFraction)
        // #639 评论 5427183226 缺口2：revealSpec 用 state.revealMode + state.revealFraction
        // + caretRevealGeometry 重建，不再只认 Insert/Delete。
        val revealSpec =
            if (revealRemaining && hasCaretGeometry) {
                buildContinuationRevealSpec(state, matchedCluster, destRect)
            } else {
                null
            }
        // #639 评论 5427183226 缺口2：startAlpha = state.currentAlpha、endAlpha = state.targetAlpha。
        // 不再硬抬 alpha。普通 Delete 仍能保持 alpha 1 -> 1 + SWALLOW（targetAlpha=1），
        // 由 CrossfadeOld 映射过来的 Delete 如果已经是 alpha .4 -> 0，下一次 rebase 不会变回 1。
        return PreparedVisualTransaction.AnimatedSlice(
            role = state.role,
            snapshot = snapshot,
            sourceRect = sourceRect,
            destinationRect = destRect,
            startAlpha = state.currentAlpha,
            endAlpha = state.targetAlpha,
            fromDestinationRect = if (positionRemaining) currentRect else null,
            clusterByteStart = state.clusterByteStart,
            clusterByteEndExclusive = state.clusterByteEndExclusive,
            revealSpec = revealSpec,
            fixedRevealClipRect = state.fixedRevealClipRect?.let { android.graphics.RectF(it) },
            caretRevealGeometry = state.caretRevealGeometry,
            progressWindow = continuedWindow,
            // #639 评论 5427812180 缺陷4/5：unmapped continuation 继续旧 state 的
            // staticSuppressionMode 和 fixedClipBaseRect，不因 role 变了瞬间切换底图 ownership。
            staticSuppressionMode = state.staticSuppressionMode,
            fixedClipBaseRect = state.fixedClipBaseRect?.let { android.graphics.RectF(it) },
        )
    }

    /**
     * #639 评论 5427183226：用 state.revealMode + state.revealFraction + caretRevealGeometry
     * 重建 revealSpec。mode 优先用 state.revealMode（新格式 SliceVisualState 保存了），
     * fallback 按 role 推断（向后兼容旧快照）。caret 几何优先用 state.caretRevealGeometry，
     * fallback 用 matchedCluster。
     */
    private fun buildContinuationRevealSpec(
        state: SliceVisualState,
        matchedCluster: LineClusterSnapshot?,
        destRect: android.graphics.RectF,
    ): TextRevealSpec? {
        val fraction = state.revealFraction ?: return null
        val mode =
            state.revealMode
                ?: when (state.role) {
                    SliceRole.Delete -> TextRevealMode.SWALLOW
                    SliceRole.Insert, SliceRole.Move, SliceRole.CrossfadeNew -> TextRevealMode.REVEAL
                    else -> return null
                }
        val visualRect: android.graphics.RectF
        val caretStartX: Float
        val caretEndX: Float
        if (state.caretRevealGeometry != null) {
            val g = state.caretRevealGeometry
            visualRect = g.visualRect
            caretStartX = g.caretStartX
            caretEndX = g.caretEndX
        } else if (matchedCluster != null) {
            visualRect = matchedCluster.visualRectInDocument
            caretStartX = matchedCluster.caretStartX
            caretEndX = matchedCluster.caretEndX
        } else {
            return null
        }
        val (shiftedAnchorX, shiftedBoundaryFromX, shiftedBoundaryToX) =
            TextRevealGeometry.shiftClusterCaretGeometry(
                visualRect,
                caretStartX,
                caretEndX,
                destRect,
                mode,
            )
        return TextRevealSpec(
            mode = mode,
            anchorX = shiftedAnchorX,
            boundaryFromX = shiftedBoundaryFromX,
            boundaryToX = shiftedBoundaryToX,
            progressStart = 0f,
            progressEnd = 1f,
            initialFraction = fraction,
        )
    }

    /** #639 评论 5433981610：emergence role 是 Move/Insert/CrossfadeNew，
     *  Core pair-aware mapping 会把它们特殊接到 CrossfadeOld。 */
    private fun isEmergenceRole(role: SliceRole): Boolean =
        role == SliceRole.Move ||
            role == SliceRole.Insert ||
            role == SliceRole.CrossfadeNew

    fun applyRebaseState(
        slice: PreparedVisualTransaction.AnimatedSlice,
        rebaseState: SliceVisualState,
        snapshotLookup: Map<Long, AndroidLineSnapshot> = emptyMap(),
    ): PreparedVisualTransaction.AnimatedSlice {
        val snapshot = slice.snapshot ?: snapshotLookup[rebaseState.snapshotId]
        val oldCurrentRect =
            android.graphics.RectF(
                rebaseState.currentLeft,
                rebaseState.currentTop,
                rebaseState.currentRight,
                rebaseState.currentBottom,
            )
        // #637 评论 5386573878：rebase continuation 窗口 — 直接消费旧帧保存的
        // remainingFraction，不再从 localProgress 重新推，连续 rebase 不会反复减速。
        val continuedWindow = VisualProgressWindow.fromRemainingFraction(rebaseState.remainingFraction)

        // #639 评论 5427812180 缺陷3：统一 mapped visual continuation。
        // mapped rebase 继续旧事务当前正在播放的视觉轨，不是重新启动新 SliceRole 默认视觉轨。
        // 新 slice 提供逻辑终点（role/snapshot/sourceRect/destinationRect/clusterByteRange/
        // caretRevealGeometry），旧 SliceVisualState 提供当前屏幕真实视觉状态
        // （currentRect/currentAlpha/targetAlpha/revealMode/revealFraction/fixedRevealClipRect/
        // caretRevealGeometry/remainingFraction）。
        // Static 不参与动画，原样返回。
        if (slice.role == SliceRole.Static) return slice

        // #639 评论 5433268179：mapped CrossfadeOld 必须原地退场，位置锁在 rebase 当下
        // 的 oldCurrentRect，禁止旧字从旧行斜飞到新行。destinationRect 和 fromDestinationRect
        // 都指向 oldCurrentRect，visualDestinationRectAt 退化为常量。
        // 其他 role 保持原有行为：Move 总是设 fromDestinationRect，其余有位置差时设。
        val mappedDestinationRect: android.graphics.RectF
        val fromDestinationRect: android.graphics.RectF?

        if (slice.role == SliceRole.CrossfadeOld) {
            mappedDestinationRect = android.graphics.RectF(oldCurrentRect)
            fromDestinationRect = null
        } else {
            mappedDestinationRect = android.graphics.RectF(slice.destinationRect)
            fromDestinationRect =
                if (slice.role == SliceRole.Move) {
                    android.graphics.RectF(oldCurrentRect)
                } else if (oldCurrentRect != mappedDestinationRect) {
                    android.graphics.RectF(oldCurrentRect)
                } else {
                    null
                }
        }

        // alpha 轨：继续旧 currentAlpha -> targetAlpha，不硬抬 alpha、不写死 endAlpha=0f。
        // 普通 Delete 仍能保持 alpha 1 -> 1 + SWALLOW（targetAlpha=1），由 CrossfadeOld 映射
        // 过来的 Delete 如果已经是 alpha .4 -> 0，下一次 rebase 不会变回 1。
        // #639 评论 5428952431 缺陷1：CrossfadeOld 的 target 必须来自新语义（alpha -> 0），
        // 当前状态才来自旧 state。旧 SliceVisualState 决定新事务第一帧的当前视觉状态
        // （currentRect/currentAlpha/reveal/fixedClip/suppression）；新 slice 决定逻辑终点语义。
        // CrossfadeOld 是"旧像素必须退出"的目标语义，endAlpha 必须是 0。startAlpha 永远取旧 currentAlpha。
        val startAlpha = rebaseState.currentAlpha
        val endAlpha =
            if (slice.role == SliceRole.CrossfadeOld) {
                0f
            } else {
                rebaseState.targetAlpha
            }

        // reveal 轨 + fixed clip：
        // - 旧 state 有 fixedRevealClipRect：原样继承（冻结半截字继续）。
        //   如果新 slice 用 revealSpec（Delete/Insert/Move/CrossfadeNew）且旧 state 也有 revealFraction，
        //   继续 revealSpec（initialFraction=旧 revealFraction）；否则 revealSpec=null。
        // - 旧 state 无 fixedRevealClipRect 但有 revealFraction（正在播放 reveal/swallow）：
        //   - 新 slice 用 revealSpec（Delete/Insert/Move/CrossfadeNew）：继续 revealSpec，
        //     initialFraction=旧 revealFraction。Move/CrossfadeNew 也继续 revealSpec 保持半截可见。
        //   - 新 slice 是 CrossfadeOld（不用 revealSpec，用 alpha 淡出）：算 fixedRevealClipRect 冻结。
        // - 旧 state 都没有：revealSpec=null, fixedRevealClipRect=null。
        // CrossfadeOld 不用 revealSpec（alpha 混合语义），其他非 Static role 都可以继续 revealSpec。
        val sliceUsesRevealSpec = slice.role != SliceRole.CrossfadeOld

        val fixedRevealClipRect: android.graphics.RectF?
        val revealSpec: TextRevealSpec?

        if (rebaseState.fixedRevealClipRect != null) {
            // #639 评论 5428952431 缺陷2：mapped rebase 前先把旧 raw clip 归一化成
            // 当前屏幕真实 clip（effectiveOldClip），避免第二次带 base 的 mapped rebase
            // 把之前累计的平移清零。
            val effectiveOldClip = effectiveFixedClipAt(rebaseState, oldCurrentRect)
            fixedRevealClipRect = effectiveOldClip
            revealSpec =
                if (sliceUsesRevealSpec && rebaseState.revealFraction != null) {
                    rebuildMappedRevealSpec(slice, rebaseState)
                } else {
                    null
                }
        } else if (rebaseState.revealFraction != null) {
            if (sliceUsesRevealSpec) {
                revealSpec = rebuildMappedRevealSpec(slice, rebaseState)
                fixedRevealClipRect = null
            } else {
                revealSpec = null
                // fresh computeFixedRevealClipRect 算出来的本来就是 oldCurrentRect 上的真实 clip，
                // 也走同样的 base 规则（下面 fixedClipBaseRect 会按 fromDestinationRect 决定）。
                fixedRevealClipRect = computeFixedRevealClipRect(rebaseState, snapshotLookup, snapshot)
            }
        } else {
            fixedRevealClipRect = null
            revealSpec = null
        }

        // #639 评论 5428952431 缺陷2：mapped fixed clip 先归一化成 effective clip 再建立新 base。
        // 如果这次还会从 oldCurrentRect -> new destination 移动（fromDestinationRect != null）：
        //   fixedRevealClipRect = effectiveOldClip（已归一化），fixedClipBaseRect = oldCurrentRect。
        // 如果这次位置不再移动：
        //   fixedRevealClipRect = effectiveOldClip（已归一化），fixedClipBaseRect = null。
        // unmapped continuation 继续原轨时保留 raw clip + old base，不需要归一化（在 buildVisualContinuation 中处理）。
        val fixedClipBaseRect: android.graphics.RectF? =
            if (fixedRevealClipRect != null && fromDestinationRect != null) {
                oldCurrentRect
            } else if (fixedRevealClipRect != null) {
                null
            } else {
                rebaseState.fixedClipBaseRect?.let { android.graphics.RectF(it) }
            }

        // #639 评论 5433981610：pair-aware mapping 翻面时 staticSuppressionMode 必须跟着翻。
        // 旧 Move/Insert/CrossfadeNew（emergence role）被 Core 特殊接到 CrossfadeOld 时，
        // 视觉 ownership 已从"新侧替代静态字"翻成"旧侧覆盖层"，suppression 必须是 NONE，
        // 不能机械继承旧 role 的 DESTINATION_RECT（否则 renderer 会在 oldCurrentRect 挖洞）。
        // Delete -> CrossfadeOld 继续旧 VISIBLE_CLIP（吞字 ownership 连续性），
        // CrossfadeOld -> Delete 继续旧 NONE，同类 emergence 互相 mapped 继续旧 mode。
        val staticSuppressionMode =
            if (slice.role == SliceRole.CrossfadeOld && isEmergenceRole(rebaseState.role)) {
                StaticSuppressionMode.NONE
            } else {
                rebaseState.staticSuppressionMode
                    ?: defaultStaticSuppressionModeForRole(rebaseState.role)
            }

        return slice.copy(
            snapshot = snapshot,
            destinationRect = mappedDestinationRect,
            startAlpha = startAlpha,
            endAlpha = endAlpha,
            fromDestinationRect = fromDestinationRect,
            revealSpec = revealSpec,
            fixedRevealClipRect = fixedRevealClipRect,
            fixedClipBaseRect = fixedClipBaseRect,
            staticSuppressionMode = staticSuppressionMode,
            progressWindow = continuedWindow,
        )
    }

    /**
     * #639 评论 5427812180 缺陷3：用旧 state 的 revealMode + revealFraction + 新 slice 的
     * caretRevealGeometry 重建 mapped revealSpec。mode 优先用旧 state.revealMode（继续
     * 旧事务正在播放的 reveal/swallow），geometry 优先用新 slice.caretRevealGeometry
     * （新位置几何），fallback 用旧 state.caretRevealGeometry。
     */
    private fun rebuildMappedRevealSpec(
        slice: PreparedVisualTransaction.AnimatedSlice,
        rebaseState: SliceVisualState,
    ): TextRevealSpec? {
        val fraction = rebaseState.revealFraction ?: return null
        // #639 评论 5427812180 缺陷3：mode 优先用旧 state.revealMode（继续旧事务正在播放
        // 的 reveal/swallow），fallback 按 rebaseState.role 推断（向后兼容旧快照没存 revealMode）。
        val mode =
            rebaseState.revealMode
                ?: when (rebaseState.role) {
                    SliceRole.Delete -> TextRevealMode.SWALLOW
                    SliceRole.Insert, SliceRole.Move, SliceRole.CrossfadeNew -> TextRevealMode.REVEAL
                    else -> return null
                }
        // geometry 优先用新 slice.caretRevealGeometry（新位置几何），fallback 用
        // rebaseState.caretRevealGeometry（旧 state 携带的几何），再 fallback 用
        // slice.destinationRect 构造简单几何（whole-line 或测试场景）。
        val geometry = slice.caretRevealGeometry ?: rebaseState.caretRevealGeometry
        val (visualRect, caretStartX, caretEndX) =
            if (geometry != null) {
                Triple(geometry.visualRect, geometry.caretStartX, geometry.caretEndX)
            } else {
                Triple(slice.destinationRect, slice.destinationRect.left, slice.destinationRect.right)
            }
        val (shiftedAnchorX, shiftedBoundaryFromX, shiftedBoundaryToX) =
            TextRevealGeometry.shiftClusterCaretGeometry(
                visualRect,
                caretStartX,
                caretEndX,
                slice.destinationRect,
                mode,
            )
        return TextRevealSpec(
            mode = mode,
            anchorX = shiftedAnchorX,
            boundaryFromX = shiftedBoundaryFromX,
            boundaryToX = shiftedBoundaryToX,
            progressStart = 0f,
            progressEnd = 1f,
            initialFraction = fraction,
        )
    }

    /**
     * #639 评论 5428952431 缺陷2：计算 state 在 currentRect 位置下的 effective document-space fixed clip。
     *
     * mapped rebase 前先把旧 raw clip 归一化成当前屏幕真实 clip，避免第二次带 base 的
     * mapped rebase 把之前累计的平移清零。base 为 null 表示 raw clip 已是绝对坐标。
     */
    private fun effectiveFixedClipAt(
        state: SliceVisualState,
        currentRect: android.graphics.RectF,
    ): android.graphics.RectF? {
        val raw = state.fixedRevealClipRect ?: return null
        val base = state.fixedClipBaseRect ?: return android.graphics.RectF(raw)
        val dx = currentRect.left - base.left
        val dy = currentRect.top - base.top
        return android.graphics.RectF(
            raw.left + dx,
            raw.top + dy,
            raw.right + dx,
            raw.bottom + dy,
        )
    }

    /**
     * #639 评论 5421085782 问题2：为 CrossfadeOld 算 rebase 冻结的 document-space clip rect。
     *
     * 旧 Insert 只 reveal 到一半时，rebase 成 CrossfadeOld 不能把半个字突然变成完整字
     * 再淡出。用旧 snapshot 匹配 cluster 的 caretStartX/caretEndX + rebaseState.revealFraction
     * 经 [TextRevealGeometry.computeClusterRevealClipRect] 算出冻结的 clip rect。
     *
     * - 旧 snapshot 优先从 [snapshotLookup] 按 [rebaseState.snapshotId] 取，fallback
     *   用传入的 [fallbackSnapshot]（即新 slice 自己的 snapshot）。
     * - cluster 用 [rebaseState.clusterByteStart]/[rebaseState.clusterByteEndExclusive]
     *   在旧 snapshot 的 clusters 里匹配。
     * - destination 用旧 Insert 的 currentRect（[rebaseState.currentLeft/Top/Right/Bottom]），
     *   即当前屏幕真实位置。
     * - mode = REVEAL，与 CaretRevealPlanner Insert REVEAL spec 一致。
     *
     * #639 评论 5424613367 问题1：caret 几何来自旧 snapshot 的 visualRectInDocument，
     * 当前 destination 是 currentRect（旧 Insert 可能正在 from/destination 中间）。
     * 用 [TextRevealGeometry.computeClusterRevealClipRect] 统一平移，与未匹配 Insert
     * continuation 共用同一份平移公式。
     *
     * 返回 null 的条件：revealFraction 为 null（旧 slice 不是 reveal 动画）、
     * 找不到旧 snapshot、找不到匹配 cluster。此时 renderer 画完整 bitmap 不裁剪
     * （fallback 到旧 CrossfadeOld 行为）。
     */
    private fun computeFixedRevealClipRect(
        rebaseState: SliceVisualState,
        snapshotLookup: Map<Long, AndroidLineSnapshot>,
        fallbackSnapshot: AndroidLineSnapshot?,
    ): android.graphics.RectF? {
        val fraction = rebaseState.revealFraction ?: return null
        val destination =
            android.graphics.RectF(
                rebaseState.currentLeft,
                rebaseState.currentTop,
                rebaseState.currentRight,
                rebaseState.currentBottom,
            )
        // #639 评论 5425871530 第四部分：caret 几何优先用 rebaseState.caretRevealGeometry
        // （slice 自带，不依赖 snapshot.clusters 反查 — synthetic run 也能继续），
        // fallback 用旧 snapshot 的 matchedCluster（向后兼容旧快照）。
        // #639 评论 5427812180 缺陷2：mode 用 rebaseState.revealMode（继续旧事务正在播放
        // 的 reveal/swallow），不再写死 REVEAL — 旧 Delete SWALLOW -> CrossfadeOld 冻结
        // 时用 SWALLOW 几何，旧 Insert REVEAL -> CrossfadeOld 冻结时用 REVEAL 几何。
        val mode = rebaseState.revealMode ?: TextRevealMode.REVEAL
        if (rebaseState.caretRevealGeometry != null) {
            val g = rebaseState.caretRevealGeometry
            return TextRevealGeometry.computeClusterRevealClipRect(
                g.visualRect,
                g.caretStartX,
                g.caretEndX,
                destination,
                mode,
                fraction,
            )
        }
        val oldSnapshot = snapshotLookup[rebaseState.snapshotId] ?: fallbackSnapshot ?: return null
        val cluster =
            oldSnapshot.clusters.firstOrNull {
                it.documentByteStart == rebaseState.clusterByteStart &&
                    it.documentByteEndExclusive == rebaseState.clusterByteEndExclusive
            } ?: return null
        // #639 评论 5424613367 问题1：caret 几何来自旧 snapshot 的 visualRectInDocument，
        // 当前 destination 是 currentRect（旧 Insert 可能正在 from/destination 中间）。
        // 用 TextRevealGeometry.computeClusterRevealClipRect 统一平移，与未匹配 Insert
        // continuation 共用同一份平移公式。
        return TextRevealGeometry.computeClusterRevealClipRect(
            cluster.visualRectInDocument,
            cluster.caretStartX,
            cluster.caretEndX,
            destination,
            mode,
            fraction,
        )
    }
}
