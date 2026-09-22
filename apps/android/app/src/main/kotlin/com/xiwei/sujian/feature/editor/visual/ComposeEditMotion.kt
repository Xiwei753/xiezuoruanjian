package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect
import kotlin.math.abs

/**
 * Issue #728 评论 5754045689：统一编辑 motion —
 * 一笔编辑只创建一个 motion，统一保存 old/new caret rect + inserted/deleted glyph units，
 * 用同一只钟驱动 caret 移动和文字吞吐。
 *
 * Issue #728 评论 5755928697：三个确定问题的收口 —
 * 1. 文字动画还没结束时移动光标，会把正在运行的 glyph channel 全丢掉：
 *    新增 [redirectCaretTo]，保留现有 unit channel 的 fraction，只换 caret 目标。
 * 2. ~~coordinated=false 时独立 smooth cursor 设置对正文编辑不生效~~
 *    Issue #735 评论 5773604666 问题2：删除双 duration —
 *    caret 和 glyph 不再各自持有 duration，统一用单一 [durationNanos]。
 *    正文 edit motion（Insert/Delete/Replace/CompositionCommit）一律一只钟，
 *    caret 和 glyph 共用同一个 progress。
 * 3. 一笔里有多个 glyph unit 时所有字同时吐/吞，不是真正跟着光标：
 *    [UnitChannel] 增加 startProgress/endProgress 区间，sample 按区间映射，
 *    光标经过哪个 unit 的区间，那个 unit 才在吞吐。
 *
 * 背景：#725 删除了旧自绘 caret，文字吞吐改由 [ComposeTextRevealTrack] 独立驱动，
 * 系统 caret 由 BasicTextField 自己画。但"文字有自己的钟、caret 由系统画"导致
 * caret 已经跳完/没动画时字还在吐或吞，视觉不协调。
 *
 * #728 决策：**一笔编辑一只钟**。caret 移动和文字吞吐共用同一个 progress：
 * - 插入：caret 从旧位置向新位置移动，文字 reveal 从 0→1（吐字），共用 progress。
 * - 删除：caret 从旧位置向新位置移动（反向），文字 conceal 从 1→0（吞字），共用 progress。
 * - 快速连续输入时，先 sample 上一笔当前帧，再从当前 caret 位置和当前文字可见比例
 *   重定向到新目标；不能把已有文字重新从 0 或 1 开始播。
 *
 * motion 不保存自己的动画进度（progress 由 frameTime 算），也不拥有文字 units
 * （units 由 [ComposeVisualTimeline] 持有）。motion 只保存 caret 两端 + 每个 unit 的
 * from/to fraction 通道 + 区间，sample 时用统一 progress 算 caret 插值和文字 fraction。
 *
 * @param originCaretRect 旧 caret rect（编辑前位置）。
 * @param targetCaretRect 新 caret rect（编辑后位置）。
 * @param unitChannels 每个 unit key → (fromFraction, toFraction, startProgress, endProgress)。
 *   - Inserted unit：from=0, to=1（吐字）。
 *   - Deleted unit：from=1, to=0（吞字）。
 *   redirectTo 时已有 unit 的 from = 当前 fraction，to 不变；新 unit from=0/1, to=1/0。
 *   startProgress/endProgress 是该 unit 在 master glyph progress 里的区间（0..1），
 *   光标经过该区间时该 unit 才在吞吐（Issue #728 评论 5755928697 问题3）。
 * @param startedAtNanos 通道开始时间戳（来自 Compose frame clock），caret 和 glyph 共用。
 * @param durationNanos 共享通道时长。<=0 表示瞬时完成。
 *   Issue #735 评论 5773604666 问题2：caret 和 glyph 共用同一个 duration/progress，
 *   不再有独立的 caretDurationNanos/glyphDurationNanos。
 *   [forSelectionMove] 仍用独立 caret-only motion（纯 selection/cursor move 没有文字吞吐）。
 */
