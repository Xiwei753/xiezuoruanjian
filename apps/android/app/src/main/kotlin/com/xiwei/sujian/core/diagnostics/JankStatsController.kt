package com.xiwei.sujian.core.diagnostics

import android.view.Window
import androidx.metrics.performance.JankStats
import java.util.concurrent.atomic.AtomicLong

/**
 * 卡顿统计控制器（Issue #612 四）。
 *
 * 用 Android 官方 [JankStats] 跟踪窗口帧，只把 `isJank == true` 的帧数据丢给
 * [DiagnosticsLogger] 持久日志线程；listener 里不做文件 I/O。
 *
 * 设计：
 * - 单例 [JankStatsController] 管理聚合统计（totalFrames / jankFrames /
 *   maxFrameDurationUiNanos / 按 screen+interaction 分组的 jank 数），
 *   per-window 的 JankStats listener 把数据报到单例。
 * - UI 上下文（screen / interaction）由调用方通过 [androidx.metrics.performance.PerformanceMetricsState]
 *   写入帧状态，JankStats listener 直接从 FrameData.states 读取，不再依赖本对象维护的上下文。
 * - 导出器直接调 [getSummary] 拿聚合数据写 jank_summary.json。
 *
 * 线程安全：聚合计数用 AtomicLong，分组 map 用 synchronized。
 * listener 由 JankStats 在主线程回调，不阻塞。
 *
 * JankStats 1.0.0 API 要点：
 * - FrameData 是顶级类，字段 frameStartNanos/frameDurationUiNanos/isJank/states。
 * - StateInfo 是顶级类，字段 key/value（不是 name/offset）。
 * - 开关用 `isTrackingEnabled = true/false` 属性（没有 enable/disable 方法）。
 */
internal object JankStatsController {
    private const val TAG = "SujianJank"

    private val totalFrames = AtomicLong(0L)
    private val jankFrames = AtomicLong(0L)
    private val maxFrameDurationUiNanos = AtomicLong(0L)

    /** 按 "screen=X,interaction=Y" 分组的 jank 帧数。 */
    private val jankByGroup = mutableMapOf<String, Long>()
    private val groupLock = Any()

    private var jankStats: JankStats? = null

    @Volatile private var trackingEnabled = false

    /**
     * 为 [window] 创建 JankStats 跟踪并注册 frame listener。
     * 幂等：重复调用先关闭旧实例再创建新的。
     */
    fun track(window: Window) {
        try {
            jankStats?.isTrackingEnabled = false
            jankStats =
                JankStats.createAndTrack(window) { frame ->
                    onFrame(
                        frameStartNanos = frame.frameStartNanos,
                        frameDurationUiNanos = frame.frameDurationUiNanos,
                        isJank = frame.isJank,
                        states = frame.states.map { state -> state.key to state.value },
                    )
                }
        } catch (e: Exception) {
            DiagnosticsLogger.w(TAG, "JankStats track failed", e)
        }
    }

    /** 开启 JankStats 跟踪。 */
    fun enable() {
        trackingEnabled = true
        try {
            jankStats?.isTrackingEnabled = true
        } catch (_: Exception) {
        }
    }

    /** 关闭 JankStats 跟踪。 */
    fun disable() {
        trackingEnabled = false
        try {
            jankStats?.isTrackingEnabled = false
        } catch (_: Exception) {
        }
    }

    /**
     * 返回 jank_summary 数据：
     * totalFrames / jankFrames / maxFrameDurationUiNanos / jankByGroup。
     */
    fun getSummary(): Map<String, Any?> {
        synchronized(groupLock) {
            return linkedMapOf<String, Any?>(
                "totalFrames" to totalFrames.get(),
                "jankFrames" to jankFrames.get(),
                "maxFrameDurationUiNanos" to maxFrameDurationUiNanos.get(),
                "jankByGroup" to jankByGroup.toMap(),
                "trackingEnabled" to trackingEnabled,
            )
        }
    }

    /** 重置聚合统计（测试与 clearLogs 用）。 */
    fun reset() {
        totalFrames.set(0L)
        jankFrames.set(0L)
        maxFrameDurationUiNanos.set(0L)
        synchronized(groupLock) {
            jankByGroup.clear()
        }
    }

    /**
     * Frame listener 回调（主线程）。只更新聚合统计 + 对 jank 帧调
     * [DiagnosticsLogger.i]，不做文件 I/O。
     *
     * [states] 是 FrameData.states 的 key-value 对列表，由调用方通过
     * [androidx.metrics.performance.PerformanceMetricsState.putState] 写入的持续状态
     * （如 "screen"=当前页面名、"interaction"=当前交互名）。
     *
     * 提取为 internal 可见函数便于单测验证聚合逻辑。
     */
    internal fun onFrame(
        frameStartNanos: Long,
        frameDurationUiNanos: Long,
        isJank: Boolean,
        states: List<Pair<String, String>>,
    ) {
        totalFrames.incrementAndGet()
        if (frameDurationUiNanos > maxFrameDurationUiNanos.get()) {
            maxFrameDurationUiNanos.set(frameDurationUiNanos)
        }
        if (!isJank) return
        jankFrames.incrementAndGet()
        val screen = states.firstOrNull { it.first == "screen" }?.second ?: "unknown"
        val interaction = states.firstOrNull { it.first == "interaction" }?.second
        val group = buildGroup(screen, interaction)
        synchronized(groupLock) {
            jankByGroup[group] = (jankByGroup[group] ?: 0L) + 1L
        }
        logJankFrame(frameStartNanos, frameDurationUiNanos, states, group)
    }

    /** 构造分组 key。提取为 internal 便于单测。 */
    internal fun buildGroup(
        screen: String,
        interaction: String?,
    ): String = if (interaction != null) "screen=$screen,interaction=$interaction" else "screen=$screen"

    private fun logJankFrame(
        frameStartNanos: Long,
        frameDurationUiNanos: Long,
        states: List<Pair<String, String>>,
        group: String,
    ) {
        val msg =
            buildString {
                append("jank frameStartNanos=")
                append(frameStartNanos)
                append(" frameDurationUiNanos=")
                append(frameDurationUiNanos)
                append(" isJank=true")
                append(" states=")
                append(states.joinToString(",") { (key, value) -> "$key=$value" })
                append(" group=")
                append(group)
            }
        DiagnosticsLogger.i(TAG, msg)
    }
}
