package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect

/**
 * Issue #728 评论 5754045689：统一编辑 motion —
 * 一笔编辑只创建一个 motion，统一保存 old/new caret rect + inserted/deleted glyph units，
 * 用同一只钟驱动 caret 移动和文字吞吐。
 *
 * Issue #728 评论 5755928697：三个确定问题的收口 —
 * 1. 文字动画还没结束时移动光标，会把正在运行的 glyph channel 全丢掉：
 *    新增 [redirectCaretTo]，保留现有 unit channel 的 fraction，只换 caret 目标。
 * 2. coordinated=false 时独立 smooth cursor 设置对正文编辑不生效：
 *    caret 和 glyph 各自持有 duration（双 duration），不再强制共用一只钟。
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
 * @param caretDurationNanos caret 通道时长。<=0 表示瞬时完成。
 * @param glyphDurationNanos glyph 通道时长。<=0 表示瞬时完成。
 *   coordinated=true 时 caretDurationNanos == glyphDurationNanos（一只钟）；
 *   coordinated=false 时两者独立（Issue #728 评论 5755928697 问题2）。
 */
class ComposeEditMotion(
    private val originCaretRect: Rect,
    private val targetCaretRect: Rect,
    private val unitChannels: Map<Long, UnitChannel>,
    private val startedAtNanos: Long,
    private val caretDurationNanos: Long,
    private val glyphDurationNanos: Long,
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
     * @param finished motion 是否已完成（caret 和 glyph 都 finished）。
     */
    data class Sample(
        val caretRect: Rect,
        val unitClipFractions: Map<Long, Float>,
        val finished: Boolean,
    )

    /**
     * 采样当前帧 — caret 用 [caretDurationNanos] 算 progress，glyph 用 [glyphDurationNanos] 算 progress，
     * 每个 unit 的 fraction 按 master glyph progress 经 [UnitChannel] 区间映射后线性插值。
     *
     * - durationNanos <= 0：该通道瞬时完成。
     * - elapsed <= 0：progress=0。
     * - elapsed >= duration：progress=1，该通道 finished。
     * - 中间：progress = elapsed/duration，线性插值。
     *
     * finished = caret finished 且 glyph finished（Issue #728 评论 5755928697 问题2）。
     */
    fun sample(frameTimeNanos: Long): Sample {
        val caretProgress = computeProgress(frameTimeNanos, caretDurationNanos)
        val glyphProgress = computeProgress(frameTimeNanos, glyphDurationNanos)
        val caretRect = lerpRect(originCaretRect, targetCaretRect, caretProgress.value)
        val fractions =
            unitChannels.mapValues { (_, ch) ->
                val localProgress =
                    mapGlyphProgressToLocal(glyphProgress.value, ch.startProgress, ch.endProgress)
                (ch.from + (ch.to - ch.from) * localProgress).coerceIn(0f, 1f)
            }
        val finished = caretProgress.finished && glyphProgress.finished
        return Sample(caretRect, fractions, finished)
    }

    /**
     * 从当前 sample 状态重定向到新目标 — 快速连续输入/删除时调用。
     *
     * 先 sample 当前帧，再从当前 caret 位置和当前文字可见比例重定向：
     * - 新 motion 的 origin = 当前 sample 的 caretRect（从屏幕真实位置开始）。
     * - 新 motion 的 target = [newTargetCaretRect]。
     * - 已有 unit（key 存在于当前 channels 且在新 keys 里）：from = 当前 fraction，to 不变。
     * - 新 inserted unit：from=0, to=1。
     * - 新 deleted unit：from=1, to=0。
     * - 不在新 keys 里的旧 unit：丢弃（已被 timeline 收口或不再由 motion 接管）。
     *
     * 不能把已有文字重新从 0 或 1 开始播 — 已有 unit 的 from 用当前 fraction。
     *
     * Issue #728 评论 5756468643 问题1：redirect 只保留 fraction 没有保留相位。
     * 修复：重定向时把剩余 channel 重新归一化到新 [0, 1]：
     * - 已完成旧 unit：直接固定终值（from=to=currentFraction）。
     * - 进行中旧 unit：从 startProgress=0 开始，from=currentFraction，to 不变，继续到原 to。
     * - 未开始旧 unit：接在它后面，从原 from 到原 to。
     * - 新 unit（不在旧 channels 里）：按 caller 传入的正文/几何顺序，和旧 unit 交错排进剩余 schedule，
     *   不按创建时间排（Issue #728 评论 5756468643 问题2）。
     *
     * @param newOriginCaretRect 新笔的 old caret rect（用于 fallback，当前 motion 已 finished 时用）。
     * @param newTargetCaretRect 新笔的 new caret rect。
     * @param newInsertedUnitKeys 新笔的 inserted unit keys。
     * @param newDeletedUnitKeys 新笔的 deleted unit keys。
     * @param frameTimeNanos 当前帧时间戳。
     * @param caretDurationNanos 新笔 caret 通道时长。<=0 表示瞬时完成。
     * @param glyphDurationNanos 新笔 glyph 通道时长。<=0 表示瞬时完成。
     */
    @Suppress("LongParameterList")
    fun redirectTo(
        newOriginCaretRect: Rect,
        newTargetCaretRect: Rect,
        newInsertedUnitKeys: List<Long>,
        newDeletedUnitKeys: List<Long>,
        frameTimeNanos: Long,
        caretDurationNanos: Long,
        glyphDurationNanos: Long,
    ): ComposeEditMotion {
        val currentSample = sample(frameTimeNanos)
        val origin = if (currentSample.finished) newOriginCaretRect else currentSample.caretRect
        // 旧 motion 当前的 master glyph progress：必须用旧 motion 的 glyphDurationNanos 算，
        // 不能用新 duration，否则相位分类（已完成/进行中/未开始）会失真，
        // 连续编辑时改了 motion 设置就会出现"先冻结再继续"。
        val oldGlyphProgress = computeProgress(frameTimeNanos, this.glyphDurationNanos).value
        // newInsertedUnitKeys / newDeletedUnitKeys 已由调用方按正文/几何顺序排好
        // （inserted 按 targetRange.start，deleted 按 range.start）。旧未完成 unit 和新 unit
        // 在这一条顺序里交错重排剩余 schedule，不按创建时间排（Issue #728 评论 5756468643 问题2）。
        val insertedSpecs =
            newInsertedUnitKeys.map { key ->
                RedirectUnitSpec(key = key, oldChannel = unitChannels[key], freshFrom = 0f, freshTo = 1f)
            }
        val deletedSpecs =
            newDeletedUnitKeys.map { key ->
                RedirectUnitSpec(key = key, oldChannel = unitChannels[key], freshFrom = 1f, freshTo = 0f)
            }
        val newChannels = buildRedirectChannels(insertedSpecs, oldGlyphProgress, currentSample).toMutableMap()
        newChannels.putAll(buildRedirectChannels(deletedSpecs, oldGlyphProgress, currentSample))
        return ComposeEditMotion(
            originCaretRect = origin,
            targetCaretRect = newTargetCaretRect,
            unitChannels = newChannels,
            startedAtNanos = frameTimeNanos,
            caretDurationNanos = caretDurationNanos,
            glyphDurationNanos = glyphDurationNanos,
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
     * @param caretDurationNanos 新 caret 通道时长。<=0 表示瞬时完成。
     * @param glyphDurationNanos glyph 通道时长，文字继续从当前 fraction 走到 to。<=0 表示瞬时完成。
     */
    @Suppress("LongParameterList")
    fun redirectCaretTo(
        newOriginCaretRect: Rect,
        newTargetCaretRect: Rect,
        frameTimeNanos: Long,
        caretDurationNanos: Long,
        glyphDurationNanos: Long,
    ): ComposeEditMotion {
        val currentSample = sample(frameTimeNanos)
        val origin = if (currentSample.finished) newOriginCaretRect else currentSample.caretRect
        // 旧 motion 当前的 master glyph progress：用旧 motion 的 glyphDurationNanos 算。
        val oldGlyphProgress = computeProgress(frameTimeNanos, this.glyphDurationNanos).value
        // 纯换 caret 目标：现有 unit 按"当前相位之后的剩余部分"重新归一，立即继续，
        // 不重新从整条旧 schedule 的 0 开始（Issue #728 评论 5756468643 问题1）。
        // unitChannels 的迭代顺序即创建时的正文顺序（forInsert/forDelete/forEdit 都按正文位置排序传入），
        // 没有新 unit 加入，无需重排相对顺序。
        val specs =
            unitChannels.map { (key, ch) ->
                RedirectUnitSpec(key = key, oldChannel = ch, freshFrom = ch.from, freshTo = ch.to)
            }
        val newChannels = buildRedirectChannels(specs, oldGlyphProgress, currentSample)
        return ComposeEditMotion(
            originCaretRect = origin,
            targetCaretRect = newTargetCaretRect,
            unitChannels = newChannels,
            startedAtNanos = frameTimeNanos,
            caretDurationNanos = caretDurationNanos,
            glyphDurationNanos = glyphDurationNanos,
        )
    }

    /**
     * redirect 时把旧未完成 unit 和新 unit 合并，按传入 [specs] 顺序重新归一化到新 [0, 1] 剩余 schedule。
     *
     * 每个 spec 带 [RedirectUnitSpec.oldChannel]（旧 unit 的通道，null 表示新 unit）。
     * [specs] 已是正文的/几何顺序（调用方按 inserted targetRange.start / deleted range.start 排好），
     * 旧 unit 和新 unit 在这一条顺序里交错排剩余 schedule，不按创建时间排
     * （Issue #728 评论 5756468643 问题2）。
     *
     * 分类（基于旧 motion 的 master glyph progress [oldGlyphProgress]）：
     * - 已完成旧 unit（oldGlyphProgress >= endProgress）：固定终值 from=to=currentFraction。
     * - 进行中旧 unit（startProgress < oldGlyphProgress < endProgress）：从 currentFraction 继续到原 to。
     * - 未开始旧 unit（oldGlyphProgress <= startProgress）：从原 from 到原 to。
     * - 新 unit（oldChannel == null）：从 freshFrom 到 freshTo。
     *
     * 分配策略（保证重定向后立即继续，不出现"先冻结再继续" — Issue #728 评论 5756468643 问题1）：
     * 1. 已完成 unit：直接固定终值（from=to=currentFraction），不占新区间。
     * 2. 进行中 unit：startProgress=0（立即继续），endProgress=remaining
     *    （remaining = 1 - oldProgressWithinChannel，即该 unit 还需要走的比例）。
     *    pending unit 分布在 (remaining, 1] 区间。
     * 3. 未开始 unit：分布到 (remaining, 1] 区间，等权分配。
     * 这样进行中 unit 在 master=0 时 localProgress=0 → fraction 从 currentFraction 继续，
     * 不会冻结；pending unit 在 master 达到其 startProgress 后才开始，各自有独立区间。
     *
     * @param specs 带顺序的 unit 描述（旧 unit 含 oldChannel，新 unit oldChannel=null）。
     * @param oldGlyphProgress 旧 motion 中的 master glyph progress（0..1，必须用旧 duration 算）。
     * @param currentSample 当前 sample（含每个 unit 的当前 fraction）。
     * @return 归一化后的新 channels。
     */
    private fun buildRedirectChannels(
        specs: List<RedirectUnitSpec>,
        oldGlyphProgress: Float,
        currentSample: Sample,
    ): Map<Long, UnitChannel> {
        val result = mutableMapOf<Long, UnitChannel>()
        // 分三组：已完成（直接固定终值）、进行中（startProgress=0 立即继续）、未开始（pending）
        // 进行中的 remaining 用于计算其 endProgress 和 pending 区间的起始
        var inProgressRemaining = 0f
        val inProgressUnits = mutableListOf<Pair<Long, UnitChannel>>()
        val pending = mutableListOf<Pair<Long, UnitChannel>>()
        for (spec in specs) {
            val ch = spec.oldChannel
            if (ch == null) {
                pending.add(spec.key to UnitChannel(spec.freshFrom, spec.freshTo, 0f, 1f))
                continue
            }
            val currentFraction = currentSample.unitClipFractions[spec.key] ?: ch.from
            val classification = classifyRedirectUnit(oldGlyphProgress, ch)
            when (classification) {
                is RedirectUnitClass.Completed ->
                    result[spec.key] = UnitChannel(currentFraction, currentFraction, 0f, 0f)
                is RedirectUnitClass.NotStarted ->
                    pending.add(spec.key to UnitChannel(ch.from, ch.to, 0f, 1f))
                is RedirectUnitClass.InProgress -> {
                    inProgressRemaining = classification.remaining
                    inProgressUnits.add(spec.key to UnitChannel(currentFraction, ch.to, 0f, classification.remaining))
                }
            }
        }
        for ((key, ch) in inProgressUnits) {
            result[key] = ch
        }
        if (pending.isNotEmpty()) {
            val available = (1f - inProgressRemaining).coerceAtLeast(0.001f)
            val step = available / pending.size
            var cursor = inProgressRemaining
            for ((key, ch) in pending) {
                result[key] = UnitChannel(ch.from, ch.to, cursor, cursor + step)
                cursor += step
            }
        }
        return result
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
        oldGlyphProgress: Float,
        ch: UnitChannel,
    ): RedirectUnitClass =
        when {
            oldGlyphProgress >= ch.endProgress -> RedirectUnitClass.Completed
            oldGlyphProgress <= ch.startProgress -> RedirectUnitClass.NotStarted
            else -> {
                val channelSpan = ch.endProgress - ch.startProgress
                val localProgress =
                    if (channelSpan > 0f) {
                        (oldGlyphProgress - ch.startProgress) / channelSpan
                    } else {
                        1f
                    }
                RedirectUnitClass.InProgress(remaining = (1f - localProgress).coerceIn(0.001f, 1f))
            }
        }

    /**
     * redirect 时单个 unit 的描述 — 带顺序，区分旧/新。
     *
     * @param key unit 唯一标识。
     * @param oldChannel 旧 motion 中该 unit 的通道；null 表示这是本次新出现的 unit。
     * @param freshFrom 新 unit 的起点 fraction（inserted: 0, deleted: 1）。旧 unit 此值不会被使用。
     * @param freshTo 新 unit 的终点 fraction（inserted: 1, deleted: 0）。旧 unit 此值不会被使用。
     */
    private data class RedirectUnitSpec(
        val key: Long,
        val oldChannel: UnitChannel?,
        val freshFrom: Float,
        val freshTo: Float,
    )

    /**
     * motion 是否已完成 — caret 和 glyph 都 finished，与 sample().finished 语义一致，但不产生 Sample。
     */
    fun isFinished(frameTimeNanos: Long): Boolean =
        computeProgress(frameTimeNanos, caretDurationNanos).finished &&
            computeProgress(frameTimeNanos, glyphDurationNanos).finished

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
         * 多个 inserted unit 时，按 [insertedUnitKeys] sorted 顺序分配 master glyph progress 区间：
         * n 个 unit，第 i 个 startProgress = i/n, endProgress = (i+1)/n。
         * 光标从左到右依次吐字（Issue #728 评论 5755928697 问题3）。
         *
         * @param originCaretRect 编辑前 caret rect。
         * @param targetCaretRect 编辑后 caret rect。
         * @param insertedUnitKeys 新插入的 unit keys。
         * @param frameTimeNanos 当前帧时间戳。
         * @param caretDurationNanos caret 通道时长。<=0 表示瞬时完成。
         * @param glyphDurationNanos glyph 通道时长。<=0 表示瞬时完成。
         */
        @Suppress("LongParameterList")
        fun forInsert(
            originCaretRect: Rect,
            targetCaretRect: Rect,
            insertedUnitKeys: List<Long>,
            frameTimeNanos: Long,
            caretDurationNanos: Long,
            glyphDurationNanos: Long,
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
                caretDurationNanos = caretDurationNanos,
                glyphDurationNanos = glyphDurationNanos,
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
         * @param caretDurationNanos caret 通道时长。<=0 表示瞬时完成。
         * @param glyphDurationNanos glyph 通道时长。<=0 表示瞬时完成。
         */
        @Suppress("LongParameterList")
        fun forDelete(
            originCaretRect: Rect,
            targetCaretRect: Rect,
            deletedUnitKeys: List<Long>,
            frameTimeNanos: Long,
            caretDurationNanos: Long,
            glyphDurationNanos: Long,
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
                caretDurationNanos = caretDurationNanos,
                glyphDurationNanos = glyphDurationNanos,
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
         * @param caretDurationNanos caret 通道时长。<=0 表示瞬时完成。
         * @param glyphDurationNanos glyph 通道时长。<=0 表示瞬时完成。
         */
        @Suppress("LongParameterList")
        fun forEdit(
            originCaretRect: Rect,
            targetCaretRect: Rect,
            insertedUnitKeys: List<Long>,
            deletedUnitKeys: List<Long>,
            frameTimeNanos: Long,
            caretDurationNanos: Long,
            glyphDurationNanos: Long,
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
                caretDurationNanos = caretDurationNanos,
                glyphDurationNanos = glyphDurationNanos,
            )
        }

        /**
         * selection-only 移动 — caret 从 origin 向 target 移动，无文字 units。
         *
         * glyph 通道无 unit，glyphDurationNanos 内部设为 0（瞬时完成），isFinished 只看 caret。
         *
         * @param originCaretRect 编辑前 caret rect。
         * @param targetCaretRect 编辑后 caret rect。
         * @param frameTimeNanos 当前帧时间戳。
         * @param caretDurationNanos caret 通道时长。<=0 表示瞬时完成。
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
                caretDurationNanos = caretDurationNanos,
                glyphDurationNanos = 0L,
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
