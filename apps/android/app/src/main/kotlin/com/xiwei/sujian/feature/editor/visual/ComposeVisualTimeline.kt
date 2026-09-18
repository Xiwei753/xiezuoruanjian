package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.layout.cursorRect
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy
import kotlin.math.max

/**
 * #689 评论 5674631257 步骤2：持续视觉时间线 —
 * 真正长期存在的屏幕动画状态。
 *
 * 不再用一条全局 `0f..1f` 控制所有字。每个仍由 overlay 接管的文字单元保存两条
 * **互不重置**的通道：透明度和位置。
 *
 * 核心不变量：
 * - 已有 unit 通过 [ComposeVisualPatch.offsetMap] 映射到新正文后仍然存活时，
 *   原来的 alpha 动画**继续使用原来的 startedAtNanos**，
 *   快速输入第二个字不能让第一个字重新从 0 开始。
 * - 如果新 layout 让它的位置发生变化，只把 position 通道从"此刻屏幕位置"重定向到新位置，
 *   position 使用新的开始时间；位置没变则 position 通道也不重建。
 * - 新插入 unit：从 `alpha 0 -> 1` 新建自己的 alpha 通道，位置直接是新 layout 的真实位置。
 * - 删除 unit：先取它此刻屏幕实际 alpha/位置，再变成 ghost unit（targetRange = null），
 *   alpha 从当前值继续到 0，位置保持当前屏幕位置。
 * - [hiddenRanges] 每一帧直接从当前 `VisualTextUnit.targetRange != null` 且仍由 overlay
 *   绘制的 unit 推导，不从"上一事务 suppressed ranges"继承。
 *
 * 这个类不是 Compose 可观察状态 — 它是纯数据状态机。
 * [ComposeEditorVisualState] 持有它并在每次 sample 后把结果同步给 Compose StateFlow。
 *
 * #698 评论 5697612595 边界声明 —
 * **timeline 只能产出绘制状态（[ComposeVisualScene]），绝不能反向改变 BasicTextField 输出表示。**
 * 具体而言：
 * - timeline 不持有 [androidx.compose.foundation.text.input.OutputTransformation] 引用，
 *   不接触 [androidx.compose.foundation.text.BasicTextField] 的 TextFieldState。
 * - [sample] 返回的 [ComposeVisualScene.hiddenRanges] 只供 draw 层（[EditorTextFieldDrawLayer]）
 *   做正文裁切（用背景色填充字形 path 遮住系统正文），不再回流给 OutputTransformation。
 * - sample() 里 hiddenRanges 推导逻辑：从当前 `targetRange != null` 且仍由 overlay 绘制的 unit 推导，
 *   不从"上一事务 suppressed ranges"继承，也不从任何 TextField 输出状态读取。
 * 这条边界是断开"动画 hiddenRanges -> OutputTransformation 改正文显示 -> BasicTextField 再 layout ->
 * VisualState 再消费 layout"回路的关键 — timeline 产出 hiddenRanges 后单向流给 draw 层，
 * draw 层用它裁切，不再回流给 OutputTransformation 触发 BasicTextField 二次 layout。
 */
@Suppress("TooManyFunctions")
class ComposeVisualTimeline {
    /** 当前所有文字单元（存活 + ghost）。 */
    private var units: List<VisualTextUnit> = emptyList()

    /** 单调递增的 unit key — 新插入 unit 分配唯一 key。 */
    private var nextUnitKey: Long = 1L

    /**
     * #691：统一光标位置 — 由同一个 VisualScene / frame clock 维护。
     * 不再使用独立的 Animatable<Rect> + LaunchedEffect。
     *
     * #691 评论 5679242735 修改3：cursorChannel 改成 [CursorTrack]，
     * 支持多段 [CursorMotionPath]（一次提交多个插入 unit 时光标依次经过每个字/cluster）。
     */
    private var cursorChannel: CursorTrack? = null

    /**
     * #703 评论 5709208101 问题2：coordinated + spatial clip 模式标记 —
     * applyPatch 时从 patch.motionPolicy.effective() 设置，
     * sample 时传给 ComposeVisualScene，draw 层据此用 clipFraction 覆盖 alpha（effective alpha=1）。
     */
    private var coordinatedSpatialClip: Boolean = false

    /**
     * #691 评论 5684993243 / 评论 5685940102：已在前一可见帧真正呈现过的 unit key 集合。
     *
     * 这个事实由 [sample] 推进 — 只有真正采样到一个存活 unit 且该帧它已被 scene 接管并可见时，
     * 才把 unit.key 计入。applyPatch 判断 started/pending 只看这份持久状态，
     * 不再从 TimedFloat.startedAtNanos/from 反推（那些字段会被 rebaseUnitForPatch 改写）。
     *
     * #691 评论 5685940102：语义从"alpha 是否离开起点"扩展到"unit 是否已在可见帧呈现过"，
     * 覆盖 retained reflow unit（alpha 1→1，只有 position 通道在动）。
     * 之前只看 alpha 是否离开起点会把 retained reflow unit 漏掉，
     * 导致下一笔 patch 把它判成 pending 重建 alpha 0→1，已可见的文字突然变透明再淡入。
     *
     * - 新 unit 创建时不在集合里。
     * - mapSurvivingSlice/rebase 保持 key（copy 保留 key）。
     * - unit 收口移除/转 ghost/clear/settleForPolicyChange 时同步清理对应 key。
     */
    private var presentedKeys: MutableSet<Long> = mutableSetOf()