class ComposeEditMotion(
    private val originCaretRect: Rect,
    private val targetCaretRect: Rect,
    private val unitChannels: Map<Long, UnitChannel>,
    private val startedAtNanos: Long,
    private val durationNanos: Long,
) {
    /**
     * 单个 unit 的 fraction 通道 — from→to，由 master glyph progress 经区间映射驱动。
     *
     * @param from 起始 fraction（inserted: 0, deleted: 1, redirect 后: 当前 fraction）。
     * @param to 目标 fraction（inserted: 1, deleted: 0）。
     * @param startProgress 该 unit 在 master glyph progress 里的区间起点（0..1）。
     * @param endProgress 该 unit 在 master glyph progress 里的区间终点（0..1）。
     *   master glyph progress <= startProgress 时 localProgress=0（该 unit 还没开始）；
     *   >= endProgress 时 localProgress=1（该 unit 已走完）；
     *   中间线性映射。Issue #728 评论 5755928697 问题3。
     */
    data class UnitChannel(
        val from: Float,
        val to: Float,
        val startProgress: Float,
        val endProgress: Float,
    )

    /**
     * sample 结果 — 一帧的 caret rect + 每个 unit 的 clip fraction + 是否结束。
     *
     * @param caretRect 当前帧的 caret rect（由 origin→target 插值）。
     * @param unitClipFractions 按 unit key → 可见 fraction（0..1）。
     * @param finished motion 是否已完成。
     */
    data class Sample(
        val caretRect: Rect,
        val unitClipFractions: Map<Long, Float>,
        val finished: Boolean,
    )

    /**
     * 采样当前帧 — caret 和 glyph 都从同一个 [durationNanos] 算 progress，
     * 每个 unit 的 fraction 按 master progress 经 [UnitChannel] 区间映射后线性插值。
     *
     * - durationNanos <= 0：瞬时完成。
     * - elapsed <= 0：progress=0。
     * - elapsed >= duration：progress=1，finished。
     * - 中间：progress = elapsed/duration，线性插值。
     *
     * Issue #735 评论 5773604666 问题2：caret 和 glyph 共用同一只钟，finished = progress finished。
     */
    fun sample(frameTimeNanos: Long): Sample {
        val progress = computeProgress(frameTimeNanos, durationNanos)
        val caretRect = lerpRect(originCaretRect, targetCaretRect, progress.value)
        val fractions =
            unitChannels.mapValues { (_, ch) ->
                val localProgress =
                    mapGlyphProgressToLocal(progress.value, ch.startProgress, ch.endProgress)
                (ch.from + (ch.to - ch.from) * localProgress).coerceIn(0f, 1f)
            }
        return Sample(caretRect, fractions, progress.finished)
    }

    /**
     * 从当前 sample 状态重定向到新目标 — 快速连续输入/删除时调用。
     *
     * 先 sample 当前帧，再从当前 caret 位置和当前文字可见比例重定向：
     * - 新 motion 的 origin = 当前 sample 的 caretRect（从屏幕真实位置开始）。
     * - 新 motion 的 target = [newTargetCaretRect]。
     * - 已有 unit（key 存在于当前 channels 且在新 keys 里）：from = 当前 fraction，
     *   to = 当前角色目标（inserted: 1, deleted: 0），不再沿用旧 ch.to。
     * - 新 inserted unit：from=0, to=1。
     * - 新 deleted unit：from=1, to=0。
     * - 不在新 keys 里的旧 unit：丢弃（已被 timeline 收口或不再由 motion 接管）。
     *
     * 不能把已有文字重新从 0 或 1 开始播 — 已有 unit 的 from 用当前 fraction。
     *
     * Issue #728 评论 5756468643 问题1：redirect 只保留 fraction 没有保留相位。
     * 修复：重定向时把剩余 channel 重新归一化到新 [0, 1]：
     * - 已完成旧 unit 且角色未变（currentFraction == desiredTo）：固定终值。
     * - 已完成旧 unit 但角色变化（currentFraction != desiredTo）：作为 pending 走新剩余动画。
     * - 进行中旧 unit：从 startProgress=0 开始，from=currentFraction，to=desiredTo，继续到新目标。
     * - 未开始旧 unit：接在它后面，from=currentFraction，to=desiredTo。
     * - 新 unit（不在旧 channels 里）：按 caller 传入的正文/几何顺序，和旧 unit 交错排进剩余 schedule，
     *   不按创建时间排（Issue #728 评论 5756468643 问题2）。
     *
     * Issue #728 评论 5760112985 问题1：oldChannel 只提供 currentFraction 和旧 phase，
     * 不再决定新目标方向；desiredTo 由当前角色决定（inserted: 1, deleted: 0）。
     * Issue #728 评论 5760112985 问题2：inserted（正文顺序）+deleted（sourceRange 反序，右往左吞）
     * 合成一条有序 traversal list，只调用一次剩余 schedule builder，统一归一化到一条 [0,1]，
     * 不再分别建 inserted/deleted 两条 schedule 导致重叠。
     *
     * @param newOriginCaretRect 新笔的 old caret rect（用于 fallback，当前 motion 已 finished 时用）。
     * @param newTargetCaretRect 新笔的 new caret rect。
     * @param newInsertedUnitKeys 新笔的 inserted unit keys（按 targetRange 正文顺序排好）。
     * @param newDeletedUnitKeys 新笔的 deleted unit keys（按 sourceRange.start 升序排好，内部会反序成右往左吞）。
     * @param frameTimeNanos 当前帧时间戳。
     * @param durationNanos 新笔共享通道时长。<=0 表示瞬时完成。
     *   Issue #735 评论 5773604666 问题2：单一 duration，caret 和 glyph 共用。
     * @param inheritedFractionsByKey Issue #728 评论 5761525795：split/rebase 换 child key 后，
     *   child 的首帧 fraction 继承信息。key → parent 当前 reveal 投影到 child 局部区间后的 fraction。
     *   - key 在此 map 中：该 unit 是旧 parent split/rekey 出来的 child，从 inheritedFraction 开始，
     *     不当作全新 unit 从 0/1 重启（避免闪烁/重影）。
     *   - key 不在此 map 中（null）：真正本笔新插入/新建 deleted ghost，从 0/1 开始。
     *   默认空 map：无继承信息，所有 oldChannel==null 的 unit 都按全新 unit 处理（保持向后兼容）。
     */
    @Suppress("LongParameterList")
    fun redirectTo(
        newOriginCaretRect: Rect,
        newTargetCaretRect: Rect,
        newInsertedUnitKeys: List<Long>,
        newDeletedUnitKeys: List<Long>,
        frameTimeNanos: Long,
        durationNanos: Long,
        inheritedFractionsByKey: Map<Long, Float> = emptyMap(),
    ): ComposeEditMotion {
        val currentSample = sample(frameTimeNanos)
        val origin = if (currentSample.finished) newOriginCaretRect else currentSample.caretRect
        // 旧 motion 当前的 master progress：必须用旧 motion 的 durationNanos 算，
        // 不能用新 duration，否则相位分类（已完成/进行中/未开始）会失真，
        // 连续编辑时改了 motion 设置就会出现"先冻结再继续"。
        val oldProgress = computeProgress(frameTimeNanos, this.durationNanos).value
        // Issue #728 评论 5760112985 问题2：把 redirect 的剩余 unit 合成一条有序 traversal list，
        // 只调用一次 buildRedirectChannels 统一归一化到一条 [0,1]，不再分别建 inserted/deleted 两条 schedule。
        // inserted 按 targetRange 正文顺序，deleted 按 sourceRange 反序（右往左吞，Backspace 语义）。
        // mixed 沿用 forEdit() 同一规则（inserted 段后接 deleted 反向段）。
        // oldChannel 只提供 currentFraction 和旧 phase，不再提供新 role/to（Issue #728 评论 5760112985 问题1）。
        // Issue #728 评论 5761525795：spec 携带 inheritedFraction，让 classifyRedirectSpec 能区分
        // "split/rekey child"（有继承 fraction）和"真正全新 unit"（无继承 fraction）。
        val insertedSpecs =
            newInsertedUnitKeys.map { key ->
                RedirectUnitSpec(
                    key = key,
                    oldChannel = unitChannels[key],
                    desiredTo = 1f,
                    inheritedFraction = inheritedFractionsByKey[key],
                )
            }
        // deleted 按 sourceRange 反序（右往左吞，Backspace 语义）。
        // newDeletedUnitKeys 已由调用方按 sourceRange.start 升序排好，reversed() 得到降序。
        val deletedSpecs =
            newDeletedUnitKeys.reversed().map { key ->
                RedirectUnitSpec(
                    key = key,
                    oldChannel = unitChannels[key],
                    desiredTo = 0f,
                    inheritedFraction = inheritedFractionsByKey[key],
                )
            }
        // 合成一条有序 traversal list：inserted 段（正文顺序）后接 deleted 反向段，
        // 沿用 forEdit() 的同一规则。只调用一次 buildRedirectChannels 统一归一化到一条 [0,1]。
        val mergedSpecs = insertedSpecs + deletedSpecs
        val newChannels = buildRedirectChannels(mergedSpecs, oldProgress, currentSample)
        return ComposeEditMotion(
            originCaretRect = origin,
            targetCaretRect = newTargetCaretRect,
            unitChannels = newChannels,
            startedAtNanos = frameTimeNanos,
            durationNanos = durationNanos,
        )
    }

    /**
     * 从当前 sample 状态重定向 caret 目标，保留现有 glyph channel —
     * 文字动画还没结束时移动光标（pending selection）时调用。
     *
     * Issue #728 评论 5755928697 问题1：原 [redirectTo] 用空 newInserted/newDeleted keys，
     * 会把不在新 keys 里的旧 unit 全部丢弃，正在吐的字突然重新不可见。
     * 本方法只换 caret 目标，不动文字 unit：
     * - 新 motion 的 origin = 当前 sample 的 caretRect（从屏幕真实位置开始）。
     * - 新 motion 的 target = [newTargetCaretRect]。
     * - 现有 unit channel 全部保留：from = 当前 sample 的 fraction，to 不变。
     * - **不接收 newInserted/newDeleted keys（纯 caret 移动）。**
     *
     * Issue #728 评论 5756468643 问题1：redirect 只保留 fraction 没有保留相位。
     * 修复：重定向后所有未完成的 channel 从新 motion 的 progress=0 立即继续，
     * 剩余 channel 按"当前相位之后的剩余部分"重新归一化到新 [0, 1]：
     * - 已完成 unit：直接固定终值（from=to=currentFraction）。
     * - 当前进行中 unit：从 startProgress=0 开始，from=currentFraction，to 不变，继续到原 to。
     * - 后续未开始 unit：接在它后面，从原 from 到原 to。
     * 没有新 unit 加入，相对顺序保持 unitChannels 的迭代顺序（即创建时的正文顺序）。
     *
     * @param newOriginCaretRect 新 caret rect（用于 fallback，当前 motion 已 finished 时用）。
     * @param newTargetCaretRect 新 caret rect。
     * @param frameTimeNanos 当前帧时间戳。
     * @param durationNanos 新共享通道时长，文字继续从当前 fraction 走到 to。<=0 表示瞬时完成。
     *   Issue #735 评论 5773604666 问题2：单一 duration，caret 和 glyph 共用。
     */
    fun redirectCaretTo(
        newOriginCaretRect: Rect,
        newTargetCaretRect: Rect,
        frameTimeNanos: Long,
        durationNanos: Long,
    ): ComposeEditMotion {
        val currentSample = sample(frameTimeNanos)
        val origin = if (currentSample.finished) newOriginCaretRect else currentSample.caretRect
        // 旧 motion 当前的 master progress：用旧 motion 的 durationNanos 算。
        val oldProgress = computeProgress(frameTimeNanos, this.durationNanos).value
        // 纯换 caret 目标：现有 unit 按"当前相位之后的剩余部分"重新归一，立即继续，
        // 不重新从整条旧 schedule 的 0 开始（Issue #728 评论 5756468643 问题1）。
        // 纯 caret 移动不改变文字目标，desiredTo = ch.to 保持原行为（Issue #728 评论 5760112985 问题1）。
        // Issue #728 评论 5760741452 问题2：不能信 unitChannels 的 map 迭代顺序（插入顺序），
        // 因为 forDelete/forEdit 的区间是反向分配的，插入顺序 [10,11,12] ≠ 实际 traversal 顺序 12→11→10。
        // selection-only redirect 必须保留原 motion 的真实 traversal 顺序，按旧 channel 的 startProgress
        // 升序重建 specs，这样 pending 排序与原 motion 一致，traversal 顺序不会反转。
        val specs =
            unitChannels.entries
                .sortedBy { it.value.startProgress }
                .map { (key, ch) ->
                    RedirectUnitSpec(key = key, oldChannel = ch, desiredTo = ch.to)
                }
        val newChannels = buildRedirectChannels(specs, oldProgress, currentSample)
        return ComposeEditMotion(
            originCaretRect = origin,
            targetCaretRect = newTargetCaretRect,
            unitChannels = newChannels,
            startedAtNanos = frameTimeNanos,
            durationNanos = durationNanos,
        )
    }

    /**
     * redirect 时把旧未完成 unit 和新 unit 合并，按传入 [specs] 顺序重新归一化到新 [0, 1] 剩余 schedule。
     *
     * 每个 spec 带 [RedirectUnitSpec.oldChannel]（旧 unit 的通道，null 表示新 unit）和
     * [RedirectUnitSpec.desiredTo]（当前角色的目标 fraction）。
     * [specs] 已是正文的/几何顺序（调用方按 inserted targetRange.start / deleted range.start 排好），
     * 旧 unit 和新 unit 在这一条顺序里交错排剩余 schedule，不按创建时间排
     * （Issue #728 评论 5756468643 问题2）。
     *
     * Issue #728 评论 5760112985 问题1：oldChannel 只提供 currentFraction 和旧 phase 分类，
     * 新目标方向由 [RedirectUnitSpec.desiredTo] 决定，不再沿用旧 ch.to。
     *
     * 分类（基于旧 motion 的 master progress [oldProgress]）：
     * - 新 unit（oldChannel == null）：from = 1 - desiredTo，to = desiredTo，作为 pending。
     * - 旧 unit 目标已达成（abs(currentFraction - desiredTo) < 1e-5，包括 Completed 角色未变、
     *   或刚好走到目标）：固定终值 from=to=currentFraction，不占区间。
     * - 旧 unit 需要动画（currentFraction != desiredTo）：
     *   - 进行中（startProgress < oldProgress < endProgress）：from=currentFraction,
     *     to=desiredTo，startProgress=0 立即继续，endProgress=remaining。
     *   - 未开始 / Completed（含角色变化）：from=currentFraction, to=desiredTo，作为 pending。
     *     **关键：Completed 且角色变化不再固定旧终值，而是作为 pending 走新剩余动画。**
     *
     * 分配策略（保证重定向后立即继续，不出现"先冻结再继续" — Issue #728 评论 5756468643 问题1）：
     * 1. 目标已达成 unit：直接固定终值（from=to=currentFraction），不占新区间。
     * 2. 进行中 unit 且方向未变（desiredTo == oldChannel.to）：startProgress=0（立即继续），
     *    endProgress=remaining（remaining = 1 - oldProgressWithinChannel，即该 unit 还需要走的比例）。
     *    pending unit 分布在 (remaining, 1] 区间。
     * 3. 未开始 / Completed+角色变化 unit：分布到 (remaining, 1] 区间，等权分配。
     * 4. 进行中 unit 且角色反转（desiredTo != oldChannel.to，Issue #728 评论 5760741452 问题1）：
     *    不再用旧方向的 remaining 决定新区间长度（旧 remaining 只对继续沿旧方向走到旧 ch.to 成立，
     *    角色反转后 glyph 会提前吞完，与 caret 不同步）。把反转的 InProgress unit 当成"重新进入新
     *    traversal schedule 的第一段"，和 pending 一起等权分配 (remaining, 1] 区间
     *    （reversedInProgress 排最前）。单 unit 反转时就是 [0, 1] 全区间，
     *    新 channel startProgress=0, endProgress=1, from=currentFraction, to=desiredTo，
     *    glyph 与从当前屏幕位置出发的 caret 在整个新 motion 内同步到目标。
     * 这样进行中 unit 在 master=0 时 localProgress=0 → fraction 从 currentFraction 继续，
     * 不会冻结；pending unit 在 master 达到其 startProgress 后才开始，各自有独立区间。
     *
     * @param specs 带顺序的 unit 描述（旧 unit 含 oldChannel，新 unit oldChannel=null；都带 desiredTo）。
     * @param oldProgress 旧 motion 中的 master progress（0..1，必须用旧 duration 算）。
     * @param currentSample 当前 sample（含每个 unit 的当前 fraction）。
     * @return 归一化后的新 channels。
     */
    private fun buildRedirectChannels(
        specs: List<RedirectUnitSpec>,
        oldProgress: Float,
        currentSample: Sample,
    ): Map<Long, UnitChannel> {
        val result = mutableMapOf<Long, UnitChannel>()
        // 分三组：已完成（直接固定终值）、进行中（startProgress=0 立即继续）、未开始（pending）
        // 进行中的 remaining 用于计算其 endProgress 和 pending 区间的起始
        var inProgressRemaining = 0f
        val inProgressUnits = mutableListOf<Pair<Long, UnitChannel>>()
        val pending = mutableListOf<Pair<Long, UnitChannel>>()
        // Issue #728 评论 5760741452 问题1：角色反转的 InProgress unit 不能用旧方向 remaining，
        // 当成"重新进入新 traversal schedule 的第一段"，和 pending 一起等权分配 (remaining, 1] 区间。
        // reversedInProgress 排在 pending 之前，保证反转 unit 紧接 in-progress 之后最先开始。
        val reversedInProgress = mutableListOf<Pair<Long, UnitChannel>>()
        for (spec in specs) {
            val desiredTo = spec.desiredTo
            when (val c = classifyRedirectSpec(spec, oldProgress, currentSample)) {
                is SpecClassification.NewUnitPending ->
                    pending.add(spec.key to UnitChannel(1f - desiredTo, desiredTo, 0f, 1f))
                is SpecClassification.FixedTerminal ->
                    result[spec.key] = UnitChannel(c.currentFraction, desiredTo, 0f, 0f)
                is SpecClassification.InProgressDirectionKept -> {
                    inProgressRemaining = c.remaining
                    inProgressUnits.add(
                        spec.key to UnitChannel(c.currentFraction, desiredTo, 0f, c.remaining),
                    )
                }
                is SpecClassification.InProgressDirectionChanged ->
                    reversedInProgress.add(spec.key to UnitChannel(c.currentFraction, desiredTo, 0f, 1f))
                is SpecClassification.Pending ->
                    pending.add(spec.key to UnitChannel(c.currentFraction, desiredTo, 0f, 1f))
            }
        }
        for ((key, ch) in inProgressUnits) {
            result[key] = ch
        }
        // 反转的 InProgress unit 排在 pending 之前，紧接方向不变的 in-progress 之后，
        // 一起等权分配 (inProgressRemaining, 1] 区间（Issue #728 评论 5760741452 问题1）。
        val schedule = reversedInProgress + pending
        if (schedule.isNotEmpty()) {
            val available = (1f - inProgressRemaining).coerceAtLeast(0.001f)
            val step = available / schedule.size
            var cursor = inProgressRemaining
            for ((key, ch) in schedule) {
                result[key] = UnitChannel(ch.from, ch.to, cursor, cursor + step)
                cursor += step
            }
        }
        return result
    }

    /**
     * redirect 时单个 spec 的分类结果 — 决定该 unit 进入哪个收集组。
     *
     * 从 [buildRedirectChannels] 的循环体抽出，降低 Cognitive Complexity。
     */
    private sealed class SpecClassification {
        /** 新 unit（oldChannel == null）：from = 1 - desiredTo，作为 pending。 */
        data object NewUnitPending : SpecClassification()

        /** 目标已达成：固定终值 from=to=currentFraction，不占区间。 */
        data class FixedTerminal(val currentFraction: Float) : SpecClassification()

        /** 方向不变的 InProgress：startProgress=0 立即继续，endProgress=remaining。 */
        data class InProgressDirectionKept(val currentFraction: Float, val remaining: Float) : SpecClassification()

        /** 角色反转的 InProgress（Issue #728 评论 5760741452 问题1）：作为新 schedule 第一段。 */
        data class InProgressDirectionChanged(val currentFraction: Float) : SpecClassification()

        /** NotStarted / Completed+角色变化：作为 pending 走新剩余动画。 */
        data class Pending(val currentFraction: Float) : SpecClassification()
    }

    /**
     * 对单个 [spec] 分类，决定它进入哪个收集组（Issue #728 评论 5760741452 问题1）。
     */
    private fun classifyRedirectSpec(
        spec: RedirectUnitSpec,
        oldProgress: Float,
        currentSample: Sample,
    ): SpecClassification {
        val ch = spec.oldChannel
        val desiredTo = spec.desiredTo
        if (ch == null) {
            // Issue #728 评论 5761525795：oldChannel == null 不再直接等价于"全新 unit"。
            // 如果 spec 带了 inheritedFraction，说明该 unit 是旧 parent split/rekey 出来的 child —
            // parent 当前已画到 inheritedFraction，child 必须从该 fraction 继续，不能从 0/1 重启。
            // 只有真正本笔新插入/新建 deleted ghost（inheritedFraction == null）才走 NewUnitPending。
            val inherited = spec.inheritedFraction
            if (inherited == null) {
                return SpecClassification.NewUnitPending
            }
            // 继承 unit：用 inherited fraction 作为当前 fraction，按与旧 unit 相同的规则分类。
            // inherited == desiredTo：固定终值（如 surviving child 已完整显示，inherited=1, desiredTo=1）。
            // inherited != desiredTo：作为 pending 走新剩余动画（如 deleted ghost 从 0.4 吞到 0）。
            if (abs(inherited - desiredTo) < 1e-5f) {
                return SpecClassification.FixedTerminal(inherited)
            }
            return SpecClassification.Pending(inherited)
        }
        val currentFraction = currentSample.unitClipFractions[spec.key] ?: ch.from
        // 目标已达成（包括 Completed 角色未变、或刚好走到目标）：固定终值，不占区间。
        if (abs(currentFraction - desiredTo) < 1e-5f) {
            return SpecClassification.FixedTerminal(currentFraction)
        }
        // 需要动画：按旧 phase 分类决定区间分配。
        val classification = classifyRedirectUnit(oldProgress, ch)
        // Issue #728 评论 5760741452 问题1：检测角色是否反转。
        // directionChanged=true 时旧方向的 remaining 不再适用，必须重新进入新 traversal schedule。
        val directionChanged = abs(desiredTo - ch.to) >= 1e-5f
        return when (classification) {
            is RedirectUnitClass.InProgress -> {
                if (directionChanged) {
                    // 角色反转：不用旧 remaining，作为新 schedule 的第一段和 pending 一起等权分配。
                    SpecClassification.InProgressDirectionChanged(currentFraction)
                } else {
                    // 方向未变：沿用旧 phase 的 remaining，startProgress=0 立即继续。
                    SpecClassification.InProgressDirectionKept(currentFraction, classification.remaining)
                }
            }
            is RedirectUnitClass.NotStarted,
            is RedirectUnitClass.Completed,
            -> {
                // Completed 且角色变化（currentFraction != desiredTo）不再固定旧终值，
                // 作为 pending 走新剩余动画（Issue #728 评论 5760112985 问题1）。
                SpecClassification.Pending(currentFraction)
            }
        }
    }

    /**
     * redirect 时单个旧 unit 的分类 — 已完成 / 未开始 / 进行中。
     */
    private sealed class RedirectUnitClass {
        data object Completed : RedirectUnitClass()

        data object NotStarted : RedirectUnitClass()

        data class InProgress(val remaining: Float) : RedirectUnitClass()
    }

    private fun classifyRedirectUnit(
        oldProgress: Float,
        ch: UnitChannel,
    ): RedirectUnitClass =
        when {
            oldProgress >= ch.endProgress -> RedirectUnitClass.Completed
            oldProgress <= ch.startProgress -> RedirectUnitClass.NotStarted
            else -> {
                val channelSpan = ch.endProgress - ch.startProgress
                val localProgress =
                    if (channelSpan > 0f) {
                        (oldProgress - ch.startProgress) / channelSpan
                    } else {
                        1f
                    }
                RedirectUnitClass.InProgress(remaining = (1f - localProgress).coerceIn(0.001f, 1f))
            }
        }

    /**
     * redirect 时单个 unit 的描述 — 带顺序，区分旧/新。
     *
     * Issue #728 评论 5760112985 问题1：把"旧通道走到哪了"和"这一笔现在要走向哪里"拆开 —
     * 前者来自 oldChannel/currentSample，后者来自当前 descriptor role（[desiredTo]）。
     * oldChannel 只用于取当前 fraction 和旧 phase 分类，**不能决定新目标方向**。
     *
     * @param key unit 唯一标识。
     * @param oldChannel 旧 motion 中该 unit 的通道；null 表示这是本次新出现的 unit。
     * @param desiredTo 当前角色的目标 fraction（inserted: 1, deleted: 0；
     *   redirectCaretTo 保持 ch.to）。旧 unit 的新目标也由此值决定，不再沿用旧 ch.to。
     * @param inheritedFraction Issue #728 评论 5761525795：split/rekey child 的继承 fraction。
     *   oldChannel == null 且 inheritedFraction != null 时，该 unit 是旧 parent split 出来的 child，
     *   从 inheritedFraction 继续而非从 0/1 重启。null 表示真正全新 unit（无 parent lineage）。
     */
    private data class RedirectUnitSpec(
        val key: Long,
        val oldChannel: UnitChannel?,
        val desiredTo: Float,
        val inheritedFraction: Float? = null,
    )

    /**
     * motion 是否已完成 — 与 sample().finished 语义一致，但不产生 Sample。
     *
     * Issue #735 评论 5773604666 问题2：只用单一 [durationNanos]。
     */
    fun isFinished(frameTimeNanos: Long): Boolean =
        computeProgress(frameTimeNanos, durationNanos).finished

    /**
     * 算单个通道的 progress 和 finished 状态。
     */
    private fun computeProgress(
        frameTimeNanos: Long,
        durationNanos: Long,
    ): ProgressResult {
        if (durationNanos <= 0L) return ProgressResult(value = 1f, finished = true)
        val elapsed = frameTimeNanos - startedAtNanos
        return when {
            elapsed <= 0L -> ProgressResult(value = 0f, finished = false)
            elapsed >= durationNanos -> ProgressResult(value = 1f, finished = true)
            else -> ProgressResult(value = elapsed.toFloat() / durationNanos.toFloat(), finished = false)
        }
    }

    private data class ProgressResult(val value: Float, val finished: Boolean)

    companion object {
        /**
         * 为一笔插入编辑创建 motion — caret 从 origin 向 target 移动，文字 0→1 吐字。
         *
         * 多个 inserted unit 时，按 [insertedUnitKeys] sorted 顺序分配 master progress 区间：
         * n 个 unit，第 i 个 startProgress = i/n, endProgress = (i+1)/n。
         * 光标从左到右依次吐字（Issue #728 评论 5755928697 问题3）。
         *
         * @param originCaretRect 编辑前 caret rect。
         * @param targetCaretRect 编辑后 caret rect。
         * @param insertedUnitKeys 新插入的 unit keys。
         * @param frameTimeNanos 当前帧时间戳。
         * @param durationNanos 共享通道时长。<=0 表示瞬时完成。
         *   Issue #735 评论 5773604666 问题2：caret 和 glyph 共用同一只钟。
         */
        @Suppress("LongParameterList")
        fun forInsert(
            originCaretRect: Rect,
            targetCaretRect: Rect,
            insertedUnitKeys: List<Long>,
            frameTimeNanos: Long,
            durationNanos: Long,
        ): ComposeEditMotion {
            val ranges = allocateEditRanges(insertedUnitKeys, emptyList())
            val channels =
                insertedUnitKeys.associateWith { key ->
                    val (startProgress, endProgress) = ranges.getValue(key)
                    UnitChannel(
                        from = 0f,
                        to = 1f,
                        startProgress = startProgress,
                        endProgress = endProgress,
                    )
                }
            return ComposeEditMotion(
                originCaretRect = originCaretRect,
                targetCaretRect = targetCaretRect,
                unitChannels = channels,
                startedAtNanos = frameTimeNanos,
                durationNanos = durationNanos,
            )
        }

        /**
         * 为一笔删除编辑创建 motion — caret 从 origin 向 target 移动，文字 1→0 吞字。
         *
         * 多个 deleted unit 时，按 [deletedUnitKeys] sorted 顺序**反向**分配区间：
         * n 个 unit，第 i 个 startProgress = (n-1-i)/n, endProgress = (n-i)/n。
         * 光标从右往左依次吞字，先吞最右边的（Issue #728 评论 5755928697 问题3）。
         *
         * @param originCaretRect 编辑前 caret rect（被删文字右侧）。
         * @param targetCaretRect 编辑后 caret rect（被删文字左侧）。
         * @param deletedUnitKeys 被删除的 unit keys。
         * @param frameTimeNanos 当前帧时间戳。
         * @param durationNanos 共享通道时长。<=0 表示瞬时完成。
         *   Issue #735 评论 5773604666 问题2：caret 和 glyph 共用同一只钟。
         */
        @Suppress("LongParameterList")
        fun forDelete(
            originCaretRect: Rect,
            targetCaretRect: Rect,
            deletedUnitKeys: List<Long>,
            frameTimeNanos: Long,
            durationNanos: Long,
        ): ComposeEditMotion {
            val ranges = allocateEditRanges(emptyList(), deletedUnitKeys)
            val channels =
                deletedUnitKeys.associateWith { key ->
                    val (startProgress, endProgress) = ranges.getValue(key)
                    UnitChannel(
                        from = 1f,
                        to = 0f,
                        startProgress = startProgress,
                        endProgress = endProgress,
                    )
                }
            return ComposeEditMotion(
                originCaretRect = originCaretRect,
                targetCaretRect = targetCaretRect,
                unitChannels = channels,
                startedAtNanos = frameTimeNanos,
                durationNanos = durationNanos,
            )
        }

        /**
         * 为一笔混合编辑（Move/替换）创建 motion — caret 从 origin 向 target 移动，
         * inserted units 0→1，deleted units 1→0。
         *
         * 混合 edit（既有 inserted 又有 deleted）区间分配：
         * inserted 按顺序分配前半段 [0, 0.5]，deleted 按反向分配后半段 [0.5, 1]。
         * 光标先经过插入区吐字，再经过删除区吞字（Issue #728 评论 5755928697 问题3）。
         *
         * @param originCaretRect 编辑前 caret rect。
         * @param targetCaretRect 编辑后 caret rect。
         * @param insertedUnitKeys 新插入的 unit keys。
         * @param deletedUnitKeys 被删除的 unit keys。
         * @param frameTimeNanos 当前帧时间戳。
         * @param durationNanos 共享通道时长。<=0 表示瞬时完成。
         *   Issue #735 评论 5773604666 问题2：caret 和 glyph 共用同一只钟。
         */
        @Suppress("LongParameterList")
        fun forEdit(
            originCaretRect: Rect,
            targetCaretRect: Rect,
            insertedUnitKeys: List<Long>,
            deletedUnitKeys: List<Long>,
            frameTimeNanos: Long,
            durationNanos: Long,
        ): ComposeEditMotion {
            val ranges = allocateEditRanges(insertedUnitKeys, deletedUnitKeys)
            val channels = mutableMapOf<Long, UnitChannel>()
            for (key in insertedUnitKeys) {
                val (startProgress, endProgress) = ranges.getValue(key)
                channels[key] =
                    UnitChannel(
                        from = 0f,
                        to = 1f,
                        startProgress = startProgress,
                        endProgress = endProgress,
                    )
            }
            for (key in deletedUnitKeys) {
                val (startProgress, endProgress) = ranges.getValue(key)
                channels[key] =
                    UnitChannel(
                        from = 1f,
                        to = 0f,
                        startProgress = startProgress,
                        endProgress = endProgress,
                    )
            }
            return ComposeEditMotion(
                originCaretRect = originCaretRect,
                targetCaretRect = targetCaretRect,
                unitChannels = channels,
                startedAtNanos = frameTimeNanos,
                durationNanos = durationNanos,
            )
        }

        /**
         * selection-only 移动 — caret 从 origin 向 target 移动，无文字 units。
         *
         * 纯 selection/cursor move 没有文字吞吐，可以继续用独立的 caret-only motion。
         * unit channels 为空，isFinished 只看 [durationNanos]。
         *
         * @param originCaretRect 编辑前 caret rect。
         * @param targetCaretRect 编辑后 caret rect。
         * @param frameTimeNanos 当前帧时间戳。
         * @param durationNanos caret 通道时长。<=0 表示瞬时完成。
         */
        fun forSelectionMove(
            originCaretRect: Rect,
            targetCaretRect: Rect,
            frameTimeNanos: Long,
            caretDurationNanos: Long,
        ): ComposeEditMotion =
            ComposeEditMotion(
                originCaretRect = originCaretRect,
                targetCaretRect = targetCaretRect,
                unitChannels = emptyMap(),
                startedAtNanos = frameTimeNanos,
                durationNanos = caretDurationNanos,
            )
    }
}

