package com.xiwei.sujian.feature.editor.visual

import android.util.Log
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlin.collections.ArrayDeque

/**
 * #641 评论1 第4/5节：Compose 显示层视觉状态。
 *
 * #689 评论 5674631257 步骤7：把视觉动画从"事务重启"改成"持续时间线"。
 *
 * 删除：
 * - _activeTransaction / activeTransaction
 * - _activeIntent / activeIntent
 * - _masterProgress / masterProgress
 * - reportProgress()
 * - finishTransaction()
 * - applyFrameUpdate() 里 _masterProgress = 0f
 *
 * 改成持有：
 * - [frameCoordinator]（只返回 [ComposeVisualPatch]）
 * - [visualTimeline]（长期持续视觉状态）
 * - [_visualScene]（每次 sample 后同步给 overlay）
 *
 * 时间戳来自 Compose frame clock（由 overlay 调用 [applyVisualPatchAtFrame] /
 * [sampleVisualScene] 时传入），不在这里用 `System.nanoTime()` 猜当前帧。
 *
 * @param targetId 当前编辑目标 ID — 用于结构化诊断事件。
 * @param initialDrawsVisualCursor 初始视觉光标状态 — smooth cursor 开启时从 attach 后一直为 true。
 */
class ComposeEditorVisualState(
    private val targetId: String,
    initialDrawsVisualCursor: Boolean = false,
) {
    companion object {
        private const val TAG = "EditorVisualState"

        /** 1 ms = 1_000_000 ns。 */
        private const val NANOS_PER_MS = 1_000_000L
    }

    /** 帧协调器 — 只回答"旧屏幕帧到新屏幕帧改了什么"。 */
    private val frameCoordinator = ComposeVisualFrameCoordinator(targetId)

    /** 持续视觉时间线 — 真正长期存在的屏幕动画状态。 */
    private val visualTimeline = ComposeVisualTimeline()

    /** 最新 layout 快照 — 供 overlay 读取 bounding box。 */
    private val _latestLayout = MutableStateFlow<ComposeLayoutSnapshot?>(null)
    val latestLayout: StateFlow<ComposeLayoutSnapshot?> = _latestLayout.asStateFlow()

    /**
     * 当前应由 overlay 接管、BasicTextField 需设透明的 ranges —
     * 每一帧直接从当前 [VisualTextUnit.targetRange] != null 且仍由 overlay 绘制的 unit 推导，
     * 不从"上一事务 suppressed ranges"继承。
     */
    private val _hiddenRanges = MutableStateFlow<List<TextRange>>(emptyList())
    val hiddenRanges: StateFlow<List<TextRange>> = _hiddenRanges.asStateFlow()

    /**
     * 视觉光标是否由 overlay 绘制 —
     * smooth cursor 开启：编辑器 attach 以后一直为 true（系统光标透明）。
     * smooth cursor 关闭：一直为 false（系统光标正常画）。
     * 仅由设置/attach 生命周期决定，不在某笔 patch 到达时改写。
     */
    private val _drawsVisualCursor = MutableStateFlow(initialDrawsVisualCursor)
    val drawsVisualCursor: StateFlow<Boolean> = _drawsVisualCursor.asStateFlow()

    /**
     * 当前视觉场景 — overlay 读取绘制。
     * 每次 [sampleVisualScene] 后更新。
     */
    private val _visualScene = MutableStateFlow(ComposeVisualScene.Empty)
    val visualScene: StateFlow<ComposeVisualScene> = _visualScene.asStateFlow()

    /**
     * 待消费的 patch 队列 — 解决快速输入时 LaunchedEffect 取消旧协程导致丢 patch 的问题。
     * 使用队列而非 conflated state，确保每一笔 patch 都能被处理。
     */
    private val pendingPatches = ArrayDeque<ComposeVisualPatch>()
    private val _patchVersion = MutableStateFlow(0L)
    val patchVersion: StateFlow<Long> = _patchVersion.asStateFlow()

    /**
     * 最新生成的 patch — 仅保留给日志/调试使用，timeline 输入不再依赖它。
     */
    private val _latestPatch = MutableStateFlow<ComposeVisualPatch?>(null)
    val latestPatch: StateFlow<ComposeVisualPatch?> = _latestPatch.asStateFlow()

    /**
     * #691：静止光标 rect — 当没有光标动画时，overlay 从这里读取光标的最终真实位置。
     * 由 [onAuthoritativeLayout] 更新，始终反映当前 selection 对应的光标几何。
     */
    private val _restingCursorRect = MutableStateFlow<Rect?>(null)
    val restingCursorRect: StateFlow<Rect?> = _restingCursorRect.asStateFlow()

    /**
     * #691 评论 5679242735 修改2：运行时 policy 切换的最新 effective policy。
     *
     * 非 null 时，[drainPendingPatchesAtFrame] 会把已入队 patch 的 motionPolicy 替换成它，
     * 防止旧 patch 带着原来的 insertedUnits/deletedUnits/retainedMoves 再进入 timeline
     * 把文字动画重新启动（用户已关闭文字动画或打开 reduce-motion）。
     */
    private var currentMotionPolicy: EditorMotionPolicy? = null

    /**
     * Core 视觉意图到达 — 只把 intent 交给 frameCoordinator，不启动动画、不改 layout。
     *
     * @param intent Core 视觉意图。
     * @param motionPolicy 动画策略 — 传入前先 effective() 收口 reduce-motion。
     */
    fun onVisualIntent(
        intent: EditorVisualIntent,
        motionPolicy: EditorMotionPolicy,
    ) {
        val update = frameCoordinator.onVisualIntent(intent, motionPolicy.effective())
        applyFrameUpdate(update)
    }

    /**
     * 系统给出权威布局 — 只记录，不修改输入几何。
     *
     * 得到 patch 后不要启动一笔新事务，只把 patch 暂存/发布给 overlay 的时间线入口。
     *
     * #691：同时更新 restingCursorRect — 当没有光标动画时 overlay 从这里读取最终位置。
     */
    fun onAuthoritativeLayout(
        result: TextLayoutResult,
        selection: TextRange,
        scrollY: Int,
    ) {
        val snapshot = ComposeLayoutSnapshot(result, selection, scrollY)
        _latestLayout.update { snapshot }

        // #691：更新静止光标 rect
        val cursorRect = computeCursorRectFromLayout(snapshot)
        _restingCursorRect.update { cursorRect }

        val update = frameCoordinator.onLayout(snapshot)
        applyFrameUpdate(update)
    }

    /**
     * 把帧协调器的更新结果应用到本地状态 — 暂存 patch 到待消费队列供 overlay 推进 timeline。
     */
    private fun applyFrameUpdate(update: FrameUpdate) {
        when (update) {
            is FrameUpdate.Empty -> {
                // 无新 patch — 首帧、无 pending、或 pending 与 layout 尚未匹配。
            }
            is FrameUpdate.NewPatch -> {
                pendingPatches.addLast(update.patch)
                _patchVersion.update { it + 1L }
                _latestPatch.update { update.patch }
                Log.d(
                    TAG,
                    "patch_published: id=${update.patch.id} " +
                        "coreTxnIds=${update.patch.coreTransactionIds} " +
                        "drawsVisualCursor=${_drawsVisualCursor.value}",
                )
            }
        }
    }

    /**
     * 在 Compose 帧时钟的回调里消费所有待处理的 patch 并应用到 timeline。
     *
     * overlay 监听 [patchVersion]，在 `withFrameNanos` 里调用本方法，
     * 把队列中所有 pending patch 逐个应用到 timeline。时间戳必须来自 Compose frame clock。
     *
     * #691：同时把光标 motion 并入 timeline — 与文字在同一个 applyPatch 内处理，
     * 保证 cursor 和 text units 使用同一个 frameTimeNanos。
     *
     * #691 评论 5679242735 修改2：如果 [currentMotionPolicy] 非 null，
     * 把已入队 patch 的 motionPolicy 替换成最新 policy，
     * 防止旧 patch 把文字动画重新启动。
     *
     * #691 评论 5679242735 修改3：cursor 参数从 (fromRect, toRect) 改成
     * (fromRect, path: List<CursorMotionPoint>)，支持多段路径。
     *
     * @param frameTimeNanos 当前帧时间戳（来自 Compose frame clock）。
     * @return 本次帧实际应用的 patch 列表。
     */
    fun drainPendingPatchesAtFrame(frameTimeNanos: Long): List<ComposeVisualPatch> {
        val applied = mutableListOf<ComposeVisualPatch>()
        while (pendingPatches.isNotEmpty()) {
            val raw = pendingPatches.removeFirst()
            // #691 评论 5679242735 修改2：用 currentMotionPolicy 替换 patch 的 motionPolicy
            val patch = currentMotionPolicy?.let { raw.copy(motionPolicy = it) } ?: raw
            // #691：光标 motion 与文字在同一个 applyPatch 调用内处理
            val cursorParams = computeCursorParamsForPatch(patch)
            visualTimeline.applyPatch(
                patch = patch,
                frameTimeNanos = frameTimeNanos,
                cursorFromRect = cursorParams?.fromRect,
                cursorPath = cursorParams?.points,
                cursorDurationNanos = cursorParams?.durationNanos ?: 0L,
            )
            applied += patch
        }
        return applied
    }

    /**
     * 是否还有待处理的 patch — overlay 据此决定是否继续推进帧时钟。
     */
    fun hasPendingPatches(): Boolean = pendingPatches.isNotEmpty()

    /**
     * #689 评论 5674631257 步骤7：在 Compose 帧时钟的回调里应用 patch 到 timeline。
     *
     * 已废弃 — 请改用 [drainPendingPatchesAtFrame]。
     * 保留此方法是为了兼容旧调用路径。
     *
     * @param patch 要应用的屏幕 diff。
     * @param frameTimeNanos 当前帧时间戳（来自 Compose frame clock）。
     */
    fun applyVisualPatchAtFrame(
        patch: ComposeVisualPatch,
        frameTimeNanos: Long,
    ) {
        visualTimeline.applyPatch(patch, frameTimeNanos)
    }

    /**
     * #689 评论 5674631257 步骤7：采样当前视觉场景 — overlay 在每帧 draw 前调用。
     *
     * 每次 sample 后把 [ComposeVisualScene.hiddenRanges] 同步给 [_hiddenRanges]，
     * [OutputTransformation] 继续只负责把这些正在由 overlay 画的最终正文 range 设透明。
     *
     * @param frameTimeNanos 当前帧时间戳（来自 Compose frame clock）。
     * @return 当前应绘制的视觉场景。
     */
    fun sampleVisualScene(frameTimeNanos: Long): ComposeVisualScene {
        val scene = visualTimeline.sample(frameTimeNanos)
        _visualScene.update { scene }
        _hiddenRanges.update { scene.hiddenRanges }
        return scene
    }

    /**
     * 是否还有活动动画 — overlay 据此决定是否继续推进帧时钟。
     *
     * @param frameTimeNanos 当前帧时间戳。
     */
    fun hasActiveVisuals(frameTimeNanos: Long): Boolean {
        return visualTimeline.hasActiveAnimation(frameTimeNanos)
    }

    // ==================== #691 统一光标位置 ====================

    /**
     * #691 评论 5679242735 修改3：光标 motion 参数 — fromRect + 完整 path + durationNanos。
     *
     * internal 可见性以便测试访问。
     */
    internal data class CursorMotionParams(
        val fromRect: Rect,
        val points: List<CursorMotionPoint>,
        val durationNanos: Long,
    )

    /**
     * #691：计算 patch 的光标 motion 参数 — 返回 [CursorMotionParams] 或 null。
     * 由 [drainPendingPatchesAtFrame] 传入 [ComposeVisualTimeline.applyPatch]，
     * 保证 cursor 和 text units 使用同一个 frameTimeNanos。
     *
     * #691 评论 5679242735 修改3：返回完整 path（List<CursorMotionPoint>），
     * 不再只取 path.points.last().rect。多字符一次提交时多段 cursor path 不再被压成一条直线。
     */
    private fun computeCursorParamsForPatch(patch: ComposeVisualPatch): CursorMotionParams? {
        val motionPolicy = patch.motionPolicy.effective()
        if (!motionPolicy.cursorEnabled) {
            // 光标动画关闭 — 不创建 cursorChannel，使用静态光标
            return null
        }

        val path = patch.cursorMotionPath
        if (path == null || path.points.isEmpty()) {
            // 无光标 motion — snap 到新 layout 的光标位置，返回单点 path
            val newCursorRect = computeCursorRectFromLayout(patch.newLayout) ?: return null
            return CursorMotionParams(
                fromRect = newCursorRect,
                points = listOf(CursorMotionPoint(rect = newCursorRect, endFraction = 1f)),
                durationNanos = 0L,
            )
        }

        // #691 评论 5679242735 修改3：fromRect = 旧 layout 真实光标位置
        // （取不到回退 path.points.first().rect），points = path.points 完整保留。
        val fromRect = computeCursorRectFromLayout(patch.oldLayout) ?: path.points.first().rect

        // #691 / 设置语义 G：coordinated=true 且有文字变化时，光标与文字共享 textDurationMillis
        // （用户设置）作为整条编辑视觉事务时长；不再用 Core intent 的 patch.durationMs。
        val durationNanos = patch.motionPolicy.effective().textDurationMillis.coerceAtLeast(0L) * NANOS_PER_MS

        // CURSOR_ONLY（没有文字视觉变化）：始终使用 cursorDurationMillis，
        // 即使 coordinated=true 也不跟随 textDurationMillis（见 EditorMotionPolicy 语义）。
        val isCursorOnly =
            patch.insertedUnits.isEmpty() &&
                patch.deletedUnits.isEmpty() &&
                patch.retainedMoves.isEmpty()
        val effectiveDurationNanos =
            if (motionPolicy.coordinated && !isCursorOnly) {
                // coordinated=true 且有文字变化：光标与文字共享整条编辑视觉事务时长。
                durationNanos
            } else {
                // coordinated=false，或 CURSOR_ONLY：光标使用独立的 cursorDurationMillis。
                motionPolicy.cursorDurationMillis.coerceAtLeast(0L) * NANOS_PER_MS
            }

        return CursorMotionParams(
            fromRect = fromRect,
            points = path.points,
            durationNanos = effectiveDurationNanos,
        )
    }

    /**
     * #691：从 layout + selection 计算光标 rect（屏幕坐标）。
     */
    private fun computeCursorRectFromLayout(layout: ComposeLayoutSnapshot): Rect? {
        return try {
            val selectionEnd =
                layout.selection.end.coerceIn(0, layout.result.layoutInput.text.length)
            layout.result.getCursorRect(selectionEnd)
        } catch (_: Throwable) {
            null
        }
    }

    /**
     * 清除所有状态 — 章节切换或 detach 时调用。
     */
    fun clear() {
        frameCoordinator.clear()
        visualTimeline.clear()
        pendingPatches.clear()
        _patchVersion.update { 0L }
        _latestLayout.update { null }
        _hiddenRanges.update { emptyList() }
        _visualScene.update { ComposeVisualScene.Empty }
        _latestPatch.update { null }
        _restingCursorRect.update { null }
        // #691 评论 5679242735 修改2：重置运行时 policy 切换状态
        currentMotionPolicy = null
        // 光标所有权只由设置/attach 决定，clear 不重置 _drawsVisualCursor。
    }

    /**
     * 设置 smooth cursor 状态 — 由外部设置变更驱动。
     * smooth cursor 开启时，编辑器 attach 以后系统光标一直透明。
     */
    fun setSmoothCursorEnabled(enabled: Boolean) {
        _drawsVisualCursor.update { enabled }
    }

    /**
     * #691 评论 5679242735 修改2：运行时 policy 切换 — 在帧边界应用新 motion policy。
     *
     * 场景：patch 已入队但还没 drain，此时用户关闭文字动画或打开 reduce-motion；
     * 旧 patch 会带着原来的 insertedUnits/deletedUnits/retainedMoves 再进入 timeline，
     * 把文字动画重新启动。本方法：
     * 1. 记录最新 effective policy 到 [currentMotionPolicy]，
     *    [drainPendingPatchesAtFrame] 会用它替换已入队 patch 的 motionPolicy。
     * 2. 调用 [ComposeVisualTimeline.settleForPolicyChange] 清掉旧 text units / ghost / cursorChannel。
     * 3. 把已入队 patch 的 motionPolicy 替换成最新 policy（防止旧 patch 重新启动文字动画）。
     *
     * @param newPolicy 新的动画策略 — 内部会先 effective() 收口 reduce-motion。
     */
    fun applyMotionPolicyAtFrame(newPolicy: EditorMotionPolicy) {
        val effective = newPolicy.effective()
        currentMotionPolicy = effective
        // 清掉旧 text units / ghost / cursorChannel
        visualTimeline.settleForPolicyChange()
        // 把已入队 patch 的 motionPolicy 替换成最新 policy
        if (pendingPatches.isNotEmpty()) {
            val updated = mutableListOf<ComposeVisualPatch>()
            while (pendingPatches.isNotEmpty()) {
                val p = pendingPatches.removeFirst()
                updated.add(p.copy(motionPolicy = effective))
            }
            updated.forEach { pendingPatches.addLast(it) }
        }
    }
}