    /**
     * 应用一个屏幕 diff — 先 sample(now) 拿到旧动画此刻屏幕真实画到的 alpha 和位置，
     * 再处理新 patch。不能从事务的 progress 反算，也不能先归零。
     *
     * #691：同时接受光标 motion 参数，在同一个调用内处理文字和光标，
     * 保证 cursor 和 text units 使用同一个 frameTimeNanos。
     *
     * #691 评论 5679242735 修改2：检查 [patch.motionPolicy.effective] 的 textEnabled —
     * 文字动画关闭时不创建任何文字 alpha/position track（units = emptyList()），
     * 不只是把 duration 改成 0。否则仍可能产生一帧 hiddenRanges/ghost 所有权问题。
     *
     * #691 评论 5679242735 修改3：cursor 参数从 `cursorToRect: Rect?` 改成
     * `cursorPath: List<CursorMotionPoint>?`，支持多段路径。
     * 一次提交多个插入 unit 时光标依次经过每个字/cluster，不再被压成一条直线。
     *
     * @param patch 这一帧的屏幕 diff — 包含 [ComposeVisualPatch.intent] 用于 fallback survival map。
     * @param frameTimeNanos 当前帧时间戳（来自 Compose frame clock，不用 System.nanoTime()）。
     * @param cursorFromRect 光标在旧 layout 中的位置（屏幕坐标）— null 表示无光标 motion。
     * @param cursorPath 光标运动路径点序列（屏幕坐标）— null 表示无光标 motion。
     *   单点路径：snap；多点路径：按 [CursorMotionPoint.endFraction] 分段插值。
     * @param cursorDurationNanos 光标动画时长 — 0 表示瞬时 snap。
     */
    fun applyPatch(
        patch: ComposeVisualPatch,
        frameTimeNanos: Long,
        cursorFromRect: Rect? = null,
        cursorPath: List<CursorMotionPoint>? = null,
        cursorDurationNanos: Long = 0L,
    ) {
        // #691 评论 5679242735 修改2 / 设置语义 G：文字动画时长以用户设置 textDurationMillis 为唯一事实来源，
        // 不再用 Core intent 的 patch.durationMs（那样用户改时长设置不生效）。
        val policy = patch.motionPolicy.effective()
        val durationNanos = policy.textDurationMillis.coerceAtLeast(0L) * NANOS_PER_MS

        // #703 评论 5709208101 问题2：记录 coordinated + spatial clip 模式，
        // sample 时传给 scene，draw 层据此用 clipFraction 覆盖 alpha。
        coordinatedSpatialClip = policy.textEnabled && policy.cursorEnabled && policy.coordinated

        // #708 评论 5728951138：当前 patch 已把 active unit 转成 ghost 的范围
        // （旧正文坐标系，与 patch.deletedUnits 同坐标系）。
        // reconcileDeletedGhosts 用它做差集，只给"deletedUnits - currentPatchGhostedCoverage"
        // 的剩余部分建 alpha=1 完整 ghost，避免整段补 ghost 导致已由 active unit 接管的部分被重画一遍（重影）。
        // 只收集**当前 patch** 产生的 ghost 范围，不收集历史遗留 ghost（targetRange == null 的旧 ghost）—
        // 历史 ghost 的 range/layout 可能属于更早的正文坐标，不能拿来减当前 patch.deletedUnits。
        val currentPatchGhostedCoverage = mutableListOf<TextRange>()

        // #703 评论 D：scene redirect — 快速输入/删除采用 scene redirect，不堆积旧动画。
        // 新 edit 到达时，从"当前屏幕真正画到的位置"重定向到新目标。
        // 旧 editEpoch 的延迟 patch、ghost、cursor path、retained move 如果已被新编辑覆盖，
        // 就必须失效或正确 rebase，不能继续在后面补播。
        // 具体：旧 ghost 的 range 被新 insertedUnits 完全包含时，移除旧 ghost
        // （新编辑已在该位置插入新字，旧 ghost 不应继续淡出）。
        // 旧存活 unit 的 targetRange 被新 deletedUnits 完全包含时，转成 ghost
        // （新编辑删除了该位置的旧字，旧 unit 不应继续存活）。
        // 只对"完全包含"转 ghost/移除 — 部分重叠保留原有 rebase/切片逻辑（#689 缺陷5 跨删除洞切存活 slice），
        // 否则会把跨删除洞的部分存活 unit 也整块转 ghost，破坏切片行为。
        if (policy.textEnabled) {
            val newInsertedRanges = patch.insertedUnits
            val newDeletedRanges = patch.deletedUnits
            if (newInsertedRanges.isNotEmpty() || newDeletedRanges.isNotEmpty()) {
                units =
                    units.mapNotNull { unit ->
                        if (unit.targetRange == null) {
                            // ghost：如果 range 被新 insertedUnits 完全包含，移除（新编辑已在该位置插入新字）
                            if (newInsertedRanges.any { ins ->
                                    ins.start <= unit.range.start && unit.range.end <= ins.end
                                }
                            ) {
                                presentedKeys.remove(unit.key)
                                null
                            } else {
                                unit
                            }
                        } else {
                            // 存活 unit：如果 targetRange 被新 deletedUnits 完全包含，转 ghost
                            val fullyDeleted =
                                newDeletedRanges.firstOrNull { del ->
                                    del.start <= unit.targetRange!!.start && unit.targetRange!!.end <= del.end
                                }
                            if (fullyDeleted != null) {
                                // #708 评论 5728951138：记录被当前 patch 转成 ghost 的范围，
                                // reconcileDeletedGhosts 用它做差集避免整段补 ghost。
                                // unit.targetRange 与 patch.deletedUnits 同坐标系（当前正文坐标）。
                                // #708 评论 5728951138 修复：alpha=0 的 unit 不可见，转成 ghost 会被
                                // mapSurvivingUnits 丢弃（currentAlpha > 0f 检查），但 coverage 已记录
                                // 该范围导致 reconcileDeletedGhosts 不再为它建 ghost → ghost 丢失。
                                // 改：alpha <= 0 时不转 ghost、不记 coverage，直接移除，
                                // 让 reconcileDeletedGhosts 为它建 alpha=1 完整 ghost（与旧 createDeletedGhosts 一致）。
                                if (currentAlpha(unit.alpha, frameTimeNanos) > 0f) {
                                    currentPatchGhostedCoverage.add(unit.targetRange!!)
                                    toGhost(unit, frameTimeNanos, durationNanos, unit.range)
                                } else {
                                    presentedKeys.remove(unit.key)
                                    null
                                }
                            } else {
                                unit
                            }
                        }
                    }
            }
        }

        // #691 评论 5681258225：surviving 列表在 if/else 之前声明，
        // 让 cursor 合并逻辑在 textEnabled=false 时也能访问（此时为空列表）。
        val surviving = mutableListOf<VisualTextUnit>()

        // #691 评论 5679242735 修改2：textEnabled=false 时不创建任何文字 alpha/position track。
        // 不要只把 duration 改成 0 — 否则仍可能产生一帧 hiddenRanges/ghost 所有权问题。
        // #691 评论 5684136311：在 rebase 之前用原始 unit 判断是否已产生可见进度。
        // rebase 会把进行中通道的 startedAt 重设为 frameTimeNanos，丢失"是否同一 VSync"信息。
        // textEnabled=false 时给空 map（cursor 路径不会用到）。
        // #691 评论 5684993243 / 评论 5685940102：hasBeenPresented 内部优先看 [presentedKeys] 持久状态，
        // 处理"同一 VSync 连续 patch"场景；否则回退到 [isUnitVisibleAndPresented] 通道判断
        // "unit 是否已可见呈现"（覆盖插入 unit alpha 0→1 和 retained reflow unit alpha 1→1），
        // 处理"不同时间 patch 但中间未 sample"场景。
        // #708 评论 5728507555：用 mutable map 让 mapSurvivingUnits 在 split 时把
        // child key 的即时 presented 状态写入，使本笔 applyPatch 后面的 started/pending
        // 分类能读到 child 的状态，不因 child key 是新分配的就判成 pending 重置 alpha。
        val effectiveProgressByKey =
            if (policy.textEnabled) {
                units.associate { it.key to hasBeenPresented(it, frameTimeNanos) }.toMutableMap()
            } else {
                mutableMapOf()
            }

        if (policy.textEnabled) {
            // 第一步：先 rebase 当前所有 unit 到此刻的真实 alpha/位置。
            // #691 评论 5680711648 修复1：不能用 sampleUnit() — 它会把尚未开始的通道也 rebase 到 now，
            // 丢失绝对 start time。改用 rebaseUnitForPatch()：尚未开始的通道原样保留未来起点。
            val sampledUnits = units.map { rebaseUnitForPatch(it, frameTimeNanos) }

            // 第二步：把存活 unit 通过 offsetMap 映射到新正文（缺陷5 切片）。
            val ghosting = mutableListOf<VisualTextUnit>()
            mapSurvivingUnits(
                sampledUnits,
                patch,
                frameTimeNanos,
                durationNanos,
                surviving,
                ghosting,
                effectiveProgressByKey,
                currentPatchGhostedCoverage,
            )

            // 第三步：处理本 patch 新插入的 unit。
            // #691 评论 5682970101：移除 queueTailEndNanos 串行 FIFO，改为 scene redirect 有界窗口。
            // 已开始的 unit 保留当前 alpha/position；尚未开始的 surviving + 新 inserted 一起在
            // [frameTimeNanos, frameTimeNanos + durationNanos] 有界窗口内按正文顺序均匀分段。
            // 这样动画尾巴不随字符数线性增长，最后一笔输入后最多再过一个 textDuration 全部完成。
            val startedSurviving = mutableListOf<VisualTextUnit>()
            val pendingSurviving = mutableListOf<VisualTextUnit>()
            for (unit in surviving) {
                if (unit.targetRange == null) {
                    // ghost（targetRange == null）不应出现在 surviving 里，但防御性保留
                    startedSurviving.add(unit)
                    continue
                }
                // #691 评论 5684136311：用 rebase 前原始 unit 的可见进度判断，不用 startedAtNanos <= frameTimeNanos。
                // 同一 VSync、零进度 → 可重新分段；已在之前可见帧产生真实进度 → 从当前状态继续，不归零。
                // #708 评论 5728507555：用 effectiveProgressByKey（mutable）替代旧 progressByKey —
                // mapSurvivingUnits 在 split 时已把 child key 的 presented 状态写入，
                // 这里能读到 child 的即时状态，不因 child key 是新分配的就判成 pending 重置 alpha。
                if (effectiveProgressByKey[unit.key] == true) {
                    startedSurviving.add(unit)
                } else {
                    pendingSurviving.add(unit)
                }
            }
            val repartitioned =
                repartitionPendingAndInsertedUnits(
                    pendingSurviving,
                    patch,
                    frameTimeNanos,
                    durationNanos,
                )

            // 第四步：处理本 patch 显式删除的 unit（缺陷1 从 oldLayout 建 ghost）+
            // 删除时间表收口（#694 评论 5694645209 问题3）。
            // #708 评论 5728951138：把所有权判断和 schedule 收口成 reconcileDeletedGhosts —
            // 用 subtractRanges(deletedUnits, currentPatchGhostedCoverage) 算剩余范围，
            // 只给 remaining 建 alpha=1 完整 ghost，避免整段补 ghost 导致重影。
            // schedule 匹配从 exact-range 改成"range 在父 deletedUnit 内"，
            // 让部分 slice（如 [1,2) 在 [0,2) 内）也能匹配到父 deletedUnit 的 stage schedule。
            reconcileDeletedGhosts(
                ghosting = ghosting,
                orderedDeletedUnits = patch.deletedUnits,
                currentPatchGhostedCoverage = currentPatchGhostedCoverage,
                oldLayout = patch.oldLayout,
                frameTimeNanos = frameTimeNanos,
                durationNanos = durationNanos,
            )

            // 第五步：retainedMoves（缺陷4 临时接管回流文字）。
            // 注意：retainedMoves 作用于 startedSurviving（已开始 unit 的位置重定向），
            // 不应作用于 pendingSurviving（它们已被重新分段）。
            applyRetainedMoves(patch, frameTimeNanos, durationNanos, startedSurviving)

            // #708 评论 5723410606 第四节：独立 reflow 通道 —
            // reflowMoves 是与 retainedMoves 不同的所有权：字仍存在，但从 oldBounds 平移到 newBounds，
            // 始终全亮。active 时把 newRange 放进 hiddenRanges，让 BasicTextField 的新位置暂时不重复画；
            // 完成后直接从 timeline 删除，下一帧由 BasicTextField 新位置接管。
            applyReflowMoves(patch, frameTimeNanos, durationNanos, startedSurviving)

            // 合并：已开始存活 + 重新分段（pending + 新插入） + ghost
            units = startedSurviving + repartitioned.allUnits + ghosting
            // surviving 列表对外暴露给 cursor 合并逻辑：只含 startedSurviving + repartitionedPending
            // （不含新 inserted units，避免 cursor 把新 inserted 当 surviving 重复计算 endFraction）。
            // cursor 的 unstartedSurviving 从此列表取"尚未开始"的 unit，只会取到 repartitionedPending。
            surviving.clear()
            surviving.addAll(startedSurviving)
            surviving.addAll(repartitioned.repartitionedPending)
        } else {
            // 文字动画关闭：不创建任何文字 alpha/position track。
            units = emptyList()
        }

        // #691 评论 5679242735 修改3：光标 motion 并入 timeline — 与文字共享同一个 frameTimeNanos。
        // cursor 独立按 policy.cursorEnabled 继续处理（由调用方 computeCursorParamsForPatch 决定是否传参）。
        // 支持多段路径：cursorPath 是 List<CursorMotionPoint>，不再只取 last().rect。
        if (cursorFromRect != null && cursorPath != null && cursorPath.isNotEmpty()) {
            applyCursorPatch(
                patch = patch,
                policy = policy,
                frameTimeNanos = frameTimeNanos,
                cursorFromRect = cursorFromRect,
                cursorPath = cursorPath,
                cursorDurationNanos = cursorDurationNanos,
                surviving = surviving,
                progressByKey = effectiveProgressByKey,
            )
        }
    }

    /**
     * #691 评论 5682970101：cursor 从文字 segment 时间表生成。
     *
     * survivingCursorPoints 只取"尚未开始"的 unit（alpha.startedAtNanos > frameTimeNanos），
     * 不包含已开始但未完成的 unit（它们已经在屏幕上，不需要 cursor 再追到它们的 caret）。
     * endFraction 从同一份 segment 时间表生成：n = pendingSurviving.size + 新 cursorPath points 数量，
     * 第 i 个 point 的 endFraction = (i + 1f) / n。
     *
     * #691 评论 5686733880：cursor 时长决定权收口到 [ComposeEditorVisualState.computeCursorParamsForPatch]，
     * 这里不再按 `policy.coordinated` 二次改时长。调用方传入的 `cursorDurationNanos` 已经是最终决定值：
     * - 真正的协同文字事务（textEnabled && cursorEnabled && coordinated && !isCursorOnly）→ textDurationMillis
     * - 其他所有情况（含设置矩阵 D：textEnabled=false, cursorEnabled=true, coordinated=true）→ cursorDurationMillis
     * 旧逻辑在此处又做一次 `if (policy.coordinated) durationNanos else cursorDurationNanos`，
     * 会把上层算好的 cursorDurationNanos 再次覆盖成 textDurationMillis，导致设置矩阵 D 下
     * cursor 错误使用 textDurationMillis（拖到 1000ms 才完成）。
     */
    @Suppress("LongParameterList")
    private fun applyCursorPatch(
        patch: ComposeVisualPatch,
        policy: EditorMotionPolicy,
        frameTimeNanos: Long,
        cursorFromRect: Rect,
        cursorPath: List<CursorMotionPoint>,
        cursorDurationNanos: Long,
        surviving: List<VisualTextUnit>,
        progressByKey: Map<Long, Boolean>,
    ) {
        val current = cursorChannel
        val startRect =
            if (current != null) {
                sampleCursorRect(frameTimeNanos) ?: cursorFromRect
            } else {
                cursorFromRect
            }

        val survivingCursorPoints = mutableListOf<CursorMotionPoint>()
        if (policy.textEnabled) {
            // #691 评论 5684136311：用 rebase 前原始 unit 的可见进度判断，不用 startedAtNanos > frameTimeNanos。
            // 尚未产生可见进度的 unit（含同一 VSync 零进度 unit）都纳入 cursor 路径，
            // 让 cursor 用同一份 segment 表生成 caret point，而不是跳过零进度 unit。
            val unstartedSurviving =
                surviving
                    .filter {
                        it.targetRange != null &&
                            progressByKey[it.key] != true
                    }
                    .sortedBy { it.targetRange!!.start }
            for (unit in unstartedSurviving) {
                val caretOffset = unit.targetRange!!.end
                // #691 评论 5681258225：跨行场景关键 — 用最新 layout 取 caret rect，不用旧 layout。
                val caretRect = safeCursorRectFromLayout(patch.newLayout, caretOffset) ?: continue
                survivingCursorPoints.add(CursorMotionPoint(rect = caretRect, endFraction = 0f))
            }
        }

        // 合并 surviving cursor points + 新 patch cursorPath points
        // #691 评论 5682970101：endFraction 从文字 segment 时间表生成 —
        // n = pendingSurviving.size + 新 cursorPath points 数量，
        // 第 i 个 point 的 endFraction = (i + 1f) / n。
        // 这在有界窗口内均匀分段时等于文字 segment 的结束分数。
        // #691 评论 5681258225：只有当存在 surviving cursor points 时才重算 endFraction —
        // 没有 surviving points 时保持原 cursorPath 的自定义 endFraction 不变
        // （单笔 patch 的 cursor path 可能有不均匀的 endFraction，如 1/3, 1/2, 1.0）。
        val allPoints = survivingCursorPoints + cursorPath
        val normalizedPoints =
            if (survivingCursorPoints.isNotEmpty() && allPoints.size > 1) {
                val n = allPoints.size
                allPoints.mapIndexed { i, point ->
                    point.copy(endFraction = (i + 1f) / n)
                }
            } else {
                allPoints
            }

        // #691 评论 5686733880：直接使用调用方传入的 cursorDurationNanos。
        // cursor 时长决定只保留在 [ComposeEditorVisualState.computeCursorParamsForPatch] 一处，
        // 这里不再按 policy.coordinated 二次覆盖（避免设置矩阵 D 下 cursor 错误使用 textDurationMillis）。
        cursorChannel =
            CursorTrack(
                fromRect = startRect,
                points = normalizedPoints,
                startedAtNanos = frameTimeNanos,
                durationNanos = cursorDurationNanos,
            )
    }