/**
 * 线性插值两个 Rect。
 */
private fun lerpRect(
    from: Rect,
    to: Rect,
    t: Float,
): Rect {
    if (t <= 0f) return from
    if (t >= 1f) return to
    return Rect(
        left = from.left + (to.left - from.left) * t,
        top = from.top + (to.top - from.top) * t,
        right = from.right + (to.right - from.right) * t,
        bottom = from.bottom + (to.bottom - from.bottom) * t,
    )
}

/**
 * 把 master glyph progress 映射到 unit 的局部 progress。
 *
 * - glyphProgress <= startProgress：0（该 unit 还没开始）。
 * - glyphProgress >= endProgress：1（该 unit 已走完）。
 * - 中间：(glyphProgress - startProgress) / (endProgress - startProgress)。
 *
 * Issue #728 评论 5755928697 问题3。
 */
private fun mapGlyphProgressToLocal(
    glyphProgress: Float,
    startProgress: Float,
    endProgress: Float,
): Float {
    if (glyphProgress <= startProgress) return 0f
    if (glyphProgress >= endProgress) return 1f
    val span = endProgress - startProgress
    if (span <= 0f) return 1f
    return (glyphProgress - startProgress) / span
}

/**
 * 为 inserted/deleted unit keys 分配 master glyph progress 区间。
 *
 * - 纯 inserted（deletedKeys 空）：n 个 inserted，第 i 个 [i/n, (i+1)/n]（光标从左到右依次吐字）。
 * - 纯 deleted（insertedKeys 空）：n 个 deleted，第 i 个 [(n-1-i)/n, (n-i)/n]（光标从右往左依次吞字）。
 * - 混合：inserted 占前半段 [0, 0.5]，deleted 占后半段 [0.5, 1]。
 *   insertedCount 个 inserted，第 i 个 [0.5*i/insertedCount, 0.5*(i+1)/insertedCount]；
 *   deletedCount 个 deleted，第 i 个 [0.5+0.5*(deletedCount-1-i)/deletedCount, 0.5+0.5*(deletedCount-i)/deletedCount]。
 *   光标先经过插入区吐字，再经过删除区吞字。
 *
 * Issue #728 评论 5756468643 问题2：keys 必须按正文位置排序后传入 —
 * inserted 按 targetRange.start（光标经过顺序），deleted 按 range.start（光标回退顺序）。
 * 不再内部 sorted()，避免按 key 编号排序导致 glyph schedule 顺序和光标位置相反。
 *
 * Issue #728 评论 5755928697 问题3。
 */
