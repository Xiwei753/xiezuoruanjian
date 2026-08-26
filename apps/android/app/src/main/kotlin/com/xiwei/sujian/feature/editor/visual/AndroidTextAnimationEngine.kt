package com.xiwei.sujian.feature.editor.visual

import com.xiwei.sujian.feature.editor.layout.AffectedLayoutRevision
import com.xiwei.sujian.feature.editor.layout.AndroidLayoutEngine
import com.xiwei.sujian.feature.editor.layout.AndroidLineSnapshot
import com.xiwei.sujian.feature.editor.projection.VisualIntent

/**
 * #638 评论 5403756824：本帧视觉事务状态 — 只读入口。
 *
 * 用当前主 timeline 的同一个 frameTimeMs 计算 [progress]，供 viewport 跟整笔视觉事务走。
 * 不让 View 自己猜动画时间，也不只暴露 cursor progress。
 */
data class VisualFrameClockState(
    val transactionId: Long,
    val progress: Float,
)

/**
 * Unified owner of the Android text animation runtime.
 *
 * Holds [AndroidVisualPlanner], [AnimationTimeline], [VisualResourceStore], and the current
 * [PreparedVisualTransaction] with its rebase/continuation state. All animation cancellation,
 * continuation, session switching, and resource release must go through this object — callers
 * must not directly touch [VisualResourceStore] or [AnimationTimeline].
 *
 * Lifecycle: [cancel] is the only legal way to abort an active transaction (releases its
 * snapshots and resets the timeline). [resetForSession] cancels + releases *all* session-owned
 * resources (for session rebind). [release] is equivalent to [resetForSession] and is called
 * when the host is permanently destroyed.
 */