    /**
     * #689 评论 5675270164 缺陷5：把存活 unit 通过 offsetMap 映射到新正文。
     * 用 splitMappedRangeForward 切片，不整块判死。
     *
     * #708 评论 5727808906：split 时分配独立新 key —
     * 当一个父 unit 被切成 2 个及以上子 unit（surviving slice + ghost slice）时，
     * 每个子 unit 都分配独立新 key（nextUnitKey++），不再共用父 key。
     * 只有一个 slice 且代表整个父 unit 时保留 parent key。
     * 这样 [ComposeVisualScene.unitClipFractions]（Map<Long, Float>，key=unit.key）
     * 不会同 key 互相覆盖，三段文字各自拿到独立 fraction。
     * 同时 [presentedKeys] 不会因 `associate` 压缩成最后一个同 key 结果，
     * `presentedKeys.remove(key)` 不会误删其他活跃 slice 的 presented 身份。
     *
     * #708 评论 5727808906：presented 状态不因换 child key 丢失 —
     * split 发生时（slices.size >= 2）：如果父 unit 已 presented（progressByKey[unit.key] == true），
     * 把每个 surviving child 的 key 放进 [presentedKeys]，把父 key 从 [presentedKeys] 移除。
     * 不 split 时（保留 parent key）：presentedKeys 不变（key 没换）。
     */
    @Suppress(
        "LongMethod",
        "CyclomaticComplexMethod",
        "CognitiveComplexMethod",
        "LongParameterList",
        "NestedBlockDepth",
    )
    private fun mapSurvivingUnits(
        sampledUnits: List<VisualTextUnit>,
        patch: ComposeVisualPatch,
        frameTimeNanos: Long,
        durationNanos: Long,
        surviving: MutableList<VisualTextUnit>,
        ghosting: MutableList<VisualTextUnit>,
        progressByKey: MutableMap<Long, Boolean>,
        currentPatchGhostedCoverage: MutableList<TextRange>,
    ) {
        val offsetMap = patch.offsetMap
        val newLayout = patch.newLayout
        val newTextLength = newLayout.result.layoutInput.text.length
        for (unit in sampledUnits) {
            val target = unit.targetRange
            if (target == null) {
                // 已经是 ghost：继续淡出。alpha 已到 0 的丢弃。
                if (currentAlpha(unit.alpha, frameTimeNanos) > 0f) {
                    ghosting.add(unit)
                }
                continue
            }
            val slices = computeSlices(target, offsetMap, newTextLength, patch.intent)
            // #708 评论 5727808906：split 时分配独立新 key —
            // 只有一个 slice 且代表整个父 unit（slice.oldSubRange == unit.range）时保留 parent key；
            // 2 个及以上子 unit 时每个子 unit 分配独立新 key。
            val isSplit = slices.size >= 2
            val parentPresented = progressByKey[unit.key] == true
            if (isSplit) {
                // 父 unit 已被 split 成子 unit，父 key 不再活跃，从 presentedKeys 移除
                presentedKeys.remove(unit.key)
                // #708 评论 5728507555：split 后父 key 不再活跃，从 effectiveProgressByKey 移除，
                // 并把每个 child key 的 presented 状态显式写入，让本次 applyPatch 后面的 started/pending
                // 分类能读到 child 的即时 presented 状态，不因 child key 是新分配的就判成 pending 重置 alpha。
                progressByKey.remove(unit.key)
            }
            for (slice in slices) {
                // 决定本 slice 的 childKey：
                // - 不 split（单 slice 且代表整个父 unit）→ 保留 parent key
                // - split（2+ 子 unit）→ 每个子 unit 分配独立新 key
                val childKey = if (isSplit) nextUnitKey++ else unit.key
                if (isSplit) {
                    // #708 评论 5728507555：child 继承父 unit 的 presented 状态，
                    // 让本次 applyPatch 后面的 started/pending 分类读到正确状态。
                    progressByKey[childKey] = parentPresented
                }
                if (slice.kind == ComposeVisualRebase.MappedRangeSliceKind.SURVIVING && slice.newSubRange != null) {
                    val mappedUnit =
                        mapSurvivingSlice(
                            unit = unit,
                            oldRange = slice.oldSubRange,
                            mappedRange = slice.newSubRange,
                            newLayout = newLayout,
                            frameTimeNanos = frameTimeNanos,
                            durationNanos = durationNanos,
                            childKey = childKey,
                        )
                    surviving.add(mappedUnit)
                    // #708 评论 5727808906：split 时父 unit 已 presented → surviving child 也算 presented，
                    // 避免下一笔 patch 把已可见的 surviving child 判成 pending 重建 alpha 0→1。
                    // ghost 不需要参与后续 surviving 的 presented 判定。
                    if (isSplit && parentPresented) {
                        presentedKeys.add(childKey)
                    }
                } else {
                    // 缺陷5 GHOST slice：按 slice.oldSubRange 创建 ghost，alpha 当前值 -> 0
                    // #708 评论 5728951138：记录被当前 patch 转成 ghost 的范围，
                    // reconcileDeletedGhosts 用它做差集避免整段补 ghost。
                    // slice.oldSubRange 与 patch.deletedUnits 同坐标系（当前正文坐标）。
                    currentPatchGhostedCoverage.add(slice.oldSubRange)
                    ghosting.add(
                        toGhost(
                            unit = unit,
                            now = frameTimeNanos,
                            durationNanos = durationNanos,
                            ghostRange = slice.oldSubRange,
                            childKey = childKey,
                        ),
                    )
                }
            }
        }
    }

    // #708 评论 5725706551：切片逻辑已抽取到 ComposeVisualRebase.computeSlices 共享 helper。
    private fun computeSlices(
        target: TextRange,
        offsetMap: List<VisualOffsetMapEntry>?,
        newTextLength: Int,
        intent: EditorVisualIntent? = null,
    ): List<ComposeVisualRebase.MappedRangeSlice> =
        ComposeVisualRebase.computeSlices(target, offsetMap, newTextLength, intent)

    // #708 评论 5725706551：与 ComposeLocalHandoffRebase.mapSurvivingSliceToHandoff 对应。
    // timeline 版本：alpha 通道不变（继续原动画），position 在新位置变化时创建动画通道。
    // handoff 版本：alpha/position 都固定在当前可见值，不推进时间。
    // #708 评论 5727808906：显式接收 childKey — split 时由 mapSurvivingUnits 分配独立新 key，
    // 不 split 时传 unit.key 保留父 key。
    @Suppress("LongParameterList")
    private fun mapSurvivingSlice(
        unit: VisualTextUnit,
        oldRange: TextRange,
        mappedRange: TextRange,
        newLayout: ComposeLayoutSnapshot,
        frameTimeNanos: Long,
        durationNanos: Long,
        childKey: Long,
    ): VisualTextUnit {
        // 存活：alpha 通道不变（继续使用原 startedAtNanos）。
        // #708 评论 5727440517：position 通道起点用 sliceScreenPosition 计算 —
        // oldRange == unit.range 时返回父 unit 当前屏幕位置；
        // oldRange 是父 unit 真子区间时用"slice 自然位置 + 父 unit 当前位移"，
        // 不再直接用父 unit 左上角，避免 surviving slice 首帧跳到父 unit 开头位置。
        val parentCurrent = currentOffset(unit.position, frameTimeNanos) ?: unit.position.to
        val oldSlicePosition =
            ComposeVisualRebase.sliceScreenPosition(
                layout = unit.layout,
                parentRange = unit.range,
                sliceRange = oldRange,
                parentScreenPosition = parentCurrent,
            ) ?: parentCurrent
        val newPosition = computeUnitPosition(newLayout, mappedRange)
        val positionChannel =
            if (newPosition != null && newPosition != oldSlicePosition) {
                TimedOffset(
                    from = oldSlicePosition,
                    to = newPosition,
                    startedAtNanos = frameTimeNanos,
                    durationNanos = durationNanos,
                )
            } else {
                TimedOffset(
                    from = oldSlicePosition,
                    to = oldSlicePosition,
                    startedAtNanos = frameTimeNanos,
                    durationNanos = 0L,
                )
            }
        return unit.copy(
            key = childKey,
            layout = newLayout,
            range = mappedRange,
            targetRange = mappedRange,
            position = positionChannel,
        )
    }