private fun allocateEditRanges(
    insertedKeys: List<Long>,
    deletedKeys: List<Long>,
): Map<Long, Pair<Float, Float>> {
    val ranges = mutableMapOf<Long, Pair<Float, Float>>()
    val sortedInserted = insertedKeys
    val sortedDeleted = deletedKeys
    val insertedCount = sortedInserted.size
    val deletedCount = sortedDeleted.size
    if (insertedCount > 0 && deletedCount > 0) {
        // 混合 edit：inserted 前半段 [0, 0.5]，deleted 后半段 [0.5, 1]
        for (i in sortedInserted.indices) {
            val key = sortedInserted[i]
            val start = HALF * i.toFloat() / insertedCount.toFloat()
            val end = HALF * (i + 1).toFloat() / insertedCount.toFloat()
            ranges[key] = start to end
        }
        for (i in sortedDeleted.indices) {
            val key = sortedDeleted[i]
            val start = HALF + HALF * (deletedCount - 1 - i).toFloat() / deletedCount.toFloat()
            val end = HALF + HALF * (deletedCount - i).toFloat() / deletedCount.toFloat()
            ranges[key] = start to end
        }
    } else if (insertedCount > 0) {
        // 纯 insert：第 i 个 [i/n, (i+1)/n]
        for (i in sortedInserted.indices) {
            val key = sortedInserted[i]
            val start = i.toFloat() / insertedCount.toFloat()
            val end = (i + 1).toFloat() / insertedCount.toFloat()
            ranges[key] = start to end
        }
    } else if (deletedCount > 0) {
        // 纯 delete：第 i 个 [(n-1-i)/n, (n-i)/n]（反向）
        for (i in sortedDeleted.indices) {
            val key = sortedDeleted[i]
            val start = (deletedCount - 1 - i).toFloat() / deletedCount.toFloat()
            val end = (deletedCount - i).toFloat() / deletedCount.toFloat()
            ranges[key] = start to end
        }
    }
    return ranges
}

private const val HALF = 0.5f