class AndroidTextAnimationEngine(
    private val visualPlanner: AndroidVisualPlanner,
    private val resourceStore: VisualResourceStore,
    private val timeSource: AnimationTimeSource = ChoreographerAnimationTimeSource(),
    private val transactionIdSource: TransactionIdSource = TransactionIdSource(),
) {
    private var activeTransaction: PreparedVisualTransaction? = null
    private var timeline: AnimationTimeline? = null
    private var cursorTimeline: AnimationTimeline? = null
    private var animationPolicy: TextAnimationPolicy = TextAnimationPolicy.INHERIT_GLOBAL
    private var smoothCursorEnabled: Boolean = true
    private var smoothCursorDurationMs: Long = 80L
    private var coordinatedEnabled: Boolean = true
    private var reduceMotion: Boolean = false

    /**
     * 平滑光标设置（生产路径：设置页 → Editor Host → 输入事务 → 本引擎）。
     *
     * 关闭时当前事务的光标过渡立即降级为静态（shouldAnimate=false）；
     * 开启时光标使用独立的 [cursorTimeline]，时长由 [smoothCursorDurationMs] 控制
     * （不超过文本事务时长，保证光标与文字同时到达终点）。
     */
    fun setSmoothCursor(
        enabled: Boolean,
        durationMs: Long,
    ) {
        smoothCursorEnabled = enabled
        smoothCursorDurationMs = durationMs
        if (!enabled) {
            cursorTimeline = null
        }
    }

    /**
     * #595 三/四：协同动画设置 — 控制文字和光标是否使用同一视觉事务。
     *
     * - coordinated=true：光标时长 = min(cursorDurationMs, textDurationMs)，
     *   文字和光标同一首帧、同一 rebase snapshot、光标先完成后停在终点。
     * - coordinated=false：光标可使用独立时长（不受文字时长限制），
     *   但仍由同一个 View、同一个 renderer、同一个 VSync 时间源驱动。
     */
    fun setCoordinatedAnimationEnabled(enabled: Boolean) {
        coordinatedEnabled = enabled
    }

    fun isCoordinatedAnimationEnabled(): Boolean = coordinatedEnabled

    /**
     * #595 三：reduce-motion 设置 — 降级所有动画为静态更新。
     */
    fun setReduceMotion(enabled: Boolean) {
        reduceMotion = enabled
        if (enabled) {
            animationPolicy = TextAnimationPolicy.SYSTEM_SUPPRESSED
            cursorTimeline = null
        }
    }

    fun isReduceMotion(): Boolean = reduceMotion

    /**
     * #595 六：暂停动画 — 临时失焦时保存当前可见帧，不永久取消事务。
     * 窗口重新获得焦点时从保存帧继续或稳定落到事务终态。
     */
    fun pause(frameTimeMs: Long) {
        timeline?.pause(frameTimeMs)
        cursorTimeline?.pause(frameTimeMs)
    }

    /**
     * #595 六：恢复动画 — 窗口重新获得焦点时从暂停帧继续。
     */
    fun resume(frameTimeMs: Long) {
        timeline?.resume(frameTimeMs)
        cursorTimeline?.resume(frameTimeMs)
    }

    fun isPaused(): Boolean = timeline?.isPaused() == true

    /**
     * Create a prepared visual transaction from old/new layout revisions and line snapshots.
     *
     * Invariant: [transactionKey] is generated exactly once here and passed to the planner.
     * All snapshots registered in this call are owned by [OwnedByTransaction(transactionKey)];
     * the planner must use the same key in [PreparedVisualTransaction.transactionId] so that
     * [completeTransaction]/[cancelTransaction] can release resources under the correct owner.
     *
     * Two-phase ownership: this method optimistically registers ALL old/new snapshots into
     * [ownedSnapshotIds]. [submit] (Phase 2) trims this set to only the snapshots actually
     * referenced by slices or static patches, releasing unreferenced ones immediately.
     */
    fun prepare(
        visualIntent: VisualIntent,
        oldRevision: AffectedLayoutRevision?,
        newRevision: AffectedLayoutRevision?,
        oldSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        newSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        rebaseFrame: VisualFrameSnapshot? = null,
    ): PreparedVisualTransaction {
        val transactionKey = transactionIdSource.nextId()
        val owner = SnapshotOwner.OwnedByTransaction(transactionKey)
        val ownedSnapshotIds = mutableSetOf<Long>()
        val snapshotLookup = mutableMapOf<Long, AndroidLineSnapshot>()
        for ((_, snapshot) in oldSnapshots) {
            resourceStore.put(snapshot, owner)
            ownedSnapshotIds.add(snapshot.snapshotId)
            snapshotLookup[snapshot.snapshotId] = snapshot
        }
        for ((_, snapshot) in newSnapshots) {
            resourceStore.put(snapshot, owner)
            ownedSnapshotIds.add(snapshot.snapshotId)
            snapshotLookup[snapshot.snapshotId] = snapshot
        }
        if (rebaseFrame != null) {
            // Populate snapshotLookup from the rebase frame's surviving slices. These
            // snapshots belong to the OLD transaction and may not be in oldSnapshots/
            // newSnapshots (which were captured fresh for this transaction). Without this
            // step, surviving slices (e.g. a still-fading Delete) would reference snapshot
            // IDs that the planner can't look up, producing null-bitmap slices that the
            // renderer silently skips — losing the old animation's visual state.
            //
            // If a snapshot was already released (e.g. old transaction completed before
            // this prepare call), resourceStore.get returns null and the entry is skipped —
            // the planner's applyRebaseToSlices will produce a surviving slice with a null
            // snapshot, which the renderer safely ignores (drawAnimatedSlices skips
            // null-bitmap slices).
            for (state in rebaseFrame.sliceVisualStates) {
                val snapshot = resourceStore.get(state.snapshotId)
                if (snapshot != null) {
                    snapshotLookup[state.snapshotId] = snapshot
                }
            }
        }
        return visualPlanner.prepare(
            visualIntent, oldRevision, newRevision,
            oldSnapshots, newSnapshots, rebaseFrame, transactionKey, ownedSnapshotIds,
            snapshotLookup,
        )
    }

    /**
     * Submit a prepared animation as the new active transaction.
     *
     * Ownership invariant: after this method, [activeTransaction.ownedSnapshotIds] is the
     * *precise* set of snapshot IDs whose Bitmaps this transaction must release on
     * complete/cancel — it contains only (a) newly captured snapshots that are actually
     * referenced by slices or static patches, and (b) old-transaction snapshots whose
     * ownership was transferred because the new transaction references them.
     *
     * Unreferenced snapshots (newly captured but unused, or old snapshots no longer needed)
     * are released immediately, preventing unbounded growth during rapid consecutive input.
     */
    fun submit(
        preparedAnimation: PreparedVisualTransaction,
        submittedAtMs: Long = timeSource.nowNanos() / 1_000_000,
    ) {
        val effectiveCursorTransition =
            if (!smoothCursorEnabled) {
                preparedAnimation.cursorTransition?.copy(shouldAnimate = false)
            } else {
                preparedAnimation.cursorTransition
            }
        val effectiveTransaction =
            if (effectiveCursorTransition !== preparedAnimation.cursorTransition) {
                preparedAnimation.copy(cursorTransition = effectiveCursorTransition)
            } else {
                preparedAnimation
            }
        // #637 评论 5386301277 项1：hasTextMotion 只计算一次，事务级 coordinated
        // 和是否创建独立 cursorTimeline 共用同一判断，不在不同分支重复计算。
        val hasTextMotion =
            preparedAnimation.animatedSlices.isNotEmpty() ||
                preparedAnimation.blockShifts.isNotEmpty()
        val oldTransaction = activeTransaction
        val newOwner = SnapshotOwner.OwnedByTransaction(preparedAnimation.transactionId)

        val unreferencedNewIds = preparedAnimation.ownedSnapshotIds - preparedAnimation.referencedSnapshotIds
        for (snapshotId in unreferencedNewIds) {
            resourceStore.release(snapshotId, newOwner)
        }

        if (oldTransaction != null) {
            val oldOwner = SnapshotOwner.OwnedByTransaction(oldTransaction.transactionId)

            // Inherit old-transaction snapshots that the new transaction actually references.
            // These were captured by the old transaction but are still needed by the new one
            // (e.g. a surviving Delete slice that continues fading out an old snapshot).
            // Ownership is transferred so they survive the old transaction's release.
            // Only snapshots in referencedSnapshotIds are transferred — unreferenced old
            // snapshots are released below, preventing unbounded accumulation during rapid input.
            //
            // .minus(preparedAnimation.ownedSnapshotIds) excludes snapshots that the new
            // transaction already captured itself — these are already owned by the new
            // transactionKey and must not be "transferred" (which would change their owner
            // from the correct new key to the same new key, a no-op that would also
            // incorrectly include them in inheritedIds, inflating preciseOwnedIds).
            // This exclusion is necessary because preparedAnimation.ownedSnapshotIds contains
            // ALL newly captured snapshots (referenced and unreferenced), while
            // referencedSnapshotIds contains only those actually used. Without .minus, a
            // newly captured but unreferenced snapshot that also happens to be in the old
            // transaction's ownedSnapshotIds would be double-counted: once in the new
            // transaction's ownedSnapshotIds and once in inheritedIds, leading to an
            // inflated preciseOwnedIds and a failed release (owner mismatch) when the new
            // transaction completes.
            val referencedIds = preparedAnimation.referencedSnapshotIds
            val inheritedIds =
                oldTransaction.ownedSnapshotIds
                    .intersect(referencedIds)
                    .minus(preparedAnimation.ownedSnapshotIds)
            for (snapshotId in inheritedIds) {
                resourceStore.transferOwnership(snapshotId, newOwner)
            }

            // Snapshots from the old transaction that are no longer referenced: release now.
            val unreferencedOldIds =
                oldTransaction.ownedSnapshotIds - referencedIds - preparedAnimation.ownedSnapshotIds
            for (snapshotId in unreferencedOldIds) {
                resourceStore.release(snapshotId, oldOwner)
            }

            // preciseOwnedIds = (newly captured & referenced) + (inherited from old transaction).
            // Subtracting unreferencedNewIds prevents Bitmaps from the old transaction that
            // the new transaction never references from accumulating — without this, rapid
            // consecutive inputs would grow ownedSnapshotIds unboundedly until the final
            // transaction completes.
            val preciseOwnedIds = (preparedAnimation.ownedSnapshotIds - unreferencedNewIds) + inheritedIds
            activeTransaction =
                effectiveTransaction.copy(
                    ownedSnapshotIds = preciseOwnedIds,
                    coordinated = coordinatedEnabled && hasTextMotion,
                )
            timeline?.complete()
            cursorTimeline?.complete()
            // 连续输入重基：新事务在旧事务完成前接管，旧快照所有权转移给新事务。
            com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.animationRebase(
                oldTransaction.transactionId,
                preparedAnimation.transactionId,
            )
            val deleteSlices = preparedAnimation.animatedSlices.count { it.role == SliceRole.Delete }
            // #638 评论 5395990973：minSliceRemaining/maxSliceRemaining 字段语义是
            // slice remainingFraction 范围，只从 animatedSlices 计算；cursor 已有独立
            // cursorRemaining 字段。用 remainingFractionAt(0f) 代替 .end — 字段语义是
            // remainingFraction，数据结构允许非零 start，当前 continuation 恰好 start=0
            // 时两者相同但不应依赖此巧合。
            val sliceRemaining =
                preparedAnimation.animatedSlices.map {
                    it.progressWindow.remainingFractionAt(0f)
                }
            val cursorRemaining =
                preparedAnimation.cursorTransition
                    ?.takeIf { it.shouldAnimate }
                    ?.progressWindow
                    ?.remainingFractionAt(0f)
                    ?: 0f
            val minSliceRemaining = if (sliceRemaining.isNotEmpty()) sliceRemaining.min() else 0f
            val maxSliceRemaining = if (sliceRemaining.isNotEmpty()) sliceRemaining.max() else 0f
            com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.animationRebaseState(
                oldTransaction.transactionId,
                preparedAnimation.transactionId,
                deleteSlices,
                cursorRemaining,
                minSliceRemaining,
                maxSliceRemaining,
            )
        } else {
            val preciseOwnedIds = preparedAnimation.ownedSnapshotIds - unreferencedNewIds
            activeTransaction =
                effectiveTransaction.copy(
                    ownedSnapshotIds = preciseOwnedIds,
                    coordinated = coordinatedEnabled && hasTextMotion,
                )
        }

        timeline = AnimationTimeline(preparedAnimation.durationMs, submittedAtMs)
        cursorTimeline =
            if (effectiveCursorTransition?.shouldAnimate == true && preparedAnimation.durationMs > 0L) {
                if (coordinatedEnabled && hasTextMotion) {
                    null // 协同模式：光标跟随主 timeline，不创建独立 cursorTimeline
                } else {
                    val cursorDuration = smoothCursorDurationMs.coerceAtLeast(1L)
                    AnimationTimeline(cursorDuration, submittedAtMs)
                }
            } else {
                null
            }
        com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.animationStart(
            preparedAnimation.transactionId,
            preparedAnimation.operationKind.name,
            preparedAnimation.durationMs,
        )
    }

    /**
     * Capture old layout → apply mirror update → capture new layout → prepare → submit.
     *
     * Ordering invariant: old snapshots must be captured *before* the mirror update,
     * and new snapshots *after*, so that old/new represent the exact before/after states.
     * [beforePatch] runs between old capture and mirror update (e.g. to hide static text
     * that would otherwise be visible alongside the old-snapshot animation during the
     * brief window between mirror update and first animation frame).
     *
     * Timestamp 分工（#623 评论 1 + 评论 4）：
     * - rebase snapshot 使用最近一次真实 VSync 帧时间 [AnimationTimeSource.lastFrameTimeNanos]：
     *   当 [frameTimeMs] 显式传入时优先用它（来自 Choreographer 帧回调），否则取
     *   [lastFrameTimeNanos]（未收到过帧时回退到当下 monotonic now）。rebase 只读取旧事务
     *   当前视觉状态，用帧时间是正确的。
     * - 事务提交时间 (submittedAtMs) **不在函数开头取**，而是在完成 old/new snapshot、layout、
     *   planner、prepare 以后真正调用 [submit] 时由其默认参数执行 [AnimationTimeSource.nowNanos]
     *   （生产实现取 [System.nanoTime]）。这是"现在"语义，不返回缓存帧。若在函数开头就取，
     *   准备工作耗时几十到几百毫秒后动画真正创建 [AnimationTimeline] 时起点已在过去，
     *   100ms 动画第一帧可能直接接近/到达终点（#623 评论 4）。
     * - 不要把准备耗时补偿进 [AnimationTimeline]，也不要改 duration；准备阶段不是动画
     *   已经播放的时间。
     * Sub-millisecond precision is intentionally discarded —
     * [AnimationTimeline.progress] operates in whole milliseconds.
     * [System.nanoTime] is monotonic (unlike [System.currentTimeMillis], which can jump
     * backwards on NTP adjustments).
     */
    fun prepareAndSubmit(
        visualIntent: VisualIntent,
        layoutEngine: AndroidLayoutEngine,
        mirrorUpdate: (() -> Unit)? = null,
        beforePatch: (() -> Unit)? = null,
        frameTimeMs: Long? = null,
    ) {
        val textSuppressed = animationPolicy == TextAnimationPolicy.SYSTEM_SUPPRESSED || reduceMotion
        val rebaseFrameTimeMs = resolveRebaseFrameTime(frameTimeMs)

        if (textSuppressed && !smoothCursorEnabled) {
            applyStaticUpdate(visualIntent, layoutEngine, mirrorUpdate, beforePatch)
            return
        }

        if (textSuppressed && smoothCursorEnabled) {
            // #595 四: CursorOnly transaction — text static, cursor animates via the
            // same FrameClock. Text slices are suppressed (SYSTEM_SUPPRESSED mode → no
            // slices from the planner), but the cursor transition is preserved so the
            // cursor timeline drives smooth cursor movement from the same VSync source.
            submitCursorOnlyTransaction(visualIntent, layoutEngine, mirrorUpdate, beforePatch)
            return
        }
        val rebaseSnapshot = captureRebaseSnapshot(rebaseFrameTimeMs)

        // #624 评论3：普通编辑只抓受影响区域 — 编辑前（old 侧）用当前 DynamicLayout
        // 的 getLineForOffset() 找到编辑所在段落，只保存该段落及删除/合段时相邻
        // 段落的 old line geometry；mirror 更新后只读取新的受影响段落。
        val oldRevision = captureOldAffected(visualIntent, layoutEngine)
        // 旧受影响行的 snapshot 必须在 mirror 更新前抓取（布局还是旧正文）。
        val oldSnapshots = captureSnapshots(oldRevision, layoutEngine)
        beforePatch?.invoke()
        mirrorUpdate?.invoke()
        // 布局推进所有权唯一入口：mirror 更新后只推进一次（配置没变则复用
        // 现有 DynamicLayout，revision 由 captureAffectedRevision 产生）。
        layoutEngine.ensureLayoutConfig()
        val newRevision = captureNewAffected(visualIntent, layoutEngine, oldRevision)
        val newSnapshots = captureSnapshots(newRevision, layoutEngine)
        val transaction = prepare(visualIntent, oldRevision, newRevision, oldSnapshots, newSnapshots, rebaseSnapshot)
        submit(transaction)
    }

    /** 静态更新路径（文字+光标都抑制）：mirror 更新后只推进一次布局配置，
     *  捕获 new 侧受影响区域仅为渲染路径提供新的光标/选区几何。 */
    private fun applyStaticUpdate(
        visualIntent: VisualIntent,
        layoutEngine: AndroidLayoutEngine,
        mirrorUpdate: (() -> Unit)?,
        beforePatch: (() -> Unit)?,
    ) {
        beforePatch?.invoke()
        mirrorUpdate?.invoke()
        layoutEngine.ensureLayoutConfig()
        layoutEngine.captureAffectedRevision(
            visualIntent.newAffectedByteRanges.firstOrNull()?.first
                ?: layoutEngine.getMirror().getCursorUtf8(),
            visualIntent.newAffectedByteRanges.lastOrNull()?.second
                ?: layoutEngine.getMirror().getCursorUtf8(),
            includeNextParagraph = false,
        )
    }

    private fun resolveRebaseFrameTime(frameTimeMs: Long?): Long =
        frameTimeMs
            ?: timeSource.lastFrameTimeNanos()?.let { it / 1_000_000 }
            ?: timeSource.nowNanos() / 1_000_000

    private fun captureSnapshots(
        revision: AffectedLayoutRevision?,
        layoutEngine: AndroidLayoutEngine,
    ): Map<Int, AndroidLineSnapshot> =
        revision?.let { layoutEngine.captureLineBitmapSnapshotsWithClusters(it) } ?: emptyMap()

    /** 编辑前捕获 old 侧受影响区域（old 坐标，mirror 尚未更新）。 */
    private fun captureOldAffected(
        visualIntent: VisualIntent,
        layoutEngine: AndroidLayoutEngine,
    ): AffectedLayoutRevision? {
        val start =
            visualIntent.oldAffectedByteRanges.firstOrNull()?.first
                ?: layoutEngine.getMirror().getCursorUtf8()
        val end =
            visualIntent.oldAffectedByteRanges.lastOrNull()?.second
                ?: layoutEngine.getMirror().getCursorUtf8()
        return layoutEngine.captureAffectedRevision(
            start,
            end,
            includeNextParagraph = visualIntent.isDeleteOrReplaceRenderRole(),
        )
    }

    /** 编辑后捕获 new 侧受影响区域（new 坐标，mirror 已更新），并计算稳定后缀 deltaY。 */
    private fun captureNewAffected(
        visualIntent: VisualIntent,
        layoutEngine: AndroidLayoutEngine,
        oldRevision: AffectedLayoutRevision?,
    ): AffectedLayoutRevision? {
        val start =
            visualIntent.newAffectedByteRanges.firstOrNull()?.first
                ?: layoutEngine.getMirror().getCursorUtf8()
        val end =
            visualIntent.newAffectedByteRanges.lastOrNull()?.second
                ?: layoutEngine.getMirror().getCursorUtf8()
        return layoutEngine.captureAffectedRevision(
            start,
            end,
            includeNextParagraph = false,
            previousRevision = oldRevision,
        )
    }

    /**
     * #595 四: CursorOnly transaction — 文字静态更新，光标由同一 FrameClock 平滑移动。
     *
     * 当 textEnabled=false 但 cursorEnabled=true 时调用。mirror/layout 先静态更新
     * （文字立即落到新状态），然后用 SYSTEM_SUPPRESSED 动画模式构造一个无文字切片
     * 但保留 cursorTransition 的事务，submit 创建 cursorTimeline 驱动光标平滑移动。
     * 事务时长设为光标时长，保证文字时间线不会先于光标完成而提前停止帧请求。
     */
    private fun submitCursorOnlyTransaction(
        visualIntent: VisualIntent,
        layoutEngine: AndroidLayoutEngine,
        mirrorUpdate: (() -> Unit)?,
        beforePatch: (() -> Unit)?,
    ) {
        val rebaseSnapshot = captureRebaseSnapshot(resolveRebaseFrameTime(null))
        val oldRevision = captureOldAffected(visualIntent, layoutEngine)
        beforePatch?.invoke()
        mirrorUpdate?.invoke()
        layoutEngine.ensureLayoutConfig()
        val newRevision = captureNewAffected(visualIntent, layoutEngine, oldRevision)
        val cursorDuration = smoothCursorDurationMs.coerceAtLeast(1L)
        val cursorOnlyIntent =
            visualIntent.copy(
                animationMode = uniffi.writer_core.AnimationModeDto.SYSTEM_SUPPRESSED,
                durationMs = cursorDuration,
            )
        val transaction = prepare(cursorOnlyIntent, oldRevision, newRevision, emptyMap(), emptyMap(), rebaseSnapshot)
        submit(transaction)
    }

    fun registerSnapshots(
        snapshots: Map<Int, AndroidLineSnapshot>,
        owner: SnapshotOwner,
    ) {
        for ((_, snapshot) in snapshots) {
            resourceStore.put(snapshot, owner)
        }
    }

    /**
     * Capture the current visual state of the active transaction for rendering or rebase.
     *
     * Returns null when no transaction is active or the timeline is null.
     *
     * Supports three timeline states:
     * - [TransactionState.Rendering] / [TransactionState.Paused]: normal frame capture at
     *   the current progress, with interpolated slice positions/alphas and cursor rect.
     * - [TransactionState.Pending]: the transaction was just submitted but has not yet
     *   been drawn on screen. Returns a frame at progress=0f with all slices at their
     *   start positions/alphas and the cursor at its start rect. This is essential for
     *   rebase during rapid consecutive input — if a second edit arrives before the first
     *   transaction's first onDraw, the rebase snapshot must capture the first transaction's
     *   initial visual state rather than returning null (which would lose the first animation).
     *
     * Returns null for Completed/Cancelled states — these are terminal and produce no frames.
     */
    fun captureFrame(frameTimeMs: Long): VisualFrameSnapshot? {
        val transaction = activeTransaction ?: return null
        val tl = timeline ?: return null
        val state = tl.getState()
        if (state == TransactionState.Completed || state == TransactionState.Cancelled) return null
        if (state == TransactionState.Pending) {
            val sliceStates = computeSliceVisualStates(transaction, 0f)
            val cursorRect = computeCurrentCursorRect(transaction, 0f)
            val blockStates = computeBlockShiftVisualStates(transaction, 0f)
            // #637 评论 5389230907：Pending 分支不能写死 1f。若本事务本身就是上一次
            // rebase 的 continuation（cursor window 已经是 [0, 0.4]），在第一帧真正
            // 画出来之前再次 rebase 时，必须仍剩 0.4，不能恢复成 Full。slice 和
            // BlockShift 在 Pending 分支已通过 compute...(transaction, 0f) 保留自己的
            // window，cursor 也用 remainingFractionAt(0f) 保持一致。
            val cursorRemainingFraction =
                transaction.cursorTransition?.progressWindow?.remainingFractionAt(0f) ?: 1f
            return VisualFrameSnapshot(
                progress = 0f,
                state = TransactionState.Pending,
                sliceVisualStates = sliceStates,
                cursorRect = cursorRect,
                cursorRemainingFraction = cursorRemainingFraction,
                blockShiftStates = blockStates,
            )
        }
        val p = tl.progress(frameTimeMs)
        val cursorProgress = getCursorProgress(frameTimeMs) ?: p
        val sliceStates = computeSliceVisualStates(transaction, p)
        val cursorRect = computeCurrentCursorRect(transaction, cursorProgress)
        val blockStates = computeBlockShiftVisualStates(transaction, p)
        // #637 评论 5386573878：保存光标在当前帧之后还剩多少基准时长，
        // 供 rebase continuation 用 fromRemainingFraction 直接消费，
        // 连续 rebase 不会反复减速。
        val cursorRemainingFraction =
            transaction.cursorTransition?.progressWindow?.remainingFractionAt(cursorProgress) ?: 1f
        return VisualFrameSnapshot(
            progress = p,
            state = state,
            sliceVisualStates = sliceStates,
            cursorRect = cursorRect,
            cursorRemainingFraction = cursorRemainingFraction,
            blockShiftStates = blockStates,
        )
    }

    /** Capture the rebase snapshot for the next transaction. Delegates to [captureFrame],
     *  which includes the current cursor rect — essential for continuous cursor animation
     *  across rapid consecutive inputs. */
    fun captureRebaseSnapshot(frameTimeMs: Long): VisualFrameSnapshot? {
        return captureFrame(frameTimeMs)
    }

    fun hasActiveAnimation(): Boolean {
        if (activeTransaction == null) return false
        val tl = timeline
        if (tl != null && tl.getState() != TransactionState.Completed) return true
        // #637 评论 5386066978 项3：用事务级 coordinated 标记，不靠引擎级设置反推。
        val tx = activeTransaction
        if (tx != null && tx.coordinated) return false
        val ctl = cursorTimeline
        if (ctl != null && ctl.getState() != TransactionState.Completed) return true
        return false
    }

    fun currentTimeNanos(): Long = timeSource.nowNanos()

    fun cancel() {
        val transaction = activeTransaction ?: return
        com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.animationCancel(transaction.transactionId)
        cancelTransaction(transaction)
        activeTransaction = null
        timeline = null
        cursorTimeline = null
    }

    /** Cancel the active transaction and release ALL resources in [resourceStore] (not just
     *  the active transaction's snapshots). Used for session rebind — [cancel] releases only
     *  the active transaction's [ownedSnapshotIds], but the store may contain snapshots from
     *  completed transactions that were not yet garbage-collected. [resetForSession] ensures
     *  no Bitmaps survive across session boundaries. */
    fun resetForSession() {
        cancel()
        resourceStore.releaseAll()
    }

    fun release() {
        cancel()
        resourceStore.releaseAll()
    }

    fun setAnimationPolicy(policy: TextAnimationPolicy) {
        animationPolicy = policy
        com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.animationPolicy(policy.name)
    }

    fun getAnimationPolicy(): TextAnimationPolicy = animationPolicy

    fun getActiveTransaction(): PreparedVisualTransaction? = activeTransaction

    fun getTimelineProgress(frameTimeMs: Long): Float {
        val tl = timeline ?: return 0f
        return tl.progress(frameTimeMs)
    }

    /**
     * 独立光标时间线进度（生产渲染路径使用）。
     *
     * 与 [captureFrame] 中的光标进度完全一致：平滑光标开启时，屏幕上的光标
     * 由 [cursorTimeline]（时长 = min(smoothCursorDurationMs, 文本事务时长)）驱动；
     * 光标时间线先于文本完成时定格在终点（1f）；关闭（无时间线）时返回 null，
     * 由调用方回退到文本进度/静态光标。
     */
    fun getCursorTimelineProgress(frameTimeMs: Long): Float? {
        val tl = cursorTimeline ?: return null
        return when (tl.getState()) {
            TransactionState.Completed -> 1f
            TransactionState.Cancelled -> null
            else -> tl.progress(frameTimeMs)
        }
    }

    /**
     * 统一光标进度入口 — 协同模式下光标跟随主 timeline，非协同模式使用独立 cursorTimeline。
     *
     * 协同模式 + 有文字视觉事务：cursorProgress = timeline.progress(frameTimeMs)
     * 非协同模式 或 无文字事务：cursorProgress = cursorTimeline?.progress(frameTimeMs)
     * CURSOR_ONLY：cursorTimeline 独立存在
     */
    fun getCursorProgress(frameTimeMs: Long): Float? {
        val tx = activeTransaction ?: return null
        // #637 评论 5386066978 项3：用事务级 coordinated 标记。
        return if (tx.coordinated) {
            timeline?.progress(frameTimeMs)
        } else {
            getCursorTimelineProgress(frameTimeMs)
        }
    }

    fun isTimelineCompleted(frameTimeMs: Long): Boolean {
        val tl = timeline ?: return true
        return tl.isCompleted(frameTimeMs)
    }

    /**
     * #595 五：光标轨是否已完成 — 光标 timeline 不存在或已结束。
     * 非协同模式下光标时长可长于文字时长，文字完成后光标仍可继续。
     */
    fun isCursorTimelineCompleted(frameTimeMs: Long): Boolean {
        val tx = activeTransaction
        if (tx != null && tx.coordinated) {
            // #637 评论 5386066978 项3：协同模式光标跟随主 timeline，
            // textFinished == cursorFinished，不会出现文字已静态、光标仍动画。
            return isTimelineCompleted(frameTimeMs)
        }
        val ctl = cursorTimeline ?: return true
        return ctl.isCompleted(frameTimeMs)
    }

    /**
     * #595 五：文字轨是否已完成 — 文字 timeline 不存在或已结束。
     */
    fun isTextTimelineCompleted(frameTimeMs: Long): Boolean = isTimelineCompleted(frameTimeMs)

    /**
     * #595 五：整个视觉事务是否完成 — 文字轨和光标轨都结束。
     */
    fun isTransactionCompleted(frameTimeMs: Long): Boolean {
        return isTextTimelineCompleted(frameTimeMs) && isCursorTimelineCompleted(frameTimeMs)
    }

    /**
     * #638：获取当前帧的视觉光标 Rect。
     *
     * 复用 [CursorTransition.rectAt] 的几何计算，与 renderer 完全一致。
     * 无活跃事务、光标未动画、光标轨已完成时返回 null（表示应使用静态光标）。
     */
    fun currentVisualCursorRect(frameTimeMs: Long): android.graphics.RectF? {
        val transaction = activeTransaction ?: return null
        val ct = transaction.cursorTransition ?: return null
        if (!ct.shouldAnimate) return null
        val cursorProgress = getCursorProgress(frameTimeMs) ?: return null
        return ct.rectAt(cursorProgress)
    }

    /**
     * #638 评论 5403756824：本帧视觉事务状态 — 用当前主 timeline 的同一个 frameTimeMs
     * 计算 progress，供 viewport 跟整笔视觉事务走。
     *
     * 无活跃事务、主 timeline 不存在或已完成时返回 null（表示应走静态分支）。
     * 用主 timeline progress（协同模式下光标跟随主 timeline，非协同模式 viewport
     * 仍按主事务整体进度过渡，光标独立轨由 currentVisualCursorRect 单独处理）。
     */
    fun currentVisualFrameClockState(frameTimeMs: Long): VisualFrameClockState? {
        val transaction = activeTransaction ?: return null
        val tl = timeline ?: return null
        if (tl.getState() == TransactionState.Completed) return null
        val progress = tl.progress(frameTimeMs)
        return VisualFrameClockState(transaction.transactionId, progress)
    }

    /**
     * Transition the timeline from Pending to Rendering on the first onDraw after submit.
     *
     * Must be called from the host's draw path (e.g. View.onDraw / Compose draw callback)
     * so that [AnimationTimeline.progress] uses a real frame timestamp rather than the
     * submission time. Without this, [captureFrame] returns a Pending-state frame at
     * progress=0f, which is correct for rebase but would never advance the animation.
     *
     * Idempotent: subsequent calls after the first are no-ops — the timeline stays in
     * Rendering until paused, completed, or cancelled.
     */
    fun markFirstVisibleFrame(frameTimeMs: Long) {
        timeline?.markFirstVisibleFrame(frameTimeMs)
        cursorTimeline?.markFirstVisibleFrame(frameTimeMs)
    }

    fun completeIfFinished(frameTimeMs: Long): Boolean {
        val transaction = activeTransaction ?: return false
        val tl = timeline ?: return false
        if (tl.getState() == TransactionState.Completed) return false
        // #595 五：只有文字轨和光标轨都完成才结束整个事务。
        // 文字完成后光标仍可继续（非协同模式光标时长 > 文字时长）。
        // 文字完成后释放文字切片 Bitmap，但保留光标过渡所需的不可变几何和事务标识。
        val textFinished = tl.isCompleted(frameTimeMs)
        val cursorFinished = isCursorTimelineCompleted(frameTimeMs)
        if (textFinished && cursorFinished) {
            com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.animationComplete(
                transaction.transactionId,
                (frameTimeMs - (tl.getFirstVisibleFrameTimeMs() ?: frameTimeMs)).coerceAtLeast(0L),
            )
            completeTransaction(transaction)
            // #595 六：统一终态 — 事务对象与两条 timeline 全部离开引擎。
            // 旧实现只把 activeTransaction 换成 ownedSnapshotIds=emptySet() 的副本，
            // 事务对象仍留在引擎中：下一次 submit 会误发 rebase 事件，且引擎无法
            // 表达 Completed/Idle 统一终态。完成后 activeTransaction==null，
            // hasActiveAnimation()==false，FrameClock 停止 repost。
            activeTransaction = null
            timeline = null
            cursorTimeline = null
            return true
        }
        return false
    }

    /**
     * Interpolate each animated slice's position and alpha from its start/end values at [progress].
     * For Move slices, [fromDestinationRect] is the pre-move position; for other roles it falls
     * back to [destinationRect] (alpha-only animation).
     *
     * Exception after rebase: Insert/CrossfadeNew slices that were rebased onto a Move slice
     * inherit [fromDestinationRect] from the Move's current position (see [applyRebaseState]).
     * In this case the slice animates both position and alpha — it slides from the old Move's
     * current position to its own destination while fading in.
     */
    private fun computeSliceVisualStates(
        transaction: PreparedVisualTransaction,
        progress: Float,
    ): List<SliceVisualState> {
        return transaction.animatedSlices.map { slice ->
            // #637 评论 5386066978 项2：localProgress 由 progressWindow.map 得到，
            // 位置/alpha/reveal 都用 localProgress 插值，与 renderer 一致。
            val localProgress = slice.progressWindow.map(progress)
            // #637 评论 5386573878：保存当前帧之后还剩多少基准时长，
            // rebase continuation 用 fromRemainingFraction 直接消费。
            val remainingFraction = slice.progressWindow.remainingFractionAt(progress)
            // #639 评论 5422606865 问题2：currentRect 统一走 visualDestinationRectAt，
            // 与 renderer 共用同一份几何，captureFrame 记录的位置就是 renderer 真正
            // 画在屏幕上的位置。fromDestinationRect 非 null 时做位置插值，否则返回
            // destinationRect（alpha-only 动画）。
            val currentRect = slice.visualDestinationRectAt(progress)
            val currentLeft = currentRect.left
            val currentTop = currentRect.top
            val currentRight = currentRect.right
            val currentBottom = currentRect.bottom
            val currentAlpha = slice.startAlpha + (slice.endAlpha - slice.startAlpha) * localProgress
            val revealFraction = slice.revealSpec?.fraction(localProgress)
            SliceVisualState(
                snapshotId = slice.snapshot?.snapshotId ?: -1L,
                role = slice.role,
                lineIndex = slice.snapshot?.lineIndex ?: -1,
                documentByteStart = slice.snapshot?.documentByteStart ?: -1,
                documentByteEndExclusive = slice.snapshot?.documentByteEndExclusive ?: -1,
                clusterByteStart = slice.clusterByteStart,
                clusterByteEndExclusive = slice.clusterByteEndExclusive,
                currentLeft = currentLeft,
                currentTop = currentTop,
                currentRight = currentRight,
                currentBottom = currentBottom,
                currentAlpha = currentAlpha,
                destinationLeft = slice.destinationRect.left,
                destinationTop = slice.destinationRect.top,
                destinationRight = slice.destinationRect.right,
                destinationBottom = slice.destinationRect.bottom,
                // #639 评论 5427183226：SliceVisualState 必须是"上一帧实际画出来的
                // 完整 slice 状态"。把 sourceRect/targetAlpha/revealMode/revealFraction/
                // fixedRevealClipRect/caretRevealGeometry 一次性收进来，未映射 continuation
                // 不再退到 snapshot.sourceRect，appearance 轨跨第二次 rebase 不再丢状态。
                sourceRect = android.graphics.Rect(slice.sourceRect),
                targetAlpha = slice.endAlpha,
                revealMode = slice.revealSpec?.mode,
                revealFraction = revealFraction,
                remainingFraction = remainingFraction,
                fixedRevealClipRect = slice.fixedRevealClipRect?.let { android.graphics.RectF(it) },
                caretRevealGeometry = slice.caretRevealGeometry,
                // #639 评论 5427812180 缺陷4/5：staticSuppressionMode 和 fixedClipBaseRect
                // 一次性收进来，mapped/unmapped continuation 继续旧 state 的 suppression mode
                // 和 fixed clip base rect，不因新 role 变了瞬间切换底图 ownership 或丢 clip base。
                staticSuppressionMode = slice.staticSuppressionMode ?: defaultStaticSuppressionModeForRole(slice.role),
                fixedClipBaseRect = slice.fixedClipBaseRect?.let { android.graphics.RectF(it) },
            )
        }
    }

    /**
     * Release all snapshots owned by this transaction and mark BOTH timelines completed.
     *
     * Invariant: [transaction.ownedSnapshotIds] is the *precise* ownership set computed by
     * [submit] — it contains only (a) newly captured snapshots referenced by slices/patches,
     * and (b) old-transaction snapshots inherited via ownership transfer. Unreferenced snapshots
     * were already released during [submit], so this method releases exactly the right set
     * without scanning slices or patches.
     *
     * #595 七：文字轨和光标轨都必须进入终态。只 complete 文字 timeline 而留下
     * cursorTimeline 非 Completed，会让 hasActiveAnimation() 持续返回 true，
     * FrameClock 永久 repost 造成无意义耗电。
     */
    private fun completeTransaction(transaction: PreparedVisualTransaction) {
        val owner = SnapshotOwner.OwnedByTransaction(transaction.transactionId)
        for (snapshotId in transaction.ownedSnapshotIds) {
            resourceStore.release(snapshotId, owner)
        }
        timeline?.complete()
        cursorTimeline?.complete()
    }

    /** Same ownership invariant as [completeTransaction]; both timelines are cancelled instead. */
    private fun cancelTransaction(transaction: PreparedVisualTransaction) {
        val owner = SnapshotOwner.OwnedByTransaction(transaction.transactionId)
        for (snapshotId in transaction.ownedSnapshotIds) {
            resourceStore.release(snapshotId, owner)
        }
        timeline?.cancel()
        cursorTimeline?.cancel()
    }

    /**
     * Interpolate the cursor rectangle from [CursorTransition] at [progress].
     *
     * #637 评论 5389230907：必须与 renderer 使用同一份几何计算 — 调用
     * [CursorTransition.rectAt]，先 [VisualProgressWindow.map] 得到 localProgress
     * 再插值。旧实现直接用全局 progress 插值，在 rebase continuation
     * （progressWindow=[0,0.4]）时与本事务 progress=0.2 对应的 renderer
     * localProgress=0.5 不一致，captureFrame 记成更靠后的位置，下一次 rebase
     * 光标回跳。
     */
    private fun computeCurrentCursorRect(
        transaction: PreparedVisualTransaction,
        progress: Float,
    ): android.graphics.RectF? {
        val ct = transaction.cursorTransition ?: return null
        if (!ct.shouldAnimate) return null
        return ct.rectAt(progress)
    }

    /**
     * Interpolate each BlockShift's vertical translation at [progress].
     *
     * Interpolation: translateY = deltaY * (progress - 1).
     * - progress=0 → translateY = -deltaY (text at its old position, above the new layout).
     * - progress=1 → translateY = 0 (text at the new layout position).
     *
     * Sign convention for deltaY: positive when the block moved downward in the new layout
     * (newTop > oldTop). During animation, currentTranslateY = deltaY * (progress - 1) is
     * negative (text is above its final position, still moving down). At progress=0,
     * currentTranslateY = -deltaY, which is the offset from the new-layout position to the
     * old-layout position. This sign convention is consistent throughout the rebase chain:
     * currentTranslateY < 0 means "text has not yet reached the new position" (animating
     * downward), currentTranslateY > 0 means "text has overshot the new position" (animating
     * upward), currentTranslateY = 0 means "text is at the new-layout position".
     *
     * [targetTranslateY] is always 0 because the animation's final state is the new layout
     * with no translation. The rebase consumer uses [currentTranslateY] to adjust the next
     * transaction's deltaY so that consecutive inputs continue from the on-screen position.
     *
     * Rebase invariant: after [AndroidVisualPlanner.applyRebaseToBlockShifts] adjusts
     * deltaY to (newDeltaY - oldCurrentTranslateY), the formula still produces the correct
     * on-screen position at progress=0: translateY = -(newDeltaY - oldCurrentTranslateY).
     * The old on-screen position = layout_1_Y + currentTranslateY_old. The new layout
     * position = layout_2_Y. Continuity requires layout_2_Y - adjustedDeltaY =
     * layout_1_Y + currentTranslateY_old, so adjustedDeltaY = newDeltaY - currentTranslateY_old.
     * At progress=1 the text reaches the new layout position (translateY = 0) regardless of rebase.
     */
    private fun computeBlockShiftVisualStates(
        transaction: PreparedVisualTransaction,
        progress: Float,
    ): List<BlockShiftVisualState> {
        return transaction.blockShifts.map { shift ->
            // #637 评论 5386066978 项2：localProgress 由 progressWindow.map 得到，
            // currentTranslateY = deltaY * (localProgress - 1)，与 renderer 一致。
            val localProgress = shift.progressWindow.map(progress)
            // #637 评论 5386573878：保存当前帧之后还剩多少基准时长。
            val remainingFraction = shift.progressWindow.remainingFractionAt(progress)
            val currentTranslateY = shift.deltaY * (localProgress - 1f)
            BlockShiftVisualState(
                startLineIndex = shift.startLineIndex,
                endLineIndexExclusive = shift.endLineIndexExclusive,
                startUtf8 = shift.startUtf8,
                endUtf8Exclusive = shift.endUtf8Exclusive,
                currentTranslateY = currentTranslateY,
                targetTranslateY = 0f,
                remainingFraction = remainingFraction,
            )
        }
    }
}

enum class TextAnimationPolicy {
    INHERIT_GLOBAL,
    ENABLED,
    SYSTEM_SUPPRESSED,
}