    /**
     * 第三步：处理本 patch 新插入的 unit + 重新分段尚未开始的 surviving unit。
     *
     * #691 评论 5682970101：移除 queueTailEndNanos 串行 FIFO，改为 scene redirect 有界窗口。
     * 把 [pendingSurviving]（尚未开始的 surviving）+ 本 patch 新 insertedRanges 合并成待显示序列，
     * 按正文顺序（range.start）排序，在 [frameTimeNanos, frameTimeNanos + durationNanos] 有界窗口内
     * 均匀分段：n = 待显示序列长度，第 i 个的 startFraction = i/n, endFraction = (i+1)/n，
     * startedAt = frameTimeNanos + durationNanos * startFraction, duration = durationNanos / n。
     *
     * - 对 pendingSurviving 中的已有 unit：copy 并重设 alpha 通道（TimedFloat(0f, 1f, unitStartedAt, unitDuration)），
     *   layout/range/position 保持（range 已映射到新正文）。
     * - 对新 insertedRanges：创建新 VisualTextUnit（和原 createInsertedUnits 类似，但从有界窗口起点开始）。
     *
     * @param pendingSurviving 尚未开始的 surviving unit（alpha.startedAtNanos > frameTimeNanos）。
     * @param patch 本帧的屏幕 diff。
     * @param frameTimeNanos 当前帧时间戳。
     * @param durationNanos 文字动画时长（有界窗口长度）。
     * @return [RepartitionResult] 包含 allUnits（pendingSurviving 重设 alpha + 新 insertedUnits）
     *   和 repartitionedPending（仅 pendingSurviving 重设 alpha，给 cursor 合并用）。
     */
    private fun repartitionPendingAndInsertedUnits(
        pendingSurviving: List<VisualTextUnit>,
        patch: ComposeVisualPatch,
        frameTimeNanos: Long,
        durationNanos: Long,
    ): RepartitionResult {
        val newLayout = patch.newLayout
        val newTextLength = newLayout.result.layoutInput.text.length
        val validInsertedRanges = patch.insertedUnits.filter { it.start < it.end && it.end <= newTextLength }

        // 构建待显示序列：pendingSurviving + 新 insertedRanges，按正文顺序（range.start）排序。
        // 用 Pair<TextRange, VisualTextUnit?> 标记：second != null 表示 pendingSurviving 的已有 unit，
        // second == null 表示新 insertedRange（需要创建新 unit）。
        val displayItems: List<Pair<TextRange, VisualTextUnit?>> =
            (pendingSurviving.map { it.targetRange!! to it } + validInsertedRanges.map { it to null })
                .sortedBy { it.first.start }

        if (displayItems.isEmpty()) {
            return RepartitionResult(allUnits = emptyList(), repartitionedPending = emptyList())
        }

        val n = displayItems.size
        val allUnits = mutableListOf<VisualTextUnit>()
        val repartitionedPending = mutableListOf<VisualTextUnit>()
        for ((i, item) in displayItems.withIndex()) {
            val range = item.first
            val existingUnit = item.second
            val startFraction = if (n <= 1) 0f else i.toFloat() / n.toFloat()
            val endFraction = if (n <= 1) 1f else (i + 1).toFloat() / n.toFloat()
            val unitStartedAt = frameTimeNanos + (durationNanos * startFraction).toLong()
            val unitDuration = (durationNanos * (endFraction - startFraction)).toLong()
            if (existingUnit != null) {
                // pendingSurviving：copy 并重设 alpha 通道，layout/range/position 保持
                val repartitioned =
                    existingUnit.copy(
                        alpha = TimedFloat(0f, 1f, unitStartedAt, unitDuration),
                    )
                allUnits.add(repartitioned)
                repartitionedPending.add(repartitioned)
            } else {
                // 新 insertedRange：创建新 VisualTextUnit
                val position = computeUnitPosition(newLayout, range) ?: Offset.Zero
                allUnits.add(
                    VisualTextUnit(
                        key = nextUnitKey++,
                        layout = newLayout,
                        range = range,
                        targetRange = range,
                        alpha = TimedFloat(0f, 1f, unitStartedAt, unitDuration),
                        position = TimedOffset(position, position, frameTimeNanos, 0L),
                        // #703 评论 5710977972 缺陷2：新插入字角色 = Inserted，
                        // 由 cursor 从左向右裁切吐出。
                        role = VisualUnitRole.Inserted,
                    ),
                )
            }
        }
        return RepartitionResult(allUnits = allUnits, repartitionedPending = repartitionedPending)
    }

    /**
     * [repartitionPendingAndInsertedUnits] 的返回结果。
     *
     * @param allUnits 所有重新分段后的 unit（pendingSurviving 重设 alpha + 新 insertedUnits）。
     * @param repartitionedPending 仅 pendingSurviving 重设 alpha 后的 unit（给 cursor 合并用，
     *   不含新 inserted units，避免 cursor 把新 inserted 当 surviving 重复计算 endFraction）。
     */
    private data class RepartitionResult(
        val allUnits: List<VisualTextUnit>,
        val repartitionedPending: List<VisualTextUnit>,
    )

    /**
     * #708 评论 5728951138：删除 ghost 所有权 + schedule 收口 —
     * 合并旧 createDeletedGhosts + rescheduleDeletedGhosts，
     * 把 timeline 的删除所有权收口成和 handoff 一样的"当前 patch 已接管范围 + 剩余范围"。
     *
     * 背景：handoff 的"部分删除差集"已修（#708 评论 5726837636），但 timeline 的
     * createDeletedGhosts 还在用 exact-match（it.range == range）判断，只认"range 完全相等"。
     * 这会导致：handoff 首帧不重影，但 timeline drain 下一帧又整段补 ghost 重新重影。
     *
     * 对每个 deletedRange：
     * 1. 用 [ComposeVisualRebase.subtractRanges] 算 remaining = deletedRange - currentPatchGhostedCoverage。
     *    currentPatchGhostedCoverage 是当前 patch 已把 active unit 转成 ghost 的范围
     *    （fullyDeleted 分支 + mapSurvivingUnits GHOST slice），这些范围已由 active unit 接管，
     *    不应再为它们建整段 alpha=1 ghost。
     * 2. 为 remaining 的每个子范围建 alpha=1 的完整 ghost（从 oldLayout 取旧位置）。
     * 3. 给属于这个 deletedUnit 的所有 ghost（range 在 deletedRange 内的 ghost，
     *    包括 active 转的 ghost 和新建的 remaining ghost）设 stage schedule：
     *    startedAt = frameTimeNanos + durationNanos * (i/n)，duration = durationNanos / n。
     *    schedule 匹配从旧 exact-range（ghost.range == range）改成"range 在 deletedRange 内"
     *    （ghost.range.start >= deletedRange.start && ghost.range.end <= deletedRange.end），
     *    让部分 slice（如 [1,2) 在 [0,2) 内）也能匹配到父 deletedUnit 的 stage schedule。
     *
     * #689 评论 5675270164 缺陷1：找不到 active unit 时从 oldLayout 建 ghost（alpha 1->0）。
     * #694 评论 5693864609 问题1 / 5694645209 问题3：删除 schedule — 有界窗口分段。
     * n = deletedRanges.size，unit i 的时间窗口为 [i/n, (i+1)/n]，
     * 这样快速连续删除多个字时每个 ghost 有自己的时间段，不会全挤在同一帧。
     * 所有本 patch 的 deleted ghost（包括从 active unit 转来的，也包括从 oldLayout 新建的）
     * 统一按 orderedDeletedUnits 的顺序设分段 schedule，正在动画的 active unit 被删除时也进入分段 schedule。
     *
     * @param ghosting 当前所有 ghost（包括从 active unit 转来的，也包括从 oldLayout 新建的）。
     * @param orderedDeletedUnits 本 patch 的 deletedUnits（按顺序）。
     * @param currentPatchGhostedCoverage 当前 patch 已把 active unit 转成 ghost 的范围
     *   （旧正文坐标系，与 orderedDeletedUnits 同坐标系）。
     * @param oldLayout 本 patch 的 oldLayout（建 ghost 取旧位置）。
     * @param frameTimeNanos 当前帧时间戳。
     * @param durationNanos 文字动画时长（有界窗口长度）。
     */
    @Suppress("LongParameterList")
    private fun reconcileDeletedGhosts(
        ghosting: MutableList<VisualTextUnit>,
        orderedDeletedUnits: List<TextRange>,
        currentPatchGhostedCoverage: List<TextRange>,
        oldLayout: ComposeLayoutSnapshot,
        frameTimeNanos: Long,
        durationNanos: Long,
    ) {
        val oldTextLength = oldLayout.result.layoutInput.text.length
        val orderedRanges = orderedDeletedUnits.filter { it.start < it.end && it.end <= oldTextLength }
        // #694 评论 5693864609 问题1：删除 schedule — 有界窗口分段
        // n = orderedRanges.size, unit i: [i/n, (i+1)/n]
        val n = orderedRanges.size
        if (n == 0) return
        for ((i, deletedRange) in orderedRanges.withIndex()) {
            // 1. 算剩余范围：deletedRange - currentPatchGhostedCoverage
            //    currentPatchGhostedCoverage 里的范围已由 active unit 接管（转成 ghost），
            //    不应再为它们建整段 alpha=1 ghost，只给剩余部分建 ghost。
            val remaining =
                ComposeVisualRebase.subtractRanges(
                    candidates = listOf(deletedRange),
                    blockers = currentPatchGhostedCoverage,
                )
            // 2. 为 remaining 建 alpha=1 完整 ghost（从 oldLayout 取旧位置）
            for (del in remaining) {
                if (del.start >= del.end) continue
                if (del.end > oldTextLength) continue
                // 防御性：检查 ghosting 里是否已有完全相同的 ghost（存活映射阶段已切片处理）
                if (ghosting.any { it.targetRange == null && it.range == del }) continue
                // 缺陷1：从 oldLayout 建 ghost（alpha 1->0）
                val oldPosition = computeUnitPosition(oldLayout, del) ?: continue
                ghosting +=
                    VisualTextUnit(
                        key = nextUnitKey++,
                        layout = oldLayout,
                        range = del,
                        targetRange = null,
                        // alpha=1 完整 ghost，schedule 下面统一设
                        alpha = TimedFloat(1f, 0f, frameTimeNanos, durationNanos),
                        position = TimedOffset(oldPosition, oldPosition, frameTimeNanos, 0L),
                        // #703 评论 5710977972 缺陷2：删除 ghost 角色 = DeletedGhost，
                        // 由 cursor 从右向左裁切吞掉。
                        role = VisualUnitRole.DeletedGhost,
                    )
            }
            // 3. 给属于这个 deletedUnit 的所有 ghost 设 stage schedule
            //    包括 active 转的 ghost（range 在 deletedRange 内）和新建的 remaining ghost。
            //    alpha.from 保持不变（当前真实 alpha，即 toGhost 时保留的 alphaNow），
            //    alpha.to = 0f。
            val startFraction = if (n <= 1) 0f else i.toFloat() / n.toFloat()
            val endFraction = if (n <= 1) 1f else (i + 1).toFloat() / n.toFloat()
            val ghostStartedAt = frameTimeNanos + (durationNanos * startFraction).toLong()
            val ghostDuration = (durationNanos * (endFraction - startFraction)).toLong()
            for (j in ghosting.indices) {
                val ghost = ghosting[j]
                // #708 评论 5728951138：schedule 匹配从 exact-range（ghost.range == range）
                // 改成"range 在 deletedRange 内"（ghost.range.start >= deletedRange.start &&
                // ghost.range.end <= deletedRange.end），让部分 slice（如 [1,2) 在 [0,2) 内）
                // 也能匹配到父 deletedUnit 的 stage schedule。
                if (ghost.targetRange == null &&
                    ghost.range.start >= deletedRange.start &&
                    ghost.range.end <= deletedRange.end
                ) {
                    ghosting[j] =
                        ghost.copy(
                            alpha =
                                TimedFloat(
                                    from = ghost.alpha.from,
                                    to = 0f,
                                    startedAtNanos = ghostStartedAt,
                                    durationNanos = ghostDuration,
                                ),
                        )
                }
            }
        }
    }

    /**
     * #689 评论 5675270164 缺陷4：retainedMoves 不能依赖 unit 事先存在。
     * 匹配不到 active unit 时从 patch.oldLayout + move.oldRange 创建 move unit。
     */
    private fun applyRetainedMoves(
        patch: ComposeVisualPatch,
        frameTimeNanos: Long,
        durationNanos: Long,
        surviving: MutableList<VisualTextUnit>,
    ) {
        val newLayout = patch.newLayout
        val newTextLength = newLayout.result.layoutInput.text.length
        for (move in patch.retainedMoves) {
            val newRange = move.newRange
            if (newRange.start >= newRange.end) continue
            if (newRange.end > newTextLength) continue
            val idx = surviving.indexOfFirst { it.targetRange == newRange }
            if (idx >= 0) {
                redirectExistingMoveUnit(surviving, idx, newRange, newLayout, frameTimeNanos, durationNanos)
            } else {
                createMoveUnitForReflow(patch, move, frameTimeNanos, durationNanos, surviving)
            }
        }
    }

    private fun redirectExistingMoveUnit(
        surviving: MutableList<VisualTextUnit>,
        idx: Int,
        newRange: TextRange,
        newLayout: ComposeLayoutSnapshot,
        frameTimeNanos: Long,
        durationNanos: Long,
    ) {
        val unit = surviving[idx]
        val newPosition = computeUnitPosition(newLayout, newRange) ?: return
        val oldPosition = currentOffset(unit.position, frameTimeNanos) ?: newPosition
        // 只有位置真变了才重定向 position 通道（删换行时几何没变的文字不产生 position track）
        val positionChannel =
            if (newPosition != oldPosition) {
                TimedOffset(oldPosition, newPosition, frameTimeNanos, durationNanos)
            } else {
                unit.position
            }
        // #703 评论 5712256296 缺口2：被判定为 retained move 的已有 unit，role 必须同步切换为 RetainedMove。
        // 否则原本 Inserted unit 被重定向后 role 仍是 Inserted，computeUnitClipFractions 仍按 cursor 裁切。
        surviving[idx] =
            unit.copy(
                position = positionChannel,
                role = VisualUnitRole.RetainedMove,
            )
    }

    private fun createMoveUnitForReflow(
        patch: ComposeVisualPatch,
        move: RetainedMove,
        frameTimeNanos: Long,
        durationNanos: Long,
        surviving: MutableList<VisualTextUnit>,
    ) {
        // 缺陷4：匹配不到 active unit，从 oldLayout + move.oldRange 创建 move unit
        val newRange = move.newRange
        val newLayout = patch.newLayout
        val oldPosition = computeUnitPosition(patch.oldLayout, move.oldRange) ?: return
        val newPosition = computeUnitPosition(newLayout, newRange) ?: return
        // 只有位置真变了才创建 move unit
        if (newPosition == oldPosition) return
        surviving +=
            VisualTextUnit(
                key = nextUnitKey++,
                layout = newLayout,
                range = newRange,
                targetRange = newRange,
                alpha = TimedFloat(1f, 1f, frameTimeNanos, 0L),
                position = TimedOffset(oldPosition, newPosition, frameTimeNanos, durationNanos),
                // #703 评论 5710977972 缺陷2：幸存回流文字角色 = RetainedMove，
                // 始终完整可见，不进入 spatial clip 裁切。
                role = VisualUnitRole.RetainedMove,
            )
    }

    /**
     * #708 评论 5723410606 第四节：独立 reflow 通道 —
     * 把 [ComposeVisualPatch.reflowMoves] 转成 [VisualTextUnit]。
     *
     * 规则（评论 5723410606 第四节）：
     * - ReflowMove alpha 永远 1；
     * - 不参加 cursor spatial clip（computeUnitClipFractions 给 fraction=1）；
     * - 只做 position old -> new；
     * - active 时把它的 newRange 放进 hiddenRanges（sample 已统一处理）；
     * - 完成后直接从 timeline 删除，下一帧由 BasicTextField 新位置接管（sample 收口）。
     *
     * 与 [applyRetainedMoves] 的区别：
     * - reflowMoves 携带 oldBounds/newBounds，不重新查 layout；
     * - reflowMoves 只对位置真变化的 slice 生成 unit（planner 已过滤）；
     * - reflowMoves 不复用已有 active unit 的 position 通道 — 它是独立轨道。
     */
    private fun applyReflowMoves(
        patch: ComposeVisualPatch,
        frameTimeNanos: Long,
        durationNanos: Long,
        surviving: MutableList<VisualTextUnit>,
    ) {
        if (patch.reflowMoves.isEmpty()) return
        val newLayout = patch.newLayout
        for (move in patch.reflowMoves) {
            val newRange = move.newRange
            if (newRange.start >= newRange.end) continue
            if (newRange.end > newLayout.result.layoutInput.text.length) continue
            // #708 评论 5724568261 缺口3：去重条件过窄修复 —
            // 旧条件只查 role==ReflowMove：`surviving.any { it.targetRange == newRange && it.role == VisualUnitRole.ReflowMove }`，
            // 不查 Inserted/RetainedMove。surviving 里已有 role=Inserted 的 unit 时，applyReflowMoves 不跳过，
            // 又创建第二个 role=ReflowMove，同一段文字被两个 overlay unit 同时画产生重影。
            //
            // 修复：用 subtractOverlayOwnedRanges 把 move.newRange 中已被其他 active unit
            // （targetRange != null 的存活 unit，含 Inserted/RetainedMove/ReflowMove）接管的范围切掉。
            // 只给没有被其他 active unit 接管的剩余 slice 创建 ReflowMove。
            // 已有 unit 已经经过 mapSurvivingSlice()，它的位置通道本身会根据新 layout 做 redirect，
            // 继续让原 unit 完成自己的动画即可。
            val remainingMoves = subtractOverlayOwnedRanges(move, surviving, patch)
            for (remaining in remainingMoves) {
                val nr = remaining.newRange
                if (nr.start >= nr.end) continue
                val oldPosition = Offset(remaining.oldBounds.left, remaining.oldBounds.top)
                val newPosition = Offset(remaining.newBounds.left, remaining.newBounds.top)
                surviving +=
                    VisualTextUnit(
                        key = nextUnitKey++,
                        layout = newLayout,
                        range = nr,
                        targetRange = nr,
                        // alpha 永远 1：始终全亮
                        alpha = TimedFloat(1f, 1f, frameTimeNanos, 0L),
                        // 只做 position old -> new
                        position = TimedOffset(oldPosition, newPosition, frameTimeNanos, durationNanos),
                        role = VisualUnitRole.ReflowMove,
                    )
            }
        }
    }

    /**
     * #708 评论 5725146968：从 [move.newRange] 中减去已被 [surviving] 中 active unit 接管的范围 —
     *
     * 使用 [ComposeOverlayOwnership.subtractOwnedRanges] 做真正的差集运算：
     * - 完全覆盖：返回空 list（不创建 ReflowMove）
     * - 无重叠：返回原 move
     * - 部分重叠：返回剩余 slice 的 ReflowMove 列表（每个 slice 的 old/new bounds 从真实 layout 取）
     *
     * 不允许同一个 UTF-16 target range 同时被两个 VisualTextUnit 拥有。
     */
    private fun subtractOverlayOwnedRanges(
        move: ComposeReflowMove,
        surviving: List<VisualTextUnit>,
        patch: ComposeVisualPatch,
    ): List<ComposeReflowMove> {
        val ownedRanges = surviving.mapNotNull { it.targetRange }
        return ComposeOverlayOwnership.subtractOwnedRanges(
            move = move,
            ownedRanges = ownedRanges,
            oldLayout = patch.oldLayout,
            newLayout = patch.newLayout,
        )
    }

    /**
     * 采样当前时间线到指定帧时间 — 返回当前应绘制的 [ComposeVisualScene]。
     *
     * #689 评论 5675270164 缺陷3：sample 后做收口 — 持续 timeline 只保存"当前仍需要
     * overlay 接管的东西"。
     * - 存活 unit（targetRange != null）：alpha==1 且 position 已到目标 -> 从 timeline 移除，交还 BasicTextField
     * - ghost unit（targetRange == null）：alpha==0 -> 删除
     * - 仍在 alpha/position 动画中的 unit -> 保留
     *
     * 收口要修改 timeline 内部的 units 列表（移除已稳定的 unit），不只是过滤返回值。
     * 否则 units 里残留的 unit 会在下次 applyPatch 时被处理，可能导致问题。
     *
     * @param frameTimeNanos 当前帧时间戳。
     * @return 当前场景（units + hiddenRanges）。
     */
    fun sample(frameTimeNanos: Long): ComposeVisualScene {
        // 缺陷3：收口 — 移除已稳定的 unit，只保留仍需 overlay 接管的 unit
        val remainingUnits = mutableListOf<VisualTextUnit>()
        val sampledUnits = mutableListOf<VisualTextUnit>()
        for (unit in units) {
            val sampled = sampleUnit(unit, frameTimeNanos)
            val target = sampled.targetRange
            val alphaFinished = isAlphaFinished(sampled.alpha, frameTimeNanos)
            val positionFinished = isPositionFinished(sampled.position, frameTimeNanos)
            if (target != null) {
                // 存活 unit：alpha==1 且 position 已到目标 -> 从 timeline 移除，交还 BasicTextField
                if (alphaFinished && positionFinished && sampled.alpha.to >= 1f) {
                    // #691 评论 5684993243 / 评论 5685940102：收口移除时同步清理 presentedKeys
                    presentedKeys.remove(unit.key)
                    continue
                }
            } else {
                // ghost unit：alpha==0 -> 删除
                if (alphaFinished && sampled.alpha.to <= 0f) {
                    // #691 评论 5684993243 / 评论 5685940102：ghost 收口移除时同步清理 presentedKeys
                    presentedKeys.remove(unit.key)
                    continue
                }
            }
            sampledUnits.add(sampled)
            remainingUnits.add(unit)
            // #691 评论 5684993243 / 评论 5685940102：只有真正 sample 到一个存活 unit 且该帧它已被
            // scene 接管并可见时，才把 unit.key 计入 presentedKeys。这个事实只能由可见帧推进，
            // 不会被同一 VSync 的 rebaseUnitForPatch 改写抹掉。
            // 必须用原始 unit（循环变量 unit）的 alpha/position 判断，不是 sampled 的 —
            // sampled 的 alpha.from 已被 sampleUnit rebase 成当前值，
            // currentAlpha(sampled.alpha, now) == sampled.alpha.from 永远成立，无法判断。
            // #691 评论 5685940102：isUnitVisibleAndPresented 同时覆盖插入 unit（alpha 0→1）
            // 和 retained reflow unit（alpha 1→1，position 已开始）。
            if (target != null && isUnitVisibleAndPresented(unit, frameTimeNanos)) {
                presentedKeys.add(unit.key)
            }
        }
        // 缺陷3：收口要修改 timeline 内部 units 列表（移除已稳定的 unit）
        units = remainingUnits
        // hiddenRanges：从当前 targetRange != null 且仍由 overlay 绘制的 unit 推导。
        // 收口后 sampledUnits 里的存活 unit 都是"仍由 overlay 接管"的（未稳定的），
        // 所以它们的 targetRange 都应在 hiddenRanges 中。
        val hiddenRanges =
            sampledUnits
                .filter { it.targetRange != null }
                .mapNotNull { it.targetRange }
                .filter { it.start < it.end }
        // #691：采样光标位置 — 与文字使用同一个 frameTimeNanos
        val sampledCursor = sampleCursorRect(frameTimeNanos)
        // #703 评论 B：空间进度驱动吞吐字 — 根据 cursor 位置算每个 unit 的可见 fraction。
        // 不再把 alpha 当作"这个字是否出现"的权威状态。
        // - 吐字（inserted unit, targetRange != null）：cursor 从 glyph 左侧向右侧移动，
        //   fraction = (cursor.left - glyph.left) / glyph.width，clamp 0..1。
        // - 吞字（deleted ghost, targetRange == null）：
        //   #703 评论 A 缺陷2 统一边界模型 — fraction = (cursor.left - glyph.left) / glyph.width，
        //   开始 cursor 在 glyph 右侧 fraction=1（完全可见），结束 cursor 在 glyph 左侧 fraction=0（被吞掉）。
        //   #703 评论 A 缺陷3 跨行裁切 — 不同行时 insert fraction=0、delete fraction=1。
        // cursor 为 null 或 glyph 退化为零宽时 fraction = 1f（完全可见，由 alpha 单独决定）。
        val computedClipFractions =
            if (sampledCursor != null) {
                computeUnitClipFractions(sampledUnits, sampledCursor)
            } else {
                emptyMap()
            }
        // #708 评论 5723410606 第二节：删除 barrier handoff 首帧的 redirectBaseClipFractions 覆盖 —
        // 不再有整屏 barrier redirect，clip fractions 直接用计算结果。
        val unitClipFractions = computedClipFractions
        return ComposeVisualScene(
            units = sampledUnits,
            hiddenRanges = hiddenRanges,
            cursorRect = sampledCursor,
            unitClipFractions = unitClipFractions,
            coordinatedSpatialClip = coordinatedSpatialClip,
        )
    }

    /**
     * #703 评论 B：空间进度驱动吞吐字 — 根据 cursor 位置算每个 unit 的可见 fraction。
     *
     * - 吐字（inserted unit, targetRange != null）：
     *   cursor 从 glyph 左侧向右侧移动，glyph 可见区域 = [glyph.left, cursor.left]。
     *   fraction = (cursor.left - glyph.left) / glyph.width，clamp 0..1。
     *   cursor.left <= glyph.left → fraction = 0（字不可见）。
     *   cursor.left >= glyph.right → fraction = 1（字完全可见）。
     * - 吞字（deleted ghost, targetRange == null）：
     *   #703 评论 A 缺陷2：统一边界模型 — delete 和 insert 的可见区域都用
     *   [glyph.left, glyph.left + glyph.width * fraction]，
     *   fraction = (cursor.left - glyph.left) / glyph.width，clamp 0..1。
     *   cursor 从 glyph 右侧向左侧移动：
     *   cursor.left >= glyph.right → fraction = 1（字仍完全可见，光标还没开始吞）。
     *   cursor.left <= glyph.left → fraction = 0（字已完全被吞掉）。
     *   draw 层对 insert 和 delete 都画 [left, left + width * fraction]。
     *
     * #703 评论 A 缺陷3：跨行裁切 — 必须先判断 cursor 和 glyph 是否在同一行。
     * 旧实现只比较 cursorRect.left 和 glyph bounds.left/right，跨行时下一行 glyph
     * 会被 coerceIn 成 1 提前完整出现。新实现先比较 top/bottom：
     * - cursor 和 glyph 不同行（cursorBottom <= glyphTop 或 cursorTop >= glyphBottom）：
     *   insert 分支 fraction = 0（光标还没到这一行，字不可见）。
     *   delete 分支 fraction = 1（光标还没退到这一行，字仍完整可见）。
     * - 同行时再按 cursorX 裁切。
     *
     * alpha 最多用于边缘柔化，不负责决定文字整体出现/消失。
     * draw 层用 fraction 裁切 glyph 可见区域。
     *
     * @param units 当前帧的 sampled units（alpha/position 已插值到当前帧）。
     * @param cursorRect 当前光标 rect。
     * @return unit key → 可见 fraction（0..1）。
     */
    private fun computeUnitClipFractions(
        units: List<VisualTextUnit>,
        cursorRect: Rect,
    ): Map<Long, Float> {
        if (units.isEmpty()) return emptyMap()
        val result = mutableMapOf<Long, Float>()
        for (unit in units) {
            // #708 评论 5727808906：抽取共享纯函数 ComposeVisualClip.fractionFor —
            // timeline 和 handoff rebase 后重建 unitClipFractions 共用同一份计算，
            // 避免 split 后 child key 查不到 fraction、三段文字共用父块空间进度。
            val fraction = ComposeVisualClip.fractionFor(unit, cursorRect, coordinatedSpatialClip)
            if (fraction != null) {
                result[unit.key] = fraction
            }
        }
        return result
    }

    /**
     * 是否还有活动动画 — overlay 据此决定是否继续推进帧时钟。
     *
     * #691：同时检查文字 units 和光标 cursorChannel 的活动状态。
     *
     * @param frameTimeNanos 当前帧时间戳。
     * @return true 表示还有 unit 的 alpha 或 position 通道未完成，或光标动画未完成。
     */
    fun hasActiveAnimation(frameTimeNanos: Long): Boolean {
        val textActive =
            units.any { unit ->
                !isAlphaFinished(unit.alpha, frameTimeNanos) ||
                    !isPositionFinished(unit.position, frameTimeNanos)
            }
        if (textActive) return true
        // #691：光标动画也算活动状态
        return hasActiveCursorAnimation(frameTimeNanos)
    }

    /**
     * 清除所有状态 — 章节切换或 detach 时调用。
     */
    fun clear() {
        units = emptyList()
        nextUnitKey = 1L
        cursorChannel = null
        // #691 评论 5684993243 / 评论 5685940102：清空已呈现 unit key 集合
        presentedKeys.clear()
        // #703 评论 5709208101 问题2：重置 coordinated + spatial clip 标记
        coordinatedSpatialClip = false
    }

    /**
     * #691 评论 5679242735 修改2：运行时 policy 切换时清掉旧 text units / ghost / cursorChannel。
     *
     * 由 [ComposeEditorVisualState.applyMotionPolicyAtFrame] 调用 —
     * 用户在动画进行中关闭文字动画或打开 reduce-motion 时，
     * 旧 patch 已带着原来的 insertedUnits/deletedUnits/retainedMoves 入队，
     * drain 时会再把文字动画重新启动。本方法把当前 timeline 里所有活动文字/光标动画清空，
     * 让后续 drain 用新 policy 重新决定是否创建 track。
     */
    fun settleForPolicyChange() {
        units = emptyList()
        cursorChannel = null
        // #691 评论 5684993243 / 评论 5685940102：policy 切换时清空已呈现 unit key 集合，
        // 让后续 drain 用新 policy 重新决定是否创建 track。
        presentedKeys.clear()
        // #703 评论 5709208101 问题2：重置 coordinated + spatial clip 标记
        coordinatedSpatialClip = false
    }

    // ==================== 统一光标位置（#691） ====================

    /**
     * #691：snapshot 光标到当前帧时间 — 返回当前位置。
     * 如果光标动画已完成，返回最终位置。
     *
     * #691 评论 5679242735 修改3：支持多段 [CursorTrack] 路径插值。
     */
    fun sampleCursorRect(frameTimeNanos: Long): Rect? {
        val ch = cursorChannel ?: return null
        return currentRect(ch, frameTimeNanos)
    }

    /**
     * #691：光标动画是否仍在进行。
     *
     * #691 评论 5679242735 修改3：基于 [CursorTrack] 判断。
     */
    fun hasActiveCursorAnimation(frameTimeNanos: Long): Boolean {
        val ch = cursorChannel ?: return false
        return !isCursorFinished(ch, frameTimeNanos)
    }

    /**
     * #691 评论 5679242735 修改3：计算 [CursorTrack] 在指定帧时间的当前位置。
     *
     * 多段路径插值：按 [CursorMotionPoint.endFraction] 把整条 timeline 分成多段，
     * 每段在前一段终点和本段目标点之间线性插值。
     * 单点路径退化为 from -> points[0].rect 的线性插值。
     */
    private fun currentRect(
        channel: CursorTrack,
        frameTimeNanos: Long,
    ): Rect {
        if (channel.durationNanos <= 0L) return channel.points.last().rect
        val elapsed = frameTimeNanos - channel.startedAtNanos
        if (elapsed <= 0L) return channel.fromRect
        if (elapsed >= channel.durationNanos) return channel.points.last().rect
        val progress = elapsed.toFloat() / channel.durationNanos.toFloat()
        val points = channel.points
        if (points.size == 1) {
            return interpolateRect(channel.fromRect, points[0].rect, progress)
        }
        var prevRect = channel.fromRect
        var prevFraction = 0f
        for (i in points.indices) {
            val point = points[i]
            if (progress <= point.endFraction || i == points.size - 1) {
                val segmentProgress =
                    if (point.endFraction > prevFraction) {
                        ((progress - prevFraction) / (point.endFraction - prevFraction)).coerceIn(0f, 1f)
                    } else {
                        1f
                    }
                return interpolateRect(prevRect, point.rect, segmentProgress)
            }
            prevRect = point.rect
            prevFraction = point.endFraction
        }
        return points.last().rect
    }

    /**
     * #691 评论 5679242735 修改3：两个 Rect 之间的线性插值。
     */
    private fun interpolateRect(
        from: Rect,
        to: Rect,
        t: Float,
    ): Rect =
        Rect(
            left = from.left + (to.left - from.left) * t,
            top = from.top + (to.top - from.top) * t,
            right = from.right + (to.right - from.right) * t,
            bottom = from.bottom + (to.bottom - from.bottom) * t,
        )

    /**
     * #691 评论 5679242735 修改3：[CursorTrack] 是否已完成。
     */
    private fun isCursorFinished(
        channel: CursorTrack,
        frameTimeNanos: Long,
    ): Boolean {
        if (channel.durationNanos <= 0L) return true
        return frameTimeNanos - channel.startedAtNanos >= channel.durationNanos
    }

    // ==================== 内部采样与通道计算 ====================

    /**
     * #689 评论 5675270164 缺陷2：把 unit 转成 ghost — alpha 从当前值继续到 0。
     *
     * #708 评论 5725706551：与 [ComposeLocalHandoffRebase.toHandoffGhost] 对应。
     * timeline 版本（本方法）：alpha 从当前值继续到 0（创建淡出动画通道），
     * position 固定在当前屏幕位置（考虑切片 ghost 的父 unit 位移）。
     * handoff 版本：alpha/position 都固定在当前可见值，不推进时间。
     *
     * 旧实现 `unit.copy(targetRange = null)` 只把 targetRange 设 null，没把 alpha 改成
     * "当前值 -> 0"。如果 unit 原来正在做插入动画（alpha 0->1），快速输入后马上删除，
     * 它会变成 ghost 继续淡入到 1，alpha 到 1 后 hasActiveAnimation 认为完成但 units 里
     * 没删掉，overlay 继续以 alpha=1 画在旧位置。
     *
     * @param unit 要转 ghost 的 unit。
     * @param now 当前帧时间。
     * @param durationNanos ghost 淡出时长。
     * @param ghostRange ghost 的 range；默认 unit.range（整个 unit 变 ghost）。
     * @param childKey ghost unit 的 key；默认 unit.key。
     *   #708 评论 5727808906：split 场景由 mapSurvivingUnits 分配独立新 key 传入；
     *   scene redirect 整块转 ghost（applyPatch 约 160 行 fullyDeleted 分支）保留原 key。
     */
    private fun toGhost(
        unit: VisualTextUnit,
        now: Long,
        durationNanos: Long,
        ghostRange: TextRange = unit.range,
        childKey: Long = unit.key,
    ): VisualTextUnit {
        val alphaNow = currentAlpha(unit.alpha, now)
        val parentCurrent = currentOffset(unit.position, now) ?: unit.position.to
        // #708 评论 5726837636：子片段 ghost 屏幕位置用共享 helper sliceScreenPosition 计算 —
        // ghostRange==unit.range 时返回 parentCurrent；否则 sliceNatural + parentDelta。
        // 与 ComposeLocalHandoffRebase.toHandoffGhost 共用同一套几何，避免两套算法不一致
        // 导致 handoff 首帧旧字跳位。
        val positionNow =
            ComposeVisualRebase.sliceScreenPosition(
                layout = unit.layout,
                parentRange = unit.range,
                sliceRange = ghostRange,
                parentScreenPosition = parentCurrent,
            ) ?: parentCurrent
        return unit.copy(
            key = childKey,
            range = ghostRange,
            targetRange = null,
            alpha = TimedFloat(alphaNow, 0f, now, durationNanos),
            position = TimedOffset(positionNow, positionNow, now, 0L),
            // #703 评论 5710977972 缺陷2：active unit 转 ghost，角色 = DeletedGhost，
            // 由 cursor 从右向左裁切吞掉。
            role = VisualUnitRole.DeletedGhost,
        )
    }

    /**
     * 采样单个 unit 到指定帧时间 — 返回 alpha/position 已插值后的 unit。
     * 采样后的 unit 的 alpha/position 通道表示"此刻屏幕真实画到的状态"。
     */
    private fun sampleUnit(
        unit: VisualTextUnit,
        frameTimeNanos: Long,
    ): VisualTextUnit {
        return unit.copy(
            alpha =
                unit.alpha.copy(
                    from = currentAlpha(unit.alpha, frameTimeNanos),
                    to = unit.alpha.to,
                    startedAtNanos = frameTimeNanos,
                    durationNanos = remainingDurationNanos(unit.alpha, frameTimeNanos),
                ),
            position =
                unit.position.copy(
                    from = currentOffset(unit.position, frameTimeNanos) ?: unit.position.from,
                    to = unit.position.to,
                    startedAtNanos = frameTimeNanos,
                    durationNanos = remainingDurationNanos(unit.position, frameTimeNanos),
                ),
        )
    }

    /**
     * #691 评论 5680711648 修复1：applyPatch 专用 rebase —
     * 把 unit 的 alpha/position 通道 rebase 到 [frameTimeNanos]，
     * 但**尚未开始的通道必须原样保留未来起点**。
     *
     * 与 [sampleUnit] 的关键区别：
     * - [sampleUnit] 用于 [sample] 画当前帧：把所有通道都 rebase 到 now，
     *   返回插值后的 unit 给 scene。对于尚未开始的通道，rebase 后
     *   from=currentAlpha=0f, startedAtNanos=now, durationNanos=remainingDuration。
     *   这会让"尚未开始"变成"从 now 开始"，丢失绝对 start time。
     * - [rebaseUnitForPatch] 用于 [applyPatch] 准备 surviving unit：尚未开始的通道
     *   原样保留（startedAtNanos 不变），这样下一笔 patch 不会让未来 unit 提前启动。
     *
     * alpha rebase 规则：
     * - now < channel.startedAtNanos：原样保留（尚未开始）
     * - now >= channel.startedAtNanos + channel.durationNanos：塌缩到 (to, to, now, 0)（已完成）
     * - 否则：from=currentAlpha(now), to=channel.to, startedAtNanos=now,
     *   durationNanos=channel.startedAtNanos + channel.durationNanos - now（进行中）
     *
     * position 通道同样处理。
     */
    private fun rebaseUnitForPatch(
        unit: VisualTextUnit,
        frameTimeNanos: Long,
    ): VisualTextUnit =
        unit.copy(
            alpha = rebaseTimedFloat(unit.alpha, frameTimeNanos),
            position = rebaseTimedOffset(unit.position, frameTimeNanos),
        )

    /**
     * #691 评论 5680711648 修复1：alpha 通道 rebase — 尚未开始原样保留。
     */
    private fun rebaseTimedFloat(
        channel: TimedFloat,
        now: Long,
    ): TimedFloat =
        when {
            // 尚未开始：原样保留未来起点
            now < channel.startedAtNanos -> channel
            // 已完成：塌缩到 to
            now >= channel.startedAtNanos + channel.durationNanos ->
                TimedFloat(channel.to, channel.to, now, 0L)
            // 进行中：从当前值继续到 to，剩余时长 = 原结束时间 - now
            else ->
                TimedFloat(
                    from = currentAlpha(channel, now),
                    to = channel.to,
                    startedAtNanos = now,
                    durationNanos = channel.startedAtNanos + channel.durationNanos - now,
                )
        }

    /**
     * #691 评论 5680711648 修复1：position 通道 rebase — 尚未开始原样保留。
     */
    private fun rebaseTimedOffset(
        channel: TimedOffset,
        now: Long,
    ): TimedOffset =
        when {
            // 尚未开始：原样保留未来起点
            now < channel.startedAtNanos -> channel
            // 已完成：塌缩到 to
            now >= channel.startedAtNanos + channel.durationNanos ->
                TimedOffset(channel.to, channel.to, now, 0L)
            // 进行中：从当前值继续到 to，剩余时长 = 原结束时间 - now
            else ->
                TimedOffset(
                    from = currentOffset(channel, now) ?: channel.from,
                    to = channel.to,
                    startedAtNanos = now,
                    durationNanos = channel.startedAtNanos + channel.durationNanos - now,
                )
        }

    /**
     * #691 评论 5685940102：判断 unit 这一帧是否实际被 scene 接管并可见 —
     * 用于决定是否计入 [presentedKeys]。
     *
     * 覆盖两种 unit：
     * - 插入 unit（alpha 0→1）：alpha 已离开起点（> 0）即算可见呈现。
     * - retained reflow unit（alpha 1→1）：alpha 本来就是 1，
     *   只要 position 通道已开始（frameTimeNanos >= position.startedAtNanos）即算可见呈现。
     *
     * 不覆盖：
     * - 尚未开始的未来 unit（alpha=0 且 position 未开始）。
     * - ghost unit（target == null，由调用方过滤）。
     */
    private fun isUnitVisibleAndPresented(
        unit: VisualTextUnit,
        frameTimeNanos: Long,
    ): Boolean {
        val alphaNow = currentAlpha(unit.alpha, frameTimeNanos)
        // alpha == 0：不可见，不算 presented
        if (alphaNow <= 0f) return false
        // alpha 已离开起点：插入 unit 已显示中间帧
        if (alphaNow != unit.alpha.from) return true
        // alpha 没离开起点但 > 0：retained reflow unit（alpha 1→1），
        // 只要 position 通道已开始/正在，就算 presented
        return frameTimeNanos >= unit.position.startedAtNanos
    }

    /**
     * #691 评论 5684993243 / 评论 5685940102：判断 unit 是否已在前一可见帧真正呈现过 —
     * 优先看 [presentedKeys] 持久状态，否则用 [isUnitVisibleAndPresented] 通道判断
     * "unit 是否已可见呈现"。
     *
     * #691 评论 5684136311 原始版本：从 TimedFloat.startedAtNanos 和 from 反推"是否已显示过"：
     *   frameTimeNanos > unit.alpha.startedAtNanos && alphaNow != unit.alpha.from
     * 这在"同一 VSync 连续 patch"场景会误判 — 第一笔 patch 的 rebaseUnitForPatch 把进行中通道
     * rebase 成 from=currentAlpha(now), startedAtNanos=now，第二笔 patch 时 frameTimeNanos == startedAtNanos，
     * 30 > 30 == false，已显示过的 unit 被误判成"零进度 pending"，alpha 跳回 0。
     *
     * #691 评论 5684993243 修复：优先看 [presentedKeys] 持久状态 —
     * 这个事实只能由真正的 sample()/可见帧推进，不会被同一 VSync 的 rebaseUnitForPatch 改写抹掉。
     * 这处理"同一 VSync 连续 patch"场景：第一笔 rebase 改写 startedAtNanos 后，
     * 第二笔仍能通过 presentedKeys 知道 a 已显示过。
     *
     * #691 评论 5685940102：回退逻辑从"alpha 是否离开起点"扩展到 [isUnitVisibleAndPresented]，
     * 覆盖 retained reflow unit（alpha 1→1，position 已开始）。
     * 否则 retained reflow unit 在"不同时间 patch 但中间未 sample"场景会被漏判，
     * 下一笔 patch 把它重建 alpha 0→1，已可见的文字突然变透明再淡入。
     *
     * 必须用 rebase 前的原始 unit 调用 — rebase 会把进行中通道的 startedAt 重设为
     * frameTimeNanos，丢失"是否同一 VSync"信息。
     */
    private fun hasBeenPresented(
        unit: VisualTextUnit,
        frameTimeNanos: Long,
    ): Boolean {
        // #691 评论 5684993243：优先看 presentedKeys 持久状态。
        if (unit.key in presentedKeys) return true
        // #691 评论 5685940102：否则用通道判断"unit 是否已可见呈现"。
        // 这处理"不同时间 patch 但中间未 sample"场景。
        return isUnitVisibleAndPresented(unit, frameTimeNanos)
    }

    /**
     * 计算通道当前值（alpha）。
     * durationNanos <= 0 表示瞬时完成，直接返回 to。
     */
    private fun currentAlpha(
        channel: TimedFloat,
        frameTimeNanos: Long,
    ): Float {
        if (channel.durationNanos <= 0L) return channel.to
        val elapsed = frameTimeNanos - channel.startedAtNanos
        if (elapsed <= 0L) return channel.from
        if (elapsed >= channel.durationNanos) return channel.to
        val t = elapsed.toFloat() / channel.durationNanos.toFloat()
        return channel.from + (channel.to - channel.from) * t
    }

    /**
     * 计算通道当前值（offset）。
     */
    private fun currentOffset(
        channel: TimedOffset,
        frameTimeNanos: Long,
    ): Offset? {
        if (channel.durationNanos <= 0L) return channel.to
        val elapsed = frameTimeNanos - channel.startedAtNanos
        if (elapsed <= 0L) return channel.from
        if (elapsed >= channel.durationNanos) return channel.to
        val t = elapsed.toFloat() / channel.durationNanos.toFloat()
        return Offset(
            channel.from.x + (channel.to.x - channel.from.x) * t,
            channel.from.y + (channel.to.y - channel.from.y) * t,
        )
    }

    /**
     * 通道是否已完成（alpha）。
     */
    private fun isAlphaFinished(
        channel: TimedFloat,
        frameTimeNanos: Long,
    ): Boolean {
        if (channel.durationNanos <= 0L) return true
        return frameTimeNanos - channel.startedAtNanos >= channel.durationNanos
    }

    /**
     * 通道是否已完成（position）。
     */
    private fun isPositionFinished(
        channel: TimedOffset,
        frameTimeNanos: Long,
    ): Boolean {
        if (channel.durationNanos <= 0L) return true
        return frameTimeNanos - channel.startedAtNanos >= channel.durationNanos
    }

    /**
     * 通道剩余 duration — 采样后用。
     */
    private fun remainingDurationNanos(
        channel: TimedFloat,
        frameTimeNanos: Long,
    ): Long {
        if (channel.durationNanos <= 0L) return 0L
        return max(0L, channel.startedAtNanos + channel.durationNanos - frameTimeNanos)
    }

    private fun remainingDurationNanos(
        channel: TimedOffset,
        frameTimeNanos: Long,
    ): Long {
        if (channel.durationNanos <= 0L) return 0L
        return max(0L, channel.startedAtNanos + channel.durationNanos - frameTimeNanos)
    }

    /**
     * #691 评论 5681258225：从 layout 快照安全取 cursor rect。
     * offset 越界或 layout 抛异常时返回 null。
     */
    private fun safeCursorRectFromLayout(
        layout: ComposeLayoutSnapshot,
        offset: Int,
    ): Rect? =
        try {
            layout.cursorRect(offset)
        } catch (_: Throwable) {
            null
        }

    /**
     * 从 layout 取 unit 的真实位置（左上角）。
     */
    private fun computeUnitPosition(
        layout: ComposeLayoutSnapshot,
        range: TextRange,
    ): Offset? {
        val bounds = ComposeVisualRebase.safePathBounds(layout.result, range) ?: return null
        return Offset(bounds.left, bounds.top)
    }

    companion object {
        /** 1 ms = 1_000_000 ns。 */
        private const val NANOS_PER_MS = 1_000_000L
    }
}

/**
 * #689 评论 5674631257 步骤2：带时间戳的 float 通道 —
 * 从 [from] 到 [to]，从 [startedAtNanos] 开始，持续 [durationNanos]。
 *
 * 通道创建后 startedAtNanos 不被重置（除非位置真变了重建 position 通道）。
 * 快速输入第二个字不能让第一个字重新从 0 开始。
 */
data class TimedFloat(
    val from: Float,
    val to: Float,
    val startedAtNanos: Long,
    val durationNanos: Long,
)

/**
 * #689 评论 5674631257 步骤2：带时间戳的 offset 通道。
 */
data class TimedOffset(
    val from: Offset,
    val to: Offset,
    val startedAtNanos: Long,
    val durationNanos: Long,
)

/**
 * #703 评论 5710977972 缺陷2：VisualTextUnit 的视觉角色 —
 * 区分 inserted（吐字）/ deletedGhost（吞字）/ retainedMove（幸存回流）/ reflowMove（独立 reflow），
 * 不再用 targetRange!=null 间接判断。
 *
 * - [Inserted]：本 patch 新插入的字，由 cursor 从左向右裁切吐出。
 * - [DeletedGhost]：本 patch 删除的 ghost 字，由 cursor 从右向左裁切吞掉。
 * - [RetainedMove]：幸存回流文字（retainedMoves 创建），始终完整可见，
 *   不进入 spatial clip 裁切（computeUnitClipFractions 直接给 fraction=1）。
 * - [ReflowMove]：#708 评论 5723410606 第四节独立 reflow 通道 —
 *   字仍存在，但从 oldBounds 平移到 newBounds，始终全亮（alpha 永远 1）。
 *   不参加 cursor spatial clip；只做 position old -> new；
 *   active 时把它的 newRange 放进 hiddenRanges，让 BasicTextField 的新位置暂时不重复画；
 *   完成后直接从 timeline 删除，下一帧由 BasicTextField 新位置接管。
 */
enum class VisualUnitRole { Inserted, DeletedGhost, RetainedMove, ReflowMove }

/**
 * #689 评论 5674631257 步骤2：单个文字单元的持续视觉状态。
 *
 * @param key 唯一标识 — 快速输入时不重置。
 * @param layout 当前所属 layout 快照。
 * @param range 在 [layout] 中的 UTF-16 range。
 * @param targetRange 在当前 new text 中的目标 range —
 *   null = ghost unit（只属于旧画面，最终应消失）；
 *   非 null = 仍存活，overlay 绘制时用此 range。
 * @param alpha 透明度通道 — 互不重置。
 * @param position 位置通道 — 互不重置；位置没变不重建。
 * @param role #703 评论 5710977972：视觉角色 —
 *   [VisualUnitRole.Inserted] / [VisualUnitRole.DeletedGhost] / [VisualUnitRole.RetainedMove]。
 *   默认 [VisualUnitRole.RetainedMove]：普通幸存 copy（data class copy 自动保留原 role）
 *   不参与吞吐裁切，始终完整可见。
 */
data class VisualTextUnit(
    val key: Long,
    val layout: ComposeLayoutSnapshot,
    val range: TextRange,
    val targetRange: TextRange?,
    val alpha: TimedFloat,
    val position: TimedOffset,
    val role: VisualUnitRole = VisualUnitRole.RetainedMove,
)

/**
 * #691：带时间戳的 Rect 通道 —
 * 从 [from] 到 [to]，从 [startedAtNanos] 开始，持续 [durationNanos]。
 *
 * 用于光标位置动画，与文字 timeline 共享同一个 frame clock。
 *
 * #691 评论 5679242735 修改3：cursor 不再用本类，改用 [CursorTrack] 支持多段路径。
 * 保留本数据类以兼容可能的其他引用。
 */
data class TimedRect(
    val from: Rect,
    val to: Rect,
    val startedAtNanos: Long,
    val durationNanos: Long,
)

/**
 * #691 评论 5679242735 修改3：带时间戳的多段光标路径通道 —
 * 从 [fromRect] 出发，依次经过 [points] 中每个 [CursorMotionPoint]，
 * 从 [startedAtNanos] 开始，持续 [durationNanos]。
 *
 * 单点路径退化为 fromRect -> points[0].rect 的线性插值。
 * 多点路径按 [CursorMotionPoint.endFraction] 分段插值 —
 * 一次提交多个插入 unit 时光标依次经过每个字/cluster，不再被压成一条直线。
 *
 * 与文字 timeline 共享同一个 frame clock。
 */
data class CursorTrack(
    val fromRect: Rect,
    val points: List<CursorMotionPoint>,
    val startedAtNanos: Long,
    val durationNanos: Long,
)

/**
 * #689 评论 5674631257 步骤2：一帧的视觉场景 — sample() 返回。
 *
 * #691：新增 [cursorRect] — 光标位置由同一个 timeline / frame clock 采样，
 * 不再由独立的 Animatable<Rect> 维护。
 *
 * #703 评论 B：新增 [unitClipFractions] — 空间进度驱动吞吐字。
 * 不再把 alpha 当作"这个字是否出现"的权威状态。
 * 光标经过哪里，字才出现/消失到哪里。
 * - 吐字（inserted unit）：cursor 从 glyph 左侧向右侧移动，
 *   glyph 可见区域由 cursor X 裁切；光标走到哪里，字吐到哪里。
 * - 吞字（deleted ghost）：#703 评论 A 缺陷2 统一边界模型 —
 *   fraction = (cursor.left - glyph.left) / glyph.width，
 *   开始 cursor 在 glyph 右侧 fraction=1（完全可见），结束 cursor 在 glyph 左侧 fraction=0（被吞掉）。
 *   #703 评论 A 缺陷3 跨行裁切 — 不同行时 insert fraction=0、delete fraction=1。
 * alpha 最多用于边缘柔化，不负责决定文字整体出现/消失。
 *
 * @param units 当前所有文字单元（alpha/position 已插值到当前帧）。
 * @param hiddenRanges 当前应由 overlay 接管、BasicTextField 需设透明的 ranges。
 *   每一帧直接从当前 [VisualTextUnit.targetRange] != null 且仍由 overlay 绘制的 unit 推导，
 *   不从"上一事务 suppressed ranges"继承。
 * @param cursorRect 光标当前位置（已插值到当前帧）— null 表示无光标动画且无静止光标。
 * @param unitClipFractions #703 评论 B：每个 unit 的空间进度可见 fraction（0..1）。
 *   key = [VisualTextUnit.key]，value = 可见 fraction。
 *   1f = 完全可见（cursor 已越过整个 glyph），0f = 完全不可见（cursor 还没到 glyph）。
 *   draw 层用此 fraction 裁切 glyph 可见区域，不再纯靠 alpha 决定出现/消失。
 * @param coordinatedSpatialClip #703 评论 5709208101 问题2：coordinated + spatial clip 模式标记。
 *   true 表示当前 patch 处于 textEnabled && cursorEnabled && coordinated 模式，
 *   draw 层据此用 clipFraction 覆盖 alpha（effective alpha=1），
 *   让整字亮度固定由空间裁切控制，而非 alpha 通道独立控制。
 *   alpha 通道仍保持 0->1 / 1->0 供非 coordinated 场景和现有测试使用。
 */
data class ComposeVisualScene(
    val units: List<VisualTextUnit>,
    val hiddenRanges: List<TextRange>,
    val cursorRect: Rect? = null,
    val unitClipFractions: Map<Long, Float> = emptyMap(),
    val coordinatedSpatialClip: Boolean = false,
) {
    companion object {
        /** 空场景。 */
        val Empty = ComposeVisualScene(units = emptyList(), hiddenRanges = emptyList())
    }
}
